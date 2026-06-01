mod dom;
pub mod net;

use net::NetError;
use rayon::prelude::*;
use serde::Serialize;

#[derive(Debug)]
pub enum Error {
    Net(NetError),
    Dom(dom::DomError),
}

impl From<NetError> for Error {
    fn from(e: NetError) -> Self {
        Error::Net(e)
    }
}

impl From<dom::DomError> for Error {
    fn from(e: dom::DomError) -> Self {
        Error::Dom(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Net(e) => e.fmt(f),
            Error::Dom(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {}

pub use dom::{
    extract_html, extract_html_with_options, DomError, ExtractOptions, ReadablePage,
};

#[derive(Debug, Clone, Serialize)]
pub struct UrlPage {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub truncated: bool,
}

pub fn extract_url(url: &str) -> Result<ReadablePage, Error> {
    extract_url_with_options(url, &ExtractOptions::default())
}

pub fn extract_url_with_options(url: &str, opts: &ExtractOptions) -> Result<ReadablePage, Error> {
    let html = net::fetch_body(url)?;
    Ok(dom::extract_html_with_options(&html, opts)?)
}

pub fn extract_urls<'a>(urls: &'a [&'a str], opts: &ExtractOptions) -> Vec<UrlPage> {
    urls.par_iter()
        .map(|url| match extract_url_with_options(url, opts) {
            Ok(page) => UrlPage {
                url: (*url).to_string(),
                title: Some(page.title),
                text: Some(page.text),
                error: None,
                truncated: page.truncated,
            },
            Err(e) => UrlPage {
                url: (*url).to_string(),
                title: None,
                text: None,
                error: Some(e.to_string()),
                truncated: false,
            },
        })
        .collect()
}
