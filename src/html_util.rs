use html5ever::tendril::TendrilSink;
use html5ever::{parse_document, serialize, ParseOpts};
use markup5ever_rcdom::{Handle, RcDom, SerializableHandle};
use std::io::BufWriter;
use std::process::exit;
use sha::sha1::Sha1;
use sha::utils::{Digest, DigestExt};
use worker::{CfProperties, console_error, Fetch, Headers, Method, Request, RequestInit, RequestRedirect, Secret, Url};
use worker::wasm_bindgen::JsValue;
use crate::{FEED_AUTHOR_NAME, GITHUB_API_VERSION, GITHUB_COMMIT_EMAIL, GITHUB_COMMIT_MESSAGE};

pub fn fetch_req(url: &str) -> Fetch {
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

pub async fn http_get(url: &str) -> Result<(String, Option<String>), u16> {
    let mut res = match fetch_req(url).send().await {
        Ok(res) => res,
        Err(e) => {
            console_error!("Error Fetching URLS: {}", e);
            exit(1);
        }
    };

    let ret = match res.bytes().await {
        Ok(bytes) => {
            let sha = Sha1::default().digest(&bytes).to_hex();
            match String::from_utf8(bytes) {
                Ok(text) if text.len() > 0 => (text, Some(sha)),
                Ok(text) => (text, None),
                Err(e) => {
                    console_error!("Error parsing response: {}", e);
                    exit(1);
                }
            }
        },
        Err(e) => {
            console_error!("Response Error: {}", e);
            exit(1);
        }
    };
    match res.status_code() {
        200 => Ok(ret),
        404 => Err(404),
        _ => {
            console_error!("Server Error: {}", res.status_code());
            exit(2);
        }
    }
}

pub async fn github_commit(url: &str, token: Secret, content: &str, sha: Option<String>) -> Result<(), u16> {

    let token = match JsValue::from(token).as_string() {
        Some(secret) => secret,
        None => {
            console_error!("Error Accessing GitHub token");
            exit(1);
        }
    };
    let mut headers = Headers::new();
    let _ = headers.set("Accept", "application/vnd.github+json");
    let _ = headers.set("Authorization", &(String::from("Bearer ") + &token));
    let _ = headers.set("X-GitHub-Api-Version", GITHUB_API_VERSION);

    let sha_str = match sha {
        Some(sha) => String::from(",\"sha\":\"") + &sha + "\"",
        None => String::new(),
    };

    let body = JsValue::from_str(&*format!(concat!("{{",
        "\"message\":\"{message}\",",
        "\"committer\":{{",
        "\"name\":\"{name}\",",
        "\"email\":\"{email}\"",
        "}},\"content\":\"{content}\"",
        "{sha}",
        "}}"),
       message = GITHUB_COMMIT_MESSAGE,
       name = FEED_AUTHOR_NAME,
       email = GITHUB_COMMIT_EMAIL,
       content = content,
       sha = sha_str,
    ));

    let req = match Request::new_with_init(
        url,
        &RequestInit {
                body: Some(body),
                headers,
                cf: CfProperties::default(),
                method: Method::Put,
                redirect: RequestRedirect::Follow
        }
    ) {
        Ok(req) => req,
        Err(e) => {
            console_error!("Error generating request: {}", e);
            exit(1);
        }
    };
    let res = match Fetch::Request(req).send().await {
        Ok(res) => res,
        Err(e) => {
            console_error!("Error Fetching URLS: {}", e);
            exit(1);
        }
    };
    match res.status_code() {
        200..=201 => Ok(()),
        _ => {
            console_error!("Server Error: {}", res.status_code());
            exit(2);
        }
    }
}
