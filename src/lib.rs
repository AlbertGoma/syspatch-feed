extern crate core;

use crate::atom_entry::AtomEntry;
use crate::traverse::{Traverse, TraverseAttrs};
use chrono::{DateTime, Duration, FixedOffset};
use futures::try_join;
use html5ever::tendril::StrTendril;
use html5ever::{parse_document, serialize, tendril::TendrilSink, ParseOpts};
use markup5ever_rcdom::{Handle, RcDom, SerializableHandle};
use regex::Regex;
use std::collections::HashMap;
use std::{io::BufWriter, process::exit, u16};
use worker::{
    console_debug, console_error, console_warn, event, Env, Fetch, ScheduleContext, ScheduledEvent,
    Url,
};

mod atom_entry;
mod traverse;

const PATCHES_URL: &str = "https://ftp.openbsd.org/pub/OpenBSD/patches/";
const ERRATA_URL: &str = "https://www.openbsd.org/errata";
const MIN_VERSION: u16 = 22;

fn fetch(url: &str) -> Fetch {
    Fetch::Url(match Url::parse(url) {
        Ok(url) => url,
        Err(e) => {
            console_error!("URL Parse Error: {}", e);
            exit(1);
        }
    })
}

fn parse_html(html: &mut String) -> RcDom {
    match parse_document(RcDom::default(), ParseOpts::default())
        .from_utf8()
        .read_from(&mut html.as_bytes())
    {
        Ok(dom) => dom,
        Err(e) => {
            console_error!("HTML Parse Error: {}", e);
            exit(1);
        }
    }
}

fn cerealize(node: Handle) -> String {
    let mut content_buf = BufWriter::new(Vec::new());

    match serialize(
        &mut content_buf,
        &SerializableHandle::from(node),
        Default::default(),
    ) {
        Err(e) => {
            console_error!("Error serializing <li> contents: {}", e);
            exit(1);
        }
        _ => {}
    };
    match String::from_utf8(match content_buf.into_inner() {
        Ok(byte_arr) => byte_arr,
        Err(e) => {
            console_error!("Error serializing <li> contents: {}", e);
            exit(1);
        }
    }) {
        Ok(content) => content,
        Err(e) => {
            console_error!("Error serializing <li> contents: {}", e);
            exit(1);
        }
    }
}

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
            console_debug!("new_content = {:?}", last_entry);
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

fn fill_date_idx(dom: &RcDom, idx: &mut HashMap<String, StrTendril>) {
    match match match &dom.document.first_child_by_name("html") {
        Some(html) => html,
        None => {
            console_warn!("Document Error: Missing <html> tag");
            return;
        }
    }
    .first_child_by_name("body")
    {
        Some(body) => body,
        None => {
            console_warn!("Document Error: Missing <body> tag");
            return;
        }
    }
    .first_child_by_name("pre")
    {
        Some(pre) => pre.index_following_text_by_children_attr(
            "href",
            |mut attr| {
                attr.replace_range(attr.find(".patch").unwrap_or(attr.len()).., "");
                attr
            },
            idx,
        ),
        None => console_warn!("Document Error: Missing <pre> tag. New release?"),
    };
}

fn get_archs(dom: &RcDom) -> Vec<String> {
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
    .first_child_by_name("pre")
    {
        Some(pre) => pre.children_by_name("a"),
        None => {
            console_warn!("Document Error: Missing <pre> tag. New release?");
            Vec::<Handle>::new()
        }
    }
    .iter()
    .filter_map(|a| a.data.first_attr_by_name("href"))
    .filter(|href| href.ends_with("/"))
    .filter(|href| !href.starts_with("."))
    .map(|href| href.to_string())
    .collect()
}

async fn get_html(errata_url: &str) -> Result<String, u16> {
    let mut errata_res = match fetch(errata_url).send().await {
        Ok(res) => res,
        Err(e) => {
            console_error!("Error Fetching URLS: {}", e);
            exit(1);
        }
    };
    let errata_html = match errata_res.text().await {
        Ok(html) => html,
        Err(e) => {
            console_error!("Response Error: {}", e);
            exit(1);
        }
    };
    match errata_res.status_code() {
        200 => Ok(errata_html),
        404 => Err(404),
        _ => {
            console_error!("Server Error: {}", errata_res.status_code());
            exit(2);
        }
    }
}

fn get_updated_date(
    errata_rgx: &Regex,
    content: &str,
    id: &str,
    date_idx: &HashMap<String, StrTendril>,
    last_good_date: &DateTime<FixedOffset>,
) -> DateTime<FixedOffset> {
    match match errata_rgx.find(content) {
        Some(result) => DateTime::parse_from_str(
            &(String::from(result.as_str()) + " 00:00:00+0000"),
            "%B %d, %Y %T%z",
        ),
        None => {
            console_warn!("Date unavailable. Parsing from ftp...");
            let id = &id[id.find("-").map_or(0, |i| i + 1)..];

            let empty = StrTendril::new();
            let item = date_idx.get(id);
            //console_debug!("date_idx.get({}) = {:?}", id, item);
            let mut date_str = item.unwrap_or(&empty).trim_start();
            date_str = &date_str[..date_str.find(" ").unwrap_or(date_str.len())];
            let date_str = date_str.to_string() + " 00:00:00+0000";

            DateTime::parse_from_str(&date_str, "%d-%b-%Y %T%z")
        }
    } {
        Ok(date) => date,
        Err(e) => {
            console_warn!("Date Parse Error: {} ---> Making up a new one", e);
            last_good_date
                .checked_add_signed(Duration::days(1))
                .unwrap_or(DateTime::default())
        }
    }
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
    loop {
        let arch_url = PATCHES_URL.to_owned() + &format!("{:.1}/", version as f32 / 10.);
        let errata_url = ERRATA_URL.to_owned() + &version.to_string() + ".html";

        let (mut errata_html, mut arch_html) =
            match try_join!(get_html(&errata_url), get_html(&arch_url)) {
                Ok((html, arch)) => (html, arch),
                Err(_) => break,
            };

        let arch_dom = parse_html(&mut arch_html);
        let archs = get_archs(&arch_dom);

        let mut date_idx = HashMap::<String, StrTendril>::new();
        for arch in archs {
            let mut arch_html = match get_html(&(arch_url.clone() + &arch + "/")).await {
                Ok(html) => html,
                Err(_) => break,
            };
            let arch_dom = parse_html(&mut arch_html);
            fill_date_idx(&arch_dom, &mut date_idx);
        }
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
                get_updated_date(&errata_regex, &content, &id, &date_idx, &last_good_date);
            last_good_date = updated.clone();
            let atom_entry = AtomEntry {
                id,
                title,
                updated,
                link: errata_url.clone(),
                content,
            };
            console_debug!("{:?}", atom_entry);
            entries.push(atom_entry);
        }

        version += 1;
    }

    //Render Feed

    //Upload to git
}
