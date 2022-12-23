extern crate core;

use crate::atom_entry::AtomEntry;
use crate::date_index::DateIndex;
use crate::html_util::{cerealize, github_commit, http_get, parse_html};
use crate::traverse_dom::{TraverseAttrs, TraverseDom};
use chrono::{DateTime, Duration, FixedOffset, Utc};
use html5ever::tendril::StrTendril;
use markup5ever_rcdom::{Handle, RcDom};
use regex::Regex;
use std::cmp::Ordering;
use std::str::FromStr;
use std::{process::exit, u16};
use worker::{
    console_debug, console_error, console_warn, event, Env, ScheduleContext, ScheduledEvent,
};
use worker::wasm_bindgen::JsValue;

mod atom_entry;
mod date_index;
mod html_util;
mod traverse_dom;

const ERRATA_URL: &str = "https://www.openbsd.org/errata";
const MIN_VERSION: u16 = 22;
const REQUEST_LIMIT: u8 = 35;
const ISO_UTC_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%z";

const GITHUB_RAW_URL: &str =
    "https://github.com/AlbertGoma/syspatch-feed.albert.goma.cat/raw/main/pub/atom.xml";
const GITHUB_COMMIT_URL: &str = "https://api.github.com/repos/AlbertGoma/syspatch-feed.albert.goma.cat/contents/pub/atom.xml";
const GITHUB_COMMIT_MESSAGE: &str = "Automated atom feed update";
const GITHUB_COMMIT_EMAIL: &str = "58812649+AlbertGoma@users.noreply.github.com";
const GITHUB_API_VERSION: &str = "2022-11-28";

