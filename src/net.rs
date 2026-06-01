use std::fs;
use std::io::Read;
use std::sync::OnceLock;
use ureq::Agent;
use url::Url;

const MAX_REDIRECTS: u8 = 10;

#[derive(Debug)]
pub enum NetError {
    InvalidUrl(String),
    UnsupportedScheme,
    LocalFile(String),
    Request(String),
    HttpStatus(u16),
    EmptyBody,
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetError::InvalidUrl(e) => write!(f, "invalid URL: {e}"),
            NetError::UnsupportedScheme => {
                write!(f, "unsupported URL scheme (use http, https, or file)")
            }
            NetError::LocalFile(e) => write!(f, "local file error: {e}"),
            NetError::Request(e) => write!(f, "HTTP request failed: {e}"),
            NetError::HttpStatus(c) => write!(f, "HTTP status {c}"),
            NetError::EmptyBody => write!(f, "empty response body"),
        }
    }
}

impl std::error::Error for NetError {}

fn agent() -> &'static Agent {
    static AGENT: OnceLock<Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::builder()
            .redirects(MAX_REDIRECTS.into())
            .user_agent("tro/0.1")
            .build()
    })
}

/// `reddit.com` / `www.reddit.com` serve a bot-check shell; `old.reddit.com` is static HTML.
pub fn resolve_fetch_url(url: &str) -> Result<String, NetError> {
    let parsed = parse_url(url)?;
    if parsed.scheme() == "file" {
        return Ok(url.to_string());
    }
    Ok(rewrite_reddit_host(&parsed))
}

pub fn fetch_body(url: &str) -> Result<String, NetError> {
    let parsed = parse_url(url)?;
    if parsed.scheme() == "file" {
        return fetch_file(&parsed);
    }
    fetch_http(&resolve_fetch_url(url)?)
}

fn rewrite_reddit_host(url: &Url) -> String {
    match url.host_str() {
        Some("www.reddit.com") | Some("reddit.com") => {
            let mut u = url.clone();
            let _ = u.set_host(Some("old.reddit.com"));
            u.to_string()
        }
        _ => url.to_string(),
    }
}

fn fetch_http(url: &str) -> Result<String, NetError> {
    let response = agent()
        .get(url)
        .call()
        .map_err(|e| NetError::Request(e.to_string()))?;

    let status = response.status();
    if !(200..300).contains(&status) {
        return Err(NetError::HttpStatus(status));
    }

    let mut body = Vec::new();
    if let Some(len) = response.header("content-length").and_then(|h| h.parse().ok()) {
        body.reserve(len);
    }
    response
        .into_reader()
        .read_to_end(&mut body)
        .map_err(|e| NetError::Request(e.to_string()))?;

    if body.is_empty() {
        return Err(NetError::EmptyBody);
    }

    Ok(String::from_utf8_lossy(&body).into_owned())
}

fn fetch_file(url: &Url) -> Result<String, NetError> {
    let path = url
        .to_file_path()
        .map_err(|_| NetError::LocalFile("invalid file URL".into()))?;
    let body = fs::read_to_string(&path).map_err(|e| NetError::LocalFile(e.to_string()))?;
    if body.is_empty() {
        return Err(NetError::EmptyBody);
    }
    Ok(body)
}

fn parse_url(url: &str) -> Result<Url, NetError> {
    let parsed = Url::parse(url).map_err(|e| NetError::InvalidUrl(e.to_string()))?;
    match parsed.scheme() {
        "http" | "https" | "file" => Ok(parsed),
        _ => Err(NetError::UnsupportedScheme),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_reddit_to_old() {
        let out = resolve_fetch_url(
            "https://www.reddit.com/r/rust/comments/bb5lnj/test/",
        )
        .unwrap();
        assert_eq!(
            out,
            "https://old.reddit.com/r/rust/comments/bb5lnj/test/"
        );
    }

    #[test]
    fn leaves_non_reddit_unchanged() {
        let u = "https://doc.rust-lang.org/book/";
        assert_eq!(resolve_fetch_url(u).unwrap(), u);
    }
}
