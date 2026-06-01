//! Test data factory — builds HTML, file URLs, and HTTP stubs without checked-in fixture files.

use httpmock::prelude::*;
use std::io::Write;
use tempfile::TempDir;

/// Built HTML document for extract tests.
#[derive(Debug, Clone)]
pub struct HtmlDocument {
    pub html: String,
}

/// `file://` URL backed by a temp file (kept alive via guard).
pub struct TempFileUrl {
    _dir: TempDir,
    pub url: String,
}

/// Local HTTP mock returning HTML (`server` must stay alive for the mock).
pub struct HttpHtmlServer {
    #[allow(dead_code)]
    pub server: MockServer,
    pub url: String,
}

pub struct PageFactory;

impl PageFactory {
    /// SSR doc page: title, main content, nav stripped, link text preserved.
    pub fn documentation() -> HtmlDocument {
        HtmlDocument {
            html: r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>API Reference — tro</title>
  <style>body { margin: 0; }</style>
  <script>window.__analytics = true;</script>
</head>
<body>
  <nav><a href="/">Home</a></nav>
  <main>
    <h1>extract_url</h1>
    <p>Fetches a URL and returns readable plain text suitable for LLM context.</p>
    <p>Press <strong>F5</strong> to reload the page in a browser.</p>
    <p>Press <strong>Esc</strong> to close the help overlay.</p>
    <p>See also <a href="https://doc.rust-lang.org/book/">The Rust Programming Language</a>.</p>
  </main>
  <footer>Copyright example</footer>
</body>
</html>"#
            .to_string(),
        }
    }

    /// Thin client shell with meta, JSON-LD, __NEXT_DATA__, and noscript fallback.
    pub fn spa_shell() -> HtmlDocument {
        HtmlDocument {
            html: r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>Loading…</title>
  <meta property="og:title" content="Release notes — v2.0">
  <meta name="description" content="Summary of breaking changes in the 2.0 release.">
  <script type="application/ld+json">
  {
    "@context": "https://schema.org",
    "@type": "Article",
    "headline": "Version 2.0 shipped",
    "articleBody": "We removed legacy endpoints and improved throughput on batch reads."
  }
  </script>
</head>
<body>
  <div id="root"></div>
  <noscript>
    <p>Enable JavaScript to view the interactive changelog.</p>
  </noscript>
  <script id="__NEXT_DATA__" type="application/json">
  {"props":{"pageProps":{"title":"Release notes","description":"Batch reads now run in parallel via rayon when you pass multiple URLs to the CLI."}}}
  </script>
  <script>function bundleInit(){}</script>
</body>
</html>"#
            .to_string(),
        }
    }

    pub fn article_in_main(title: &str, body: &str) -> HtmlDocument {
        HtmlDocument {
            html: format!(
                r#"<!DOCTYPE html><html><head><title>{title}</title></head>
<body><main><p>{body}</p></main></body></html>"#
            ),
        }
    }

    pub fn plain_text(content: &str) -> String {
        content.to_string()
    }
}

impl HtmlDocument {
    pub fn write_file_url(self) -> std::io::Result<TempFileUrl> {
        FileUrlFactory::from_html(&self.html)
    }
}

pub struct FileUrlFactory;

impl FileUrlFactory {
    pub fn from_html(html: &str) -> std::io::Result<TempFileUrl> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("page.html");
        let mut file = std::fs::File::create(&path)?;
        file.write_all(html.as_bytes())?;
        let url = url::Url::from_file_path(&path)
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "file url"))?
            .to_string();
        Ok(TempFileUrl { _dir: dir, url })
    }
}

pub struct HttpFactory;

impl HttpFactory {
    pub fn serve_html(html: &str) -> HttpHtmlServer {
        Self::serve_html_at(html, "/")
    }

    pub fn serve_html_at(html: &str, path: &str) -> HttpHtmlServer {
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(GET).path(path);
            then.status(200)
                .header("content-type", "text/html; charset=utf-8")
                .body(html);
        });
        let url = format!("{}{}", server.base_url(), path.trim_start_matches('/'));
        HttpHtmlServer { server, url }
    }
}

pub struct UrlFactory;

impl UrlFactory {
    pub fn reddit_thread() -> &'static str {
        "https://www.reddit.com/r/rust/comments/bb5lnj/example/"
    }

    pub fn doc_rust_lang() -> &'static str {
        "https://doc.rust-lang.org/book/"
    }
}
