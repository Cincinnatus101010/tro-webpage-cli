mod common;

use common::factory::{FileUrlFactory, HttpFactory, PageFactory, UrlFactory};
use tro::{
    extract_html, extract_html_with_options, extract_url, extract_url_cached, extract_urls,
    extract_urls_cached, ExtractOptions, PageCache,
};

#[test]
fn documentation_page_extracts_main_content() {
    let html = PageFactory::documentation().html;
    let page = extract_html(&html).unwrap();

    assert_eq!(page.title, "API Reference — tro");
    assert!(page.text.contains("extract_url"));
    assert!(page.text.contains("Press F5"));
    assert!(page.text.contains("Rust Programming Language"));
    assert!(!page.text.contains("window.__analytics"));
}

#[test]
fn plain_text_passthrough_without_html() {
    let md = PageFactory::plain_text("# Hello\n\nworld");
    let page = extract_html(&md).unwrap();
    assert_eq!(page.text, md);
    assert!(!page.truncated);
}

#[test]
fn spa_shell_uses_embedded_metadata() {
    let html = PageFactory::spa_shell().html;
    let page = extract_html(&html).unwrap();

    assert_eq!(page.title, "Release notes — v2.0");
    assert!(page.text.contains("Batch reads now run in parallel"));
    assert!(page.text.contains("Enable JavaScript"));
    assert!(!page.text.contains("function bundleInit"));
}

#[test]
fn max_chars_truncates_output() {
    let html = PageFactory::article_in_main("Long", &"word ".repeat(200)).html;
    let page = extract_html_with_options(
        &html,
        &ExtractOptions {
            max_chars: Some(50),
        },
    )
    .unwrap();

    assert!(page.truncated);
    assert!(page.text.ends_with("[truncated]"));
    assert!(page.text.chars().count() < html.len());
}

#[test]
fn max_chars_stops_during_dom_walk() {
    let body = "x".repeat(20_000);
    let html = PageFactory::article_in_main("Huge", &body).html;
    let page = extract_html_with_options(
        &html,
        &ExtractOptions {
            max_chars: Some(100),
        },
    )
    .unwrap();

    assert!(page.truncated);
    assert!(page.text.ends_with("[truncated]"));
    // Body chars capped at 100; marker adds a few more.
    assert!(page.text.chars().count() <= 120);
}

#[test]
fn page_cache_skips_repeat_fetches() {
    let html = PageFactory::article_in_main(
        "Cached",
        "This page is served once then read from cache.",
    )
    .html;
    let server = HttpFactory::serve_html(&html);
    let cache = PageCache::new(8);
    let opts = ExtractOptions::default();

    let first = extract_url_cached(&cache, &server.url, &opts).unwrap();
    let second = extract_url_cached(&cache, &server.url, &opts).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.title, "Cached");
}

#[test]
fn page_cache_batch_reuses_entries() {
    let html = PageFactory::documentation().html;
    let server = HttpFactory::serve_html(&html);
    let cache = PageCache::new(8);
    let opts = ExtractOptions::default();
    let urls = [server.url.as_str()];

    let _ = extract_urls_cached(&cache, &urls, &opts);
    let again = extract_urls_cached(&cache, &urls, &opts);
    assert_eq!(again.len(), 1);
    assert!(again[0].error.is_none());
    assert!(again[0].text.as_ref().unwrap().contains("extract_url"));
}

#[test]
fn extract_url_reads_local_file() {
    let file = PageFactory::documentation().write_file_url().unwrap();
    let page = extract_url(&file.url).unwrap();
    assert!(page.text.contains("extract_url"));
}

#[test]
fn extract_url_fetches_http() {
    let html = PageFactory::article_in_main(
        "HTTP doc",
        "This paragraph is served over HTTP from a mock server for integration testing.",
    )
    .html;
    let server = HttpFactory::serve_html(&html);
    let page = extract_url(&server.url).unwrap();

    assert_eq!(page.title, "HTTP doc");
    assert!(page.text.contains("mock server"));
}

#[test]
fn extract_urls_parallel_file_and_http() {
    let file = PageFactory::documentation().write_file_url().unwrap();
    let spa = PageFactory::spa_shell();
    let http = HttpFactory::serve_html(&spa.html);

    let urls = [file.url.as_str(), http.url.as_str()];
    let pages = extract_urls(&urls, &ExtractOptions::default());

    assert_eq!(pages.len(), 2);
    assert!(pages.iter().all(|p| p.error.is_none()));
    assert!(pages[0].text.as_ref().unwrap().contains("extract_url"));
    assert!(pages[1].title.as_deref().unwrap().contains("Release notes"));
}

#[test]
fn resolve_fetch_url_rewrites_reddit() {
    let resolved = tro::net::resolve_fetch_url(UrlFactory::reddit_thread()).unwrap();
    assert_eq!(
        resolved,
        "https://old.reddit.com/r/rust/comments/bb5lnj/example/"
    );
}

#[test]
fn resolve_fetch_url_leaves_other_hosts() {
    let u = UrlFactory::doc_rust_lang();
    assert_eq!(tro::net::resolve_fetch_url(u).unwrap(), u);
}

#[test]
fn fetch_body_rejects_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.html");
    std::fs::write(&path, "").unwrap();
    let url = url::Url::from_file_path(&path).unwrap().to_string();
    let err = tro::net::fetch_body(&url).unwrap_err();
    assert!(matches!(err, tro::net::NetError::EmptyBody));
}

#[test]
fn file_url_factory_roundtrip() {
    let html = "<html><body><p>factory</p></body></html>";
    let file = FileUrlFactory::from_html(html).unwrap();
    let body = tro::net::fetch_body(&file.url).unwrap();
    assert!(body.contains("factory"));
}
