extern crate core;

use crate::atom_entry::AtomEntry;
use crate::traverse::{Traverse, TraverseAttrs};
use html5ever::{namespace_url, ns, parse_document, tendril::TendrilSink, ParseOpts};
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use std::iter::repeat;
use std::{process::exit, u16};
use worker::{
    console_debug, console_error, console_warn, event, Env, Fetch, ScheduleContext,
    ScheduledEvent, Url,
};

mod atom_entry;
mod traverse;
mod utils;

//const PATCHES_URL: &str = "https://ftp.openbsd.org/pub/OpenBSD/patches/";
const ERRATA_URL: &str = "https://www.openbsd.org/errata";
const MIN_VERSION: u16 = 22;

fn fetch(url: &str) -> Fetch {
    Fetch::Url(Url::parse(url).unwrap_or_else(|e| {
        console_error!("URL Parse Error: {}", e);
        exit(1);
    }))
}

fn parse_html(html: &mut String) -> RcDom {
    parse_document(RcDom::default(), ParseOpts::default())
        .from_utf8()
        .read_from(&mut html.as_bytes())
        .unwrap_or_else(|e| {
            console_error!("HTML Parse Error: {}", e);
            exit(1);
        })
}

fn walk(indent: usize, handle: &Handle) {
    let node = handle;

    let spaces = format!("{}", repeat(" ").take(indent).collect::<String>());
    match node.data {
        NodeData::Document => console_debug!("{}#Document", spaces),

        NodeData::Doctype {
            ref name,
            ref public_id,
            ref system_id,
        } => console_debug!(
            "{}<!DOCTYPE {} \"{}\" \"{}\">",
            spaces,
            name,
            public_id,
            system_id
        ),

        NodeData::Text { ref contents } => {
            console_debug!("{}#text: {}", spaces, contents.borrow().escape_default())
        }

        NodeData::Comment { ref contents } => {
            console_debug!("{}<!-- {} -->", spaces, contents.escape_default())
        }

        NodeData::Element {
            ref name,
            ref attrs,
            ..
        } => {
            assert!(name.ns == ns!(html));
            let mut name = format!("{}<{}", spaces, name.local);
            for attr in attrs.borrow().iter() {
                assert!(attr.name.ns == ns!());
                name.push_str(&format!(" {}=\"{}\"", attr.name.local, attr.value));
            }
            console_debug!("{}>", name);
        }

        NodeData::ProcessingInstruction { .. } => unreachable!(),
    }

    for child in node.children.borrow().iter() {
        walk(indent + 4, child);
    }
}

#[event(scheduled)]
pub async fn scheduled(_: ScheduledEvent, _: Env, _: ScheduleContext) {
    utils::set_panic_hook();

    //let patch_url = PATCHES_URL.to_owned() + &*format!("{:.1}/common/", MIN_VERSION as f32 / 10.);

    let mut entries = Vec::<AtomEntry>::new();

    let mut version: u16 = MIN_VERSION;
    loop {
        let errata_url = ERRATA_URL.to_owned() + &version.to_string() + ".html";
        let errata_req = fetch(&errata_url);
        let mut errata_res = errata_req.send().await.unwrap_or_else(|e| {
            console_error!("Error Fetching URLS: {}", e);
            exit(1);
        });
        let mut errata_html = errata_res.text().await.unwrap_or_else(|e| {
            console_error!("Response Error: {}", e);
            exit(1);
        });

        if errata_res.status_code() != 200 && errata_res.status_code() != 404 {
            console_error!("Server Error: {}", errata_res.status_code());
            exit(2);
        } else if errata_res.status_code() == 404 {
            break;
        }

        let errata_dom = parse_html(&mut errata_html);
        let errata_doc = &errata_dom.document;

        //walk(0, errata_doc);

        let body = errata_doc
            .first_child_by_name("html")
            .unwrap_or_else(|| {
                console_error!("Document Error: Missing <html> tag");
                exit(1);
            })
            .first_child_by_name("body")
            .unwrap_or_else(|| {
                console_error!("Document Error: Missing <body> tag");
                exit(1);
            });
        let mut patches = Vec::<Handle>::new();
        if let Some(ul) = body.first_child_by_name("ul") {
            patches = ul.children_by_name("li", patches);
        } else {
            console_warn!("Document Error: Missing <ul> tag. New release?");
        }

        for (i, patch) in patches.iter().enumerate() {
            let id: String;
            if let Some(id_attr) = patch.data.first_attr_by_name("id") {
                id = format!("v{}-{}", version, id_attr /*String::from(id_attr)*/);
            } else {
                id = format!("v{}-p{:03}???", version, i + 1);
                //For some reason the ID is missing
            }

            let title;
            if let Some(strong) = patch.first_child_by_name("strong") {
                title = strong.get_first_text();
            } else {
                console_warn!("Document Error: Missing <strong> tag in <li>"); //append content to previous entry
                continue;
            }

            let atomentry = AtomEntry {
                id,
                title,
                updated: Default::default(),
                link: errata_url.clone(),
                content: "".to_string(),
            };
            console_debug!("{:?}", atomentry);
            entries.push(atomentry);
        }
        version += 1;
    }
}
