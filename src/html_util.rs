use html5ever::tendril::TendrilSink;
use html5ever::{parse_document, serialize, ParseOpts};
use markup5ever_rcdom::{Handle, RcDom, SerializableHandle};
use std::io::BufWriter;
use std::process::exit;
use worker::{console_error, Fetch, Url};

pub fn fetch(url: &str) -> Fetch {
    Fetch::Url(match Url::parse(url) {
        Ok(url) => url,
        Err(e) => {
            console_error!("URL Parse Error: {}", e);
            exit(1);
        }
    })
}

pub fn parse_html(html: &mut String) -> RcDom {
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

pub fn cerealize(node: Handle) -> String {
    let mut content_buf = BufWriter::new(Vec::new());

    match serialize(
        &mut content_buf,
        &SerializableHandle::from(node),
        Default::default(),
    ) {
        Err(e) => {
            console_error!("Error serializing contents: {}", e);
            exit(1);
        }
        _ => {}
    };
    match String::from_utf8(match content_buf.into_inner() {
        Ok(byte_arr) => byte_arr,
        Err(e) => {
            console_error!("Error serializing contents: {}", e);
            exit(1);
        }
    }) {
        Ok(content) => content,
        Err(e) => {
            console_error!("Error serializing contents: {}", e);
            exit(1);
        }
    }
}

pub async fn get_html(errata_url: &str) -> Result<String, u16> {
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
