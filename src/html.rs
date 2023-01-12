use crate::{
    FEED_AUTHOR_NAME, GITHUB_API_VERSION, GITHUB_COMMIT_EMAIL, GITHUB_COMMIT_MESSAGE,
    GITHUB_REPO_OWNER,
};
use bytes::BufMut;
use html5ever::{
    parse_document, serialize,
    tendril::{fmt::Slice, TendrilSink},
    ParseOpts,
};
use markup5ever_rcdom::{Handle, RcDom, SerializableHandle};
use reqwest::{
    header,
    header::{HeaderMap, HeaderName, HeaderValue},
    StatusCode,
};
use serde_json::json;
use sha::{
    sha1::Sha1,
    utils::{Digest, DigestExt},
};
use std::{io::BufWriter, process::exit};

pub fn parse_html(html: &mut String) -> RcDom {
    match parse_document(RcDom::default(), ParseOpts::default())
        .from_utf8()
        .read_from(&mut html.as_bytes())
    {
        Ok(dom) => dom,
        Err(e) => {
            eprintln!("HTML Parse Error: {}", e);
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
            eprintln!("Error serializing contents: {}", e);
            exit(1);
        }
        _ => {}
    };
    match String::from_utf8(match content_buf.into_inner() {
        Ok(byte_arr) => byte_arr,
        Err(e) => {
            eprintln!("Error serializing contents: {}", e);
            exit(1);
        }
    }) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Error serializing contents: {}", e);
            exit(1);
        }
    }
}

pub(crate) fn calc_git_sha1(bytes: &[u8]) -> String {
    let mut blob = Vec::<u8>::new();
    blob.put_slice(format!("blob {}", bytes.len()).as_bytes());
    blob.put_u8(0);
    blob.put_slice(&bytes);
    Sha1::default().digest(blob.as_bytes()).to_hex()
}

pub async fn http_get(url: &str, git_sha: bool) -> Result<(String, Option<String>), u16> {
    println!("Fetching url: {}", url);
    let res = match reqwest::get(url).await {
        Ok(res) => res,
        Err(e) => {
            eprintln!("Error Fetching URL: {}", e);
            exit(1);
        }
    };
    let status = res.status().clone();
    let ret = match res.bytes().await {
        Ok(bytes) => {
            match String::from_utf8(bytes.to_vec()) {
                Ok(text) if git_sha && status == StatusCode::OK => {
                    (text, Some(calc_git_sha1(&bytes)))
                } //File exists, replace
                Ok(text) => (text, None), //File doesn't exist or error
                Err(e) => {
                    eprintln!("Error parsing response: {}", e);
                    exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("Response Error: {}", e);
            exit(1);
        }
    };
    match status {
        StatusCode::OK => Ok(ret),
        StatusCode::NOT_FOUND => Err(404),
        _ => {
            eprintln!("Server Error: {} URL: {}", status, &url);
            exit(2);
        }
    }
}

pub async fn github_commit(url: &str, token: &str, content: &str, sha: &str) -> Result<(), u16> {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ACCEPT,
        match HeaderValue::from_str("application/vnd.github+json") {
            Ok(value) => value,
            Err(_) => {
                eprintln!("Error with Accept header on github_commit()");
                exit(1);
            }
        },
    );
    headers.insert(
        HeaderName::from_static("x-github-api-version"),
        match HeaderValue::from_str(GITHUB_API_VERSION) {
            Ok(value) => value,
            Err(_) => {
                eprintln!("Error with X-GitHub-Api-Version header on github_commit()");
                exit(1);
            }
        },
    );
    headers.insert(
        header::USER_AGENT,
        match HeaderValue::from_str(GITHUB_REPO_OWNER) {
            Ok(value) => value,
            Err(_) => {
                eprintln!("Error with User-Agent header on github_commit()");
                exit(1);
            }
        },
    );
    let req_body = json!({
        "message": GITHUB_COMMIT_MESSAGE,
        "committer": {
            "name": FEED_AUTHOR_NAME,
            "email": GITHUB_COMMIT_EMAIL
        },
        "content": content,
        "sha": sha
    })
    .to_string();

    let res = match reqwest::Client::new()
        .put(url)
        .headers(headers)
        .bearer_auth(token)
        .body(req_body.clone())
        .send()
        .await
    {
        Ok(res) => res,
        Err(e) => {
            eprintln!("Error Fetching URLS: {}", e);
            exit(1);
        }
    };
    let status = res.status().as_u16();
    let txt = match res.text().await {
        Ok(txt) => txt,
        Err(e) => {
            eprintln!("Error receiving response: {}", e);
            exit(1);
        }
    };

    match status {
        200 | 201 => Ok(()),
        code => {
            eprintln!("GitHub Server Error: {}, Response Body: {:?}", code, txt);
            exit(2);
        }
    }
}
