use html5ever::tendril::StrTendril;
use markup5ever_rcdom::{Handle, RcDom};
use std::collections::HashMap;
use std::process::exit;

use crate::html::{http_get, parse_html};
use crate::traverse_dom::{TraverseAttrs, TraverseDom};
use crate::PATCHES_URL;

#[derive(Debug)]
pub struct DateIndex {
    idx: Option<HashMap<String, StrTendril>>,
    version: u16,
}

impl DateIndex {
    pub fn new(version: u16) -> DateIndex {
        DateIndex { idx: None, version }
    }

    fn fill_date_idx(&mut self, dom: &RcDom) {
        if let Some(ref mut idx) = self.idx {
            match match match &dom.document.first_child_by_name("html") {
                Some(html) => html,
                None => {
                    eprintln!("Document Error: Missing <html> tag");
                    return;
                }
            }
            .first_child_by_name("body")
            {
                Some(body) => body,
                None => {
                    eprintln!("Document Error: Missing <body> tag");
                    return;
                }
            }
            .first_child_by_name("pre")
            {
                Some(pre) => pre.index_following_text_by_children_attr(
                    "href",
                    |mut attr| {
                        attr.replace_range(attr.find(".patch").unwrap_or(attr.len()).., "");
                        println!("Replaced ftp date string: {}", attr);
                        attr
                    },
                    idx,
                ),
                None => eprintln!("Document Error: Missing <pre> tag. New release?"),
            };
        }
    }

    fn get_archs(dom: &RcDom) -> Vec<String> {
        match match match &dom.document.first_child_by_name("html") {
            Some(html) => html,
            None => {
                eprintln!("Document Error: Missing <html> tag");
                exit(1);
            }
        }
        .first_child_by_name("body")
        {
            Some(body) => body,
            None => {
                eprintln!("Document Error: Missing <body> tag");
                exit(1);
            }
        }
        .first_child_by_name("pre")
        {
            Some(pre) => pre.children_by_name("a"),
            None => {
                eprintln!("Document Error: Missing <pre> tag. New release?");
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

    pub async fn lazy_load(&mut self, version: u16) -> &mut Option<HashMap<String, StrTendril>> {
        let mut load = false;
        match self.idx {
            None => {
                self.idx = Some(HashMap::<String, StrTendril>::new());
                load = true;
                self.version = version
            }
            Some(ref mut idx) if self.version != version => {
                idx.clear();
                load = true;
                self.version = version
            }
            _ => {}
        };
        if load {
            let arch_url = PATCHES_URL.to_owned() + &format!("{:.1}/", version as f32 / 10.);

            let mut arch_html = match http_get(&arch_url, false).await {
                Ok((html, _)) => html,
                Err(_) => return &mut self.idx,
            };
            let arch_dom = parse_html(&mut arch_html);
            let archs = Self::get_archs(&arch_dom);

            for arch in archs {
                println!("Arch: {}", arch.as_str());
                let mut arch_html = match http_get(&(arch_url.clone() + &arch), false).await {
                    Ok((html, _)) => html,
                    Err(_) => break,
                };
                let arch_dom = parse_html(&mut arch_html);
                self.fill_date_idx(&arch_dom);
            }
        }
        &mut self.idx
    }
}
