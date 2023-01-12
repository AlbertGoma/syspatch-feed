extern crate core;

use crate::atom_entry::AtomEntry;
use crate::date_index::DateIndex;
use crate::html::{calc_git_sha1, cerealize, github_commit, http_get, parse_html};
use crate::traverse_dom::{TraverseAttrs, TraverseDom};

use base64::Engine;
use chrono::{DateTime, Duration, FixedOffset, Utc};
use html5ever::tendril::StrTendril;
use markup5ever_rcdom::{Handle, RcDom};
use regex::Regex;
use std::{fs, os::unix::fs::PermissionsExt, process::exit, str::FromStr, u16};

mod atom_entry;
mod date_index;
mod html;
mod traverse_dom;

const ERRATA_URL: &str = "https://www.openbsd.org/errata";
const PATCHES_URL: &str = "https://ftp.openbsd.org/pub/OpenBSD/patches/";
const HOME_PAGE_URL: &str = "https://www.openbsd.org/index.html";

const ISO_UTC_FORMAT: &str = "%Y-%m-%dT%H:%M:%SZ";

const GITHUB_RAW_URL: &str =
    "https://github.com/AlbertGoma/syspatch-feed.albert.goma.cat/raw/main/pub/atom.xml";
const GITHUB_COMMIT_URL: &str =
    "https://api.github.com/repos/AlbertGoma/syspatch-feed.albert.goma.cat/contents/pub/atom.xml";
const GITHUB_REPO_OWNER: &str = "AlbertGoma";
const GITHUB_COMMIT_MESSAGE: &str = "Automated atom feed update";
const GITHUB_COMMIT_EMAIL: &str = "58812649+AlbertGoma@users.noreply.github.com";
const GITHUB_API_VERSION: &str = "2022-11-28";
const GITHUB_TOKEN_PATH: &str = "/etc/syspatch-feed-token";

const FEED_TITLE: &str = "OpenBSD Patches";
const FEED_LINK: &str = "https://syspatch.albert.goma.cat/atom.xml";
const FEED_LINK_REL: &str = "https://www.openbsd.org";
const FEED_AUTHOR_NAME: &str = "Albert Gomà i León";
const FEED_AUTHOR_URI: &str = "https://albert.goma.cat";
const FEED_UUID: &str = "tag:albert.goma.cat,2023:feed/openbsd/sypatch";

