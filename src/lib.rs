extern crate core;

use crate::atom_entry::AtomEntry;
use crate::traverse_dom::{TraverseDom, TraverseAttrs};
use chrono::{DateTime, Duration, FixedOffset, Utc};
use html5ever::{tendril::StrTendril};
use markup5ever_rcdom::{Handle, RcDom};
use regex::Regex;
use std::{process::exit, u16};
use std::cmp::Ordering;
use std::str::FromStr;
use worker::{
    console_debug, console_error, console_warn, event, Env, ScheduleContext, ScheduledEvent
};
use crate::date_index::DateIndex;
use crate::html_util::{cerealize, get_html, parse_html};

mod atom_entry;
mod traverse_dom;
mod date_index;
mod html_util;

const ERRATA_URL: &str = "https://www.openbsd.org/errata";
const MIN_VERSION: u16 = 65;//22;
const ISO_UTC_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%z";


fn get_title(patch: &Handle, entries: &mut Vec<AtomEntry>, version: u16) -> Option<String> {
    match patch.first_child_by_name("strong") {
        Some(strong) => Some(strong.get_first_text()),
        None => {
            let last_entry = match entries
                .last_mut()
                .filter(|atom_entry| atom_entry.id.starts_with(&format!("v{}-", version)))
            {
                Some(last_entry) => last_entry,
                None => {
                    console_error!("Document Error: <li> without <strong> is first element");
                    exit(1);
                }
            }
            .content += &cerealize(patch.clone());
            //console_debug!("new_content = {:?}", last_entry);
            None
        }
    }
}

fn get_id(patch: &Handle, version: u16, iteration: usize) -> String {
    match patch.data.first_attr_by_name("id") {
        Some(id_attr) => format!("v{}-{}", version, id_attr),
        None => format!("v{}-*nopatch{:03}", version, iteration + 1),
    }
}

fn get_patches(dom: &RcDom) -> Vec<Handle> {
    match match match &dom.document.first_child_by_name("html") {
        Some(html) => html,
        None => {
            console_error!("Document Error: Missing <html> tag");
            exit(1);
        }
    }
    .first_child_by_name("body")
    {
        Some(body) => body,
        None => {
            console_error!("Document Error: Missing <body> tag");
            exit(1);
        }
    }
    .first_child_by_name("ul")
    {
        Some(ul) => ul.children_by_name("li"),
        None => {
            console_warn!("Document Error: Missing <ul> tag. New release?");
            Vec::<Handle>::new()
        }
    }
}

fn invent_date(last_good_date: &DateTime<FixedOffset>) -> DateTime<FixedOffset> {
    last_good_date
        .checked_add_signed(Duration::days(1))
        .unwrap_or(DateTime::default())
}


async fn get_updated_date(
    errata_rgx: &Regex,
    content: &str,
    id: &str,
    date_idx: &mut DateIndex,
    last_good_date: &DateTime<FixedOffset>,
    version: u16
) -> DateTime<FixedOffset> {
    match match errata_rgx.find(content) {
        Some(result) => DateTime::parse_from_str(
            &(String::from(result.as_str()) + " 00:00:00+0000"),
            "%B %d, %Y %T%z",
        ),
        None => {
            if let Some(ref idx) = date_idx.lazy_load(version).await {
                console_warn!("Date unavailable. Parsing from ftp...");
                let id = &id[id.find("-").map_or(0, |i| i + 1)..];

                let empty = StrTendril::new();
                let item = idx.get(id);
                //console_debug!("date_idx.get({}) = {:?}", id, item);
                let mut date_str = item.unwrap_or(&empty).trim_start();
                date_str = &date_str[..date_str.find(" ").unwrap_or(date_str.len())];
                let date_str = date_str.to_string() + " 00:00:00+0000";

                DateTime::parse_from_str(&date_str, "%d-%b-%Y %T%z")
            } else {
                Ok(invent_date(last_good_date))
            }
        }
    } {
        Ok(date) => date,
        Err(e) => {
            console_warn!("Date Parse Error: {} ---> Making up a new one", e);
            invent_date(last_good_date)
        }
    }
}

