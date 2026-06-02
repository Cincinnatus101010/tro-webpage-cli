mod dom;
pub mod net;
mod page_cache;

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
pub use page_cache::PageCache;

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
        .map(|url| url_page_from_result(*url, extract_url_with_options(url, opts)))
        .collect()
}

pub fn extract_urls_cached<'a>(
    cache: &page_cache::PageCache,
    urls: &'a [&'a str],
    opts: &ExtractOptions,
) -> Vec<UrlPage> {
    let mut pages: Vec<Option<UrlPage>> = vec![None; urls.len()];
    let mut missing: Vec<(usize, &'a str)> = Vec::new();

    for (i, url) in urls.iter().enumerate() {
        if let Some(page) = cache.get(url, opts.max_chars) {
            pages[i] = Some(url_page_from_readable(url, page));
        } else {
            missing.push((i, *url));
        }
    }

    if !missing.is_empty() {
        let refs: Vec<&str> = missing.iter().map(|(_, url)| *url).collect();
        let fetched = extract_urls(&refs, opts);
        for ((i, url), page) in missing.into_iter().zip(fetched) {
            if page.error.is_none() {
                if let (Some(title), Some(text)) = (&page.title, &page.text) {
                    cache.insert(
                        url,
                        opts.max_chars,
                        ReadablePage {
                            title: title.clone(),
                            text: text.clone(),
                            truncated: page.truncated,
                        },
                    );
                }
            }
            pages[i] = Some(page);
        }
    }

    pages.into_iter().map(|p| p.expect("every slot filled")).collect()
}

pub fn extract_url_cached(
    cache: &page_cache::PageCache,
    url: &str,
    opts: &ExtractOptions,
) -> Result<ReadablePage, Error> {
    if let Some(page) = cache.get(url, opts.max_chars) {
        return Ok(page);
    }
    let page = extract_url_with_options(url, opts)?;
    cache.insert(url, opts.max_chars, page.clone());
    Ok(page)
}

fn url_page_from_readable(url: &str, page: ReadablePage) -> UrlPage {
    UrlPage {
        url: url.to_string(),
        title: Some(page.title),
        text: Some(page.text),
        error: None,
        truncated: page.truncated,
    }
}

fn url_page_from_result(url: &str, result: Result<ReadablePage, Error>) -> UrlPage {
    match result {
        Ok(page) => url_page_from_readable(url, page),
        Err(e) => UrlPage {
            url: url.to_string(),
            title: None,
            text: None,
            error: Some(e.to_string()),
            truncated: false,
        },
    }
}