fn get_title(
    patch: &Handle,
    entries: &mut Vec<AtomEntry>,
    version: u16,
    date_regex: &Regex,
) -> Option<String> {
    match patch.first_child_by_name("strong") {
        Some(strong) => {
            let mut text = strong.get_first_text();
            let date_offset = date_regex.find(&text).map_or(text.len(), |m| {
                if text[m.start() - 2..m.start()].eq(": ") {
                    m.start() - 2
                } else {
                    m.start()
                }
            });
            //Strip the date from the title
            text.truncate(date_offset);
            Some(format!("OpenBSD {:.1}, {}", version as f32 / 10., text))
        }
        None => {
            match entries
                .last_mut()
                .filter(|atom_entry| atom_entry.id.starts_with(&format!("v{}-", version)))
            {
                Some(last_entry) => last_entry,
                None => {
                    eprintln!("Document Error: <li> without <strong> is first element");
                    exit(1);
                }
            }
            .content += &cerealize(patch.clone());
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
    .first_child_by_name("ul")
    {
        Some(ul) => ul.children_by_name("li"),
        None => {
            eprintln!("Document Error: Missing <ul> tag. New release?");
            Vec::<Handle>::new()
        }
    }
}

fn make_up_date(last_good_date: &DateTime<FixedOffset>) -> DateTime<FixedOffset> {
    last_good_date
        .checked_add_signed(Duration::days(1))
        .unwrap_or(DateTime::default())
}

async fn get_updated_date(
    date_regex: &Regex,
    content: &str,
    id: &str,
    date_idx: &mut DateIndex,
    last_good_date: &DateTime<FixedOffset>,
    version: u16,
    iteration: usize,
) -> DateTime<FixedOffset> {
    match match date_regex.find(content) {
        Some(result) => DateTime::parse_from_str(
            &(String::from(result.as_str()) + " 00:00:00+0000"),
            "%B %d, %Y %T%z",
        ),
        None => {
            if let Some(ref idx) = date_idx.lazy_load(version /*, request_ctr*/).await {
                eprintln!("Date unavailable. Parsing from ftp...");
                let id = &id[id.find("-").map_or(0, |i| i + 1)..];

                let empty = StrTendril::new();
                let item = idx.get(id);
                println!("date_idx.get({}) = {:?}", id, item);
                let mut date_str = item.unwrap_or(&empty).trim_start();
                date_str = &date_str[..date_str.find(" ").unwrap_or(date_str.len())];
                let date_str = date_str.to_string() + " 00:00:00+0000";

                DateTime::parse_from_str(&date_str, "%d-%b-%Y %T%z")
            } else {
                Ok(make_up_date(last_good_date))
            }
        }
    } {
        Ok(date) => date
            .checked_add_signed(Duration::seconds(iteration as i64))
            .unwrap_or(date),
        Err(e) => {
            eprintln!("Date Parse Error: {} ---> Making up a new one", e);
            make_up_date(last_good_date)
        }
    }
}

fn render_feed(old_feed: &str, entries: &mut Vec<AtomEntry>, sha: &str) -> Option<String> {
    let header_ending = old_feed
        .match_indices("\n")
        .nth(10)
        .map_or(old_feed.len(), |(h, _)| h + 1);
    let old_header = &old_feed[..header_ending];
    let new_header = format!(
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
    let mut new_feed = String::new();
    for entry in entries {
        new_feed += &format!(
            concat!(
                "   <entry>\n",
                "       <id>{id_prefix}/{id}</id>\n",
                "       <title type=\"html\">{title}</title>\n",
                "       <updated>{updated}</updated>\n",
                "       <content type=\"html\">{content}</content>\n",
                "       <link rel=\"alternate\" type=\"text/html\" href=\"{link}\"/>\n",
                "   </entry>\n"
            ),
            id_prefix = "tag:albert.goma.cat,2023:syspatch_feed",
            id = entry.id,
            title = html_escape::encode_safe(&entry.title),
            updated = entry.updated.format(ISO_UTC_FORMAT),
            content = html_escape::encode_safe(&entry.content),
            link = entry.link
        );
    }
    new_feed += "</feed>";

    if calc_git_sha1((old_header.to_owned() + &new_feed).as_bytes()) != sha {
        Some(new_header + &new_feed)
    } else {
        None
    }
}

async fn get_latest_version() -> u16 {
    let mut front_page_html = match http_get(HOME_PAGE_URL, false).await {
        Ok((html, _)) => html,
        Err(e) => {
            eprintln!("Error fetching OpenBSD's website front page: {}", e);
            exit(1);
        }
    };
    let front_page_dom = parse_html(&mut front_page_html);

    match match match match match match match match &front_page_dom
        .document
        .first_child_by_name("html")
    {
        Some(html) => html,
        None => {
            eprintln!("Document Error: Missing <html> tag");
            exit(1);
        }
    }
    .first_child_by_name("body")
    {
        Some(main) => main,
        None => {
            eprintln!("Document Error: Missing <body> tag");
            exit(1);
        }
    }
    .first_child_by_name("main")
    {
        Some(main) => main,
        None => {
            eprintln!("Document Error: Missing <main> tag");
            exit(1);
        }
    }
    .first_child_by_name("article")
    {
        Some(article) => article,
        None => {
            eprintln!("Document Error: Missing <article> tag");
            exit(1);
        }
    }
    .first_child_by_name("h2")
    {
        Some(h2) => h2,
        None => {
            eprintln!("Document Error: Missing <h2> tag");
            exit(1);
        }
    }
    .first_child_by_name("a")
    {
        Some(a) => a.data.first_attr_by_name("href"),
        None => {
            eprintln!("Document Error: Missing <a> tag");
            exit(1);
        }
    } {
        Some(href) => u16::from_str(&href[..href.len() - 5]),
        None => {
            eprintln!("Document Error: Missing href attribute in <a> tag");
            exit(1);
        }
    } {
        Ok(version) => version,
        Err(e) => {
            eprintln!("Error parsing version number: {}", e);
            exit(1);
        }
    }
}

#[tokio::main]
async fn main() {
    //Read GitHub secret from file
    let (config_file_attrs, file_type) = match fs::metadata(GITHUB_TOKEN_PATH) {
        Ok(meta) => (meta.permissions().mode(), meta.file_type()),
        Err(e) => {
            eprintln!(
                "Error opening GitHub authentication token file in \"{}\": {:}",
                GITHUB_TOKEN_PATH, e
            );
            exit(1);
        }
    };

    if !file_type.is_file() {
        eprintln!("Error: {} should be a file", GITHUB_TOKEN_PATH);
        exit(1);
    }
    //Regular file permissions bitmask:
    //0b_0100_ugs_rwx_rwx_rwx   //S_IFREG can have other values in non-strictly POSIX systems
    //0b_xxxx_xxx_1xx_xxx_000   (x = don't care)
    match config_file_attrs as u16 ^ 0b_0100_000_100_000_000u16 {
        res if res << 13 != 0 || (res << 7) >> 15 != 0 => {
            eprintln!(
                "Permissions Error: Only the owner and group should be able to access {}",
                GITHUB_TOKEN_PATH
            );
            exit(1);
        }
        _ => (),
    };

    let token = match fs::read_to_string(GITHUB_TOKEN_PATH) {
        Ok(secret) => secret,
        Err(e) => {
            eprintln!(
                "Error reading GitHub authentication token in {}: {:}",
                GITHUB_TOKEN_PATH, e
            );
            exit(1);
        }
    };

    //Download latest feed from GitHub
    let (old_feed, sha) = match http_get(GITHUB_RAW_URL, true).await {
        Ok((feed, Some(sha))) => (feed, sha),
        Ok((_, None)) => {
            eprintln!("Create an empty /pub/atom.xml file manually on the repository");
            exit(1);
        }
        Err(e) => {
            eprintln!("Cannot retrieve previous feed file: {}", e);
            exit(1);
        }
    };

    let mut entries = Vec::<AtomEntry>::new();
    let date_regex = match Regex::new(concat!(
        //We can't generate it at compile time :(
        "(Jan|January|Feb|February|Mar",
        "|March|Apr|April|May|Jun|June",
        "|Jul|July|Aug|August|Sep|September",
        "|Oct|October|Nov|November|Dec|December)",
        "\\s\\d{1,2},\\s\\d{4}"
    )) {
        Ok(regex) => regex,
        Err(e) => {
            eprintln!("Wrong regex: {}", e);
            exit(1);
        }
    };

    //Parse latest version
    let latest_version = get_latest_version().await;
    let min_version = latest_version - 2;
    let mut date_idx = DateIndex::new(min_version);

    //Fetch until current release or exit if 404
    for version in min_version..=latest_version {
        let errata_url = ERRATA_URL.to_owned() + &version.to_string() + ".html";
        let mut errata_html = match http_get(&errata_url, false).await {
            Ok((html, _)) => html,
            Err(_) => break,
        };

        //Parse the contents into a data structure
        let errata_dom = parse_html(&mut errata_html);
        let patches = get_patches(&errata_dom);
        let mut last_good_date = DateTime::<FixedOffset>::default();

        for (i, patch) in patches.iter().enumerate() {
            // The contents of entries without a title belong to the previous one. Handled
            // inside get_title()
            let title = match get_title(patch, &mut entries, version, &date_regex) {
                Some(title) => title,
                None => continue,
            };

            let content = cerealize(patch.clone());
            let id = get_id(patch, version, i);
            let updated = get_updated_date(
                &date_regex,
                &content,
                &id,
                &mut date_idx,
                &last_good_date,
                version,
                i,
            )
            .await;
            last_good_date = updated.clone();
            let link = errata_url.clone() + "#" + &id[id.find("-").map_or(0, |i| i + 1)..];
            let atom_entry = AtomEntry {
                id,
                title,
                updated,
                link,
                content,
                release_version: version,
                iteration_count: i,
            };
            entries.push(atom_entry);
        }
    }
    entries.sort_by(AtomEntry::cmp_entries);

    //Render the feed and checksum for changes
    if let Some(feed) = render_feed(&old_feed, &mut entries, &sha) {
        //Upload it back to GitHub
        match github_commit(
            GITHUB_COMMIT_URL,
            token.trim(),
            &base64::engine::general_purpose::STANDARD.encode(feed),
            &sha,
        )
        .await
        {
            Ok(_) => (),
            Err(e) => eprintln!("Error committing to GitHub. Error code: {}", e),
        };
    }
}