fn render_feed(entries: &mut Vec<AtomEntry>) -> String {
    entries.sort_by(|a, b| {
        match b.release_version.cmp(&a.release_version) {
            Ordering::Less => Ordering::Less,
            Ordering::Equal => b.iteration_count.cmp(&a.iteration_count),
            Ordering::Greater => Ordering::Greater,
        }
    });

    let mut feed = format!(concat!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n",
        "<feed xmlns=\"http://www.w3.org/2005/Atom\">\n",
        "   <title>{title}</title>\n",
        "   <link rel=\"related\" href=\"{link}\"/>\n",
        "   <updated>{updated}</updated>\n",
        "   <author>\n",
        "       <name>{author_name}</name>\n",
        "       <uri>{author_uri}</uri>\n",
        "   </author>\n",
        "   <id>{id}</id>\n",

    ),
        title = "OpenBSD Syspatch Feed",
        link = "https://www.openbsd.org",
        updated = Utc::now().format(ISO_UTC_FORMAT),
        author_name = "Albert Gomà i León",
        author_uri = "https://albert.goma.cat",
        id = "albert.goma.cat,2022:feed/openbsd/sypatch"
    );
    for entry in entries {
        feed += &format!(concat!(
        "   <entry>\n",
        "       <id>{id_prefix}{id}</id>\n",
        "       <title type=\"text\">{title}</title>\n",
        "       <updated>{updated}</updated>\n",
        "       <content type =\"html\">{content}</content>\n",
        "       <link rel=\"alternate\" type=\"text/html\" href=\"{link}\"/>\n",
        "   </entry>\n"
        ),
            id_prefix = "albert.goma.cat,2022:",
            id = entry.id,
            title = entry.title,
            updated = entry.updated.format(ISO_UTC_FORMAT),
            content = entry.content,
            link = entry.link
        );
    }

    feed += "</feed>";

    console_debug!("{}",feed);
    base64::encode(feed)
}

#[event(scheduled)]
pub async fn scheduled(_: ScheduledEvent, env: Env, _: ScheduleContext) {
    let mut entries = Vec::<AtomEntry>::new();
    let errata_regex = Regex::new(concat!(
        "(Jan|January|Feb|February|Mar",
        "|March|Apr|April|May|Jun|June",
        "|Jul|July|Aug|August|Sep|September",
        "|Oct|October|Nov|November|Dec|December)",
        "\\s\\d{1,2},\\s\\d{4}"
    ))
    .unwrap_or_else(|e| {
        console_error!("Wrong regex: {}", e);
        exit(1);
    });

    let mut version: u16 = MIN_VERSION;
    let mut date_idx = DateIndex::new(version);

    loop {
        let errata_url = ERRATA_URL.to_owned() + &version.to_string() + ".html";

        let mut errata_html = match get_html(&errata_url).await {
            Ok(html) => html,
            Err(_) => break,
        };

        //console_debug!("{:?}", date_idx);

        let errata_dom = parse_html(&mut errata_html);
        let patches = get_patches(&errata_dom);
        let mut last_good_date = DateTime::<FixedOffset>::default();

        for (i, patch) in patches.iter().enumerate() {
            // The contents of entries without a title belong to the previous one. Handled
            // inside get_title()
            let title = match get_title(patch, &mut entries, version) {
                Some(title) => title,
                None => continue,
            };

            let content = cerealize(patch.clone());
            let id = get_id(patch, version, i);
            let updated =
                get_updated_date(&errata_regex, &content, &id, &mut date_idx, &last_good_date, version).await;
            last_good_date = updated.clone();
            let atom_entry = AtomEntry {
                id,
                title,
                updated,
                link: errata_url.clone(),
                content,
                release_version: version,
                iteration_count: i
            };
            //console_debug!("{:?}", atom_entry);
            entries.push(atom_entry);
        }

        version += 1;
    }

    //Render Feed
    let feed = render_feed(&mut entries);
    //Upload to git
}
