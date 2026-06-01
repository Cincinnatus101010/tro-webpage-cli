use tro::{extract_url_with_options, extract_urls, ExtractOptions};

fn usage() -> ! {
    eprintln!("usage: tro [--json] [--max-chars N] <url>...");
    eprintln!("       multiple URLs are fetched in parallel");
    std::process::exit(1);
}

fn valid_url(arg: &str) -> bool {
    arg.starts_with("http://")
        || arg.starts_with("https://")
        || arg.starts_with("file://")
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut json = false;
    let mut max_chars = None;
    let mut urls = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        match raw[i].as_str() {
            "--json" => json = true,
            "--max-chars" => {
                i += 1;
                max_chars = raw.get(i).and_then(|s| s.parse().ok());
            }
            a if a.starts_with("--max-chars=") => {
                max_chars = a.strip_prefix("--max-chars=").and_then(|s| s.parse().ok());
            }
            a if valid_url(a) => urls.push(a.to_string()),
            _ => {}
        }
        i += 1;
    }
    if urls.is_empty() {
        usage();
    }
    let opts = ExtractOptions { max_chars };
    let refs: Vec<&str> = urls.iter().map(String::as_str).collect();

    if urls.len() == 1 {
        match extract_url_with_options(refs[0], &opts) {
            Ok(page) if json => {
                println!(
                    "{}",
                    serde_json::json!({
                        "url": refs[0],
                        "title": page.title,
                        "text": page.text,
                        "truncated": page.truncated,
                    })
                );
            }
            Ok(page) => {
                if page.title.is_empty() {
                    print!("{}", page.text);
                } else {
                    println!("{}\n\n{}", page.title, page.text);
                }
            }
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }

    let pages = extract_urls(&refs, &opts);
    if json {
        println!("{}", serde_json::json!({ "pages": pages }));
        return;
    }
    for page in pages {
        if let Some(err) = &page.error {
            eprintln!("{}: {err}", page.url);
            continue;
        }
        let title = page.title.as_deref().unwrap_or("");
        let text = page.text.as_deref().unwrap_or("");
        if title.is_empty() {
            println!("=== {}\n{}\n", page.url, text);
        } else {
            println!("=== {} — {}\n{}\n", page.url, title, text);
        }
    }
}
