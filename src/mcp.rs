use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::sync::OnceLock;
use tro::{
    extract_url_cached, extract_urls_cached, ExtractOptions, PageCache,
};

#[derive(Debug, Deserialize)]
struct RpcRequest {
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct RpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

#[derive(Debug, Deserialize)]
struct ReadUrlArgs {
    url: String,
    #[serde(default)]
    max_chars: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ReadUrlsArgs {
    urls: Vec<String>,
    #[serde(default)]
    max_chars: Option<usize>,
}

fn ok(id: Value, result: Value) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    }
}

fn err(id: Value, code: i32, message: impl Into<String>) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(RpcError {
            code,
            message: message.into(),
        }),
    }
}

fn tools_list() -> Value {
    json!([
        {
            "name": "read_url",
            "description": "Fetch one documentation URL and return readable title + text (use for a single page)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "http://, https://, or file:// URL" },
                    "max_chars": { "type": "integer", "description": "Optional cap on body text length" }
                },
                "required": ["url"]
            }
        },
        {
            "name": "read_urls",
            "description": "Fetch multiple documentation URLs in parallel; prefer this when reading several doc pages at once",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "urls": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of doc URLs to fetch in parallel"
                    },
                    "max_chars": { "type": "integer", "description": "Optional cap per page" }
                },
                "required": ["urls"]
            }
        }
    ])
}

fn opts(max_chars: Option<usize>) -> ExtractOptions {
    ExtractOptions { max_chars }
}

fn page_cache() -> &'static PageCache {
    static CACHE: OnceLock<PageCache> = OnceLock::new();
    CACHE.get_or_init(PageCache::default)
}

fn tool_result(id: Value, value: Value, is_error: bool) -> RpcResponse {
    ok(
        id,
        json!({
            "content": [{ "type": "text", "text": value.to_string() }],
            "isError": is_error
        }),
    )
}

fn handle_request(req: RpcRequest) -> Option<RpcResponse> {
    let id = req.id.clone().unwrap_or(Value::Null);

    match req.method.as_str() {
        "initialize" => Some(ok(
            id,
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "tro", "version": env!("CARGO_PKG_VERSION") }
            }),
        )),
        "notifications/initialized" | "initialized" => None,
        "tools/list" => Some(ok(id, json!({ "tools": tools_list() }))),
        "tools/call" => {
            let name = req.params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = req.params.get("arguments").cloned().unwrap_or(Value::Null);
            match name {
                "read_url" => {
                    let args: ReadUrlArgs = match serde_json::from_value(args) {
                        Ok(a) => a,
                        Err(e) => return Some(err(id, -32602, format!("bad arguments: {e}"))),
                    };
                    if args.url.is_empty() {
                        return Some(err(id, -32602, "missing url"));
                    }
                    return match extract_url_cached(page_cache(), &args.url, &opts(args.max_chars)) {
                        Ok(page) => Some(tool_result(
                            id,
                            json!({
                                "url": args.url,
                                "title": page.title,
                                "text": page.text,
                                "truncated": page.truncated,
                            }),
                            false,
                        )),
                        Err(e) => Some(tool_result(id, json!(format!("{e}")), true)),
                    };
                }
                "read_urls" => {
                    let args: ReadUrlsArgs = match serde_json::from_value(args) {
                        Ok(a) => a,
                        Err(e) => return Some(err(id, -32602, format!("bad arguments: {e}"))),
                    };
                    if args.urls.is_empty() {
                        return Some(err(id, -32602, "missing urls"));
                    }
                    let refs: Vec<&str> = args.urls.iter().map(String::as_str).collect();
                    let pages = extract_urls_cached(page_cache(), &refs, &opts(args.max_chars));
                    return Some(tool_result(id, json!({ "pages": pages }), false));
                }
                _ => Some(err(id, -32602, format!("unknown tool: {name}"))),
            }
        }
        "ping" => Some(ok(id, json!({}))),
        _ => Some(err(id, -32601, format!("method not found: {}", req.method))),
    }
}

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let req: RpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = err(Value::Null, -32700, format!("parse error: {e}"));
                writeln!(stdout, "{}", serde_json::to_string(&resp).unwrap())?;
                stdout.flush()?;
                continue;
            }
        };
        if let Some(resp) = handle_request(req) {
            writeln!(stdout, "{}", serde_json::to_string(&resp).unwrap())?;
            stdout.flush()?;
        }
    }
    Ok(())
}
