# Documentation reading (tro MCP)

For **any technical documentation** (Rust book, MDN, framework docs, API reference, man pages on the web):

1. **Never** use raw fetch, browser tools, or paste HTML into context.
2. **Always** use MCP server **`tro`**.

## One page

Tool **`read_url`**:

```json
{ "url": "https://doc.rust-lang.org/book/ch01-02-hello-world.html" }
```

## Multiple pages (preferred when comparing or reading a chapter split across URLs)

Tool **`read_urls`** — fetches **in parallel**:

```json
{
  "urls": [
    "https://doc.rust-lang.org/std/primitive.str.html",
    "https://doc.rust-lang.org/std/string/struct.String.html"
  ],
  "max_chars": 60000
}
```

## Token control

- Use **`max_chars`** on huge pages (default: full extract). Example: `80000` per page.
- Use returned **`text`** only; ignore HTML.
- If **`truncated`** is true, ask for a more specific sub-URL instead of widening the cap.

## CLI (manual)

```bash
tro --json --max-chars=60000 URL1 URL2
```

## Limits

Static / SSR documentation only. Pure client-rendered SPAs with no content in HTML may be empty.

`reddit.com` / `www.reddit.com` URLs are fetched via **`old.reddit.com`** automatically.