const FEED_HEADER_LINES: usize = 11;
const FEED_TITLE: &str = "OpenBSD Syspatch Feed";
const FEED_LINK: &str = "https://syspatch-feed.albert.goma.cat/atom.xml";
const FEED_LINK_REL: &str = "https://www.openbsd.org";
const FEED_AUTHOR_NAME: &str = "Albert Gomà i León";
const FEED_AUTHOR_URI: &str = "https://albert.goma.cat";
const FEED_UUID: &str = "tag:albert.goma.cat,2022:feed/openbsd/sypatch";

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
    version: u16,
    request_ctr: &mut u8,
) -> DateTime<FixedOffset> {
    match match errata_rgx.find(content) {
        Some(result) => DateTime::parse_from_str(
            &(String::from(result.as_str()) + " 00:00:00+0000"),
            "%B %d, %Y %T%z",
        ),
        None => {
            if let Some(ref idx) = date_idx.lazy_load(version, request_ctr).await {
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

fn render_feed(feed: &str, entries: &mut Vec<AtomEntry>, old_feed_last_id: Option<&str>) -> String {
    entries.sort_by(|a, b| match b.release_version.cmp(&a.release_version) {
        Ordering::Less => Ordering::Less,
        Ordering::Equal => b.iteration_count.cmp(&a.iteration_count),
        Ordering::Greater => Ordering::Greater,
    });

    //If you modify this make sure to update FEED_HEADER_LINES accordingly
    let mut new_feed = format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n",
            "<feed xmlns=\"http://www.w3.org/2005/Atom\">\n",
            "    <title>{title}</title>\n",
            "    <link rel=\"self\" href=\"{link}\"/>\n",
            "    <link rel=\"related\" href=\"{link_rel}\"/>\n",
            "    <updated>{updated}</updated>\n",
            "    <author>\n",
            "        <name>{author_name}</name>\n",
            "        <uri>{author_uri}</uri>\n",
            "    </author>\n",
            "    <id>{id}</id>\n",
        ),
        title = FEED_TITLE,
        link = FEED_LINK,
        link_rel = FEED_LINK_REL,
        updated = Utc::now().format(ISO_UTC_FORMAT),
        author_name = FEED_AUTHOR_NAME,
        author_uri = FEED_AUTHOR_URI,
        id = FEED_UUID
    );
    for entry in entries {
        match old_feed_last_id {
            Some(last_id) if entry.id.eq(last_id) => break,
            _ => (),
        }
        new_feed += &format!(
            concat!(
                "   <entry>\n",
                "       <id>{id_prefix}/{id}</id>\n",
                "       <title type=\"html\">{title}</title>\n",
                "       <updated>{updated}</updated>\n",
                "       <content type =\"html\">{content}</content>\n",
                "       <link rel=\"alternate\" type=\"text/html\" href=\"{link}\"/>\n",
                "   </entry>\n"
            ),
            id_prefix = "albert.goma.cat,2022:syspatch_feed",
            id = entry.id,
            title = html_escape::encode_safe(&entry.title),
            updated = entry.updated.format(ISO_UTC_FORMAT),
            content = html_escape::encode_safe(&entry.content),
            link = entry.link
        );
    }
    new_feed += "</feed>";

    console_debug!("{}", feed);
    new_feed + feed
}

fn get_last_version_n_patch(old_feed_last_id: Option<&str>) -> (u16, Option<&str>) {
    match old_feed_last_id {
        Some(id) => match (id.find("/").and_then(|b| Some(b + 1)), id.find("-")) {
            (Some(start), Some(end)) if id[start..].starts_with("v") => {
                match u16::from_str(&id[start + 1..end]) {
                    Ok(version) => (version, Some(&id[end + 1..id.len()])),
                    Err(e) => {
                        console_error!("Error parsing last feed's entry ID: {}", e);
                        exit(1);
                    }
                }
            }
            _ => {
                console_error!("Error parsing last feed's entry ID");
                exit(1);
            }
        },
        None => (MIN_VERSION, None),
    }
}

#[event(scheduled)]
pub async fn scheduled(_: ScheduledEvent, env: Env, _: ScheduleContext) {
    //#1 Download latest feed from GitHub
    let token = match env.secret("") { //FIXME: write name of the secret
        Ok(secret) => secret,
        Err(e) => {
            console_error!("Cannot retrieve GitHub's token: {}", e);
            exit(1);
        }
    };
    let mut request_ctr: u8 = 1; //must be lower than 50 anyway...
    let (mut feed, sha) = match http_get(GITHUB_RAW_URL).await {
        Ok(html) => html,
        Err(e) => {
            console_error!("Cannot retrieve previous feed file: {}", e);
            exit(1);
        }
    };

    //#2 Get last entry ID
    let old_feed_last_id = match &feed.lines().nth(FEED_HEADER_LINES + 1) {
        // has at least one entry
        Some(id_line) => Some(&id_line.trim()[4..id_line.len() - 5]),
        None => None,
    };

    let mut entries = Vec::<AtomEntry>::new();
    let errata_regex = match Regex::new(concat!(
        "(Jan|January|Feb|February|Mar",
        "|March|Apr|April|May|Jun|June",
        "|Jul|July|Aug|August|Sep|September",
        "|Oct|October|Nov|November|Dec|December)",
        "\\s\\d{1,2},\\s\\d{4}"
    )) {
        Ok(regex) => regex,
        Err(e) => {
            console_error!("Wrong regex: {}", e);
            exit(1);
        }
    };

    //#3 Parse last version and patch IDs
    let (mut version, _) = get_last_version_n_patch(old_feed_last_id);
    let mut date_idx = DateIndex::new(version);

    //#4 Fetch current release and the following ones until 404 or request limit
    loop {
        let errata_url = ERRATA_URL.to_owned() + &version.to_string() + ".html";

        request_ctr += 1;
        let (mut errata_html, _) = match http_get(&errata_url).await {
            Ok(html) => html,
            Err(_) => break,
        };

        //console_debug!("{:?}", date_idx);

        //#5 Parse the contents into a data structure
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
            let updated = get_updated_date(
                &errata_regex,
                &content,
                &id,
                &mut date_idx,
                &last_good_date,
                version,
                &mut request_ctr,
            )
            .await;
            last_good_date = updated.clone();
            let atom_entry = AtomEntry {
                id,
                title,
                updated,
                link: errata_url.clone(),
                content,
                release_version: version,
                iteration_count: i,
            };
            //console_debug!("{:?}", atom_entry);
            entries.push(atom_entry);
        }

        if request_ctr > REQUEST_LIMIT {
            break;
        }
        version += 1;
    }

    //#6 Render the feed and patch the old one
    let feed = base64::encode(render_feed(&feed, &mut entries, old_feed_last_id));

    //#7 Upload it back to GitHub
    github_commit(GITHUB_COMMIT_URL, token, &feed, sha);
}
