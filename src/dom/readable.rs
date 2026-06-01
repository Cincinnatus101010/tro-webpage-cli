use super::spa;
use html5ever::parse_document;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use std::fmt;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtractOptions {
    pub max_chars: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadablePage {
    pub title: String,
    pub text: String,
    pub truncated: bool,
}

#[derive(Debug)]
pub enum DomError {
    Parse(String),
}

impl fmt::Display for DomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DomError::Parse(msg) => write!(f, "HTML parse failed: {msg}"),
        }
    }
}

impl std::error::Error for DomError {}

pub fn extract_html(html: &str) -> Result<ReadablePage, DomError> {
    extract_html_with_options(html, &ExtractOptions::default())
}

pub fn extract_html_with_options(html: &str, opts: &ExtractOptions) -> Result<ReadablePage, DomError> {
    let trimmed = html.trim_start();
    if !trimmed.starts_with('<') {
        let (text, truncated) = truncate_text(html, opts.max_chars);
        return Ok(ReadablePage {
            title: String::new(),
            text,
            truncated,
        });
    }

    let dom = parse_document(RcDom::default(), Default::default())
        .from_utf8()
        .read_from(&mut html.as_bytes())
        .map_err(|e| DomError::Parse(format!("{e}")))?;

    let mut title = String::new();
    if let Some(head) = find_tag(&dom.document, "head") {
        if let Some(title_node) = find_tag(&head, "title") {
            inner_text(&title_node, &mut title);
        }
    }

    let root = content_root(&dom.document);
    let mut out = String::with_capacity(html.len().min(256 * 1024) / 4);
    append_readable(&root, &mut out);
    normalize_in_place(&mut out);

    let thin = out.len() < 320;
    let title = spa::pick_title(trim_owned(title), &dom.document);
    let mut text = if thin {
        spa::merge_visible(&out, &dom.document, html)
    } else {
        out
    };
    if thin {
        normalize_in_place(&mut text);
    }
    let (text, truncated) = truncate_text(&text, opts.max_chars);
    Ok(ReadablePage {
        title,
        text,
        truncated,
    })
}

#[derive(Default)]
struct Attrs<'a> {
    alt: Option<&'a str>,
    href: Option<&'a str>,
    role: Option<&'a str>,
    aria_hidden: Option<&'a str>,
    hidden: bool,
}

#[inline]
fn append_readable(handle: &Handle, out: &mut String) {
    match &handle.data {
        NodeData::Text { contents } => push_words(out, contents.borrow().as_ref()),
        NodeData::Element { name, attrs, .. } => {
            let tag = name.local.as_ref();
            if skip_tag_name(tag) {
                return;
            }

            let attrs_ref = attrs.borrow();
            let mut parsed = Attrs::default();
            for attr in attrs_ref.iter() {
                match attr.name.local.as_ref() {
                    "alt" => parsed.alt = Some(attr.value.as_ref()),
                    "href" => parsed.href = Some(attr.value.as_ref()),
                    "role" => parsed.role = Some(attr.value.as_ref()),
                    "aria-hidden" => parsed.aria_hidden = Some(attr.value.as_ref()),
                    "hidden" => parsed.hidden = true,
                    _ => {}
                }
            }

            if skip_attrs(&parsed) {
                return;
            }
            if is_block(tag) && needs_break(out) {
                out.push('\n');
            }
            let children = handle.children.borrow();
            match tag {
                "br" => out.push('\n'),
                "hr" => {
                    if needs_break(out) {
                        out.push('\n');
                    }
                    out.push_str("---\n");
                }
                "li" => {
                    out.push_str("- ");
                    for child in children.iter() {
                        append_readable(child, out);
                    }
                    out.push('\n');
                }
                "img" => {
                    if let Some(alt) = parsed.alt {
                        let alt = alt.trim();
                        if !alt.is_empty() {
                            push_words(out, alt);
                        }
                    }
                }
                "a" => {
                    for child in children.iter() {
                        append_readable(child, out);
                    }
                    if let Some(href) = parsed.href {
                        let href = href.trim();
                        if link_url_is_useful(href) && !href.starts_with('/') {
                            out.push_str(" (");
                            out.push_str(href);
                            out.push(')');
                        }
                    }
                }
                "tr" => {
                    let mut first_cell = true;
                    for child in children.iter() {
                        if first_cell {
                            first_cell = false;
                        } else {
                            out.push('\t');
                        }
                        append_readable(child, out);
                    }
                    out.push('\n');
                }
                "td" | "th" => {
                    for child in children.iter() {
                        append_readable(child, out);
                    }
                }
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    for child in children.iter() {
                        append_readable(child, out);
                    }
                    out.push_str("\n\n");
                }
                _ => {
                    for child in children.iter() {
                        append_readable(child, out);
                    }
                    if matches!(
                        tag,
                        "p" | "div" | "blockquote" | "pre" | "section" | "article" | "main"
                            | "ul" | "ol" | "table" | "figcaption"
                    ) {
                        out.push('\n');
                    }
                }
            }
        }
        _ => {}
    }
}

#[inline]
fn content_root(handle: &Handle) -> Handle {
    let body = find_tag(handle, "body").unwrap_or_else(|| handle.clone());
    find_tag(&body, "main")
        .or_else(|| find_tag(&body, "article"))
        .or_else(|| find_by_role(&body, "main"))
        .or_else(|| find_by_id(&body, "content"))
        .or_else(|| find_by_id(&body, "main-content"))
        .or_else(|| find_by_class_hint(&body, "markdown-body"))
        .or_else(|| find_by_class_hint(&body, "theme-doc-markdown"))
        .or_else(|| find_by_class_hint(&body, "documentation"))
        .unwrap_or(body)
}

fn find_by_role(handle: &Handle, role: &str) -> Option<Handle> {
    let mut stack = vec![handle.clone()];
    while let Some(node) = stack.pop() {
        if element_attr_eq(&node, "role", role) {
            return Some(node);
        }
        stack.extend(node.children.borrow().iter().cloned());
    }
    None
}

fn find_by_id(handle: &Handle, id: &str) -> Option<Handle> {
    let mut stack = vec![handle.clone()];
    while let Some(node) = stack.pop() {
        if element_attr_eq(&node, "id", id) {
            return Some(node);
        }
        stack.extend(node.children.borrow().iter().cloned());
    }
    None
}

fn find_by_class_hint(handle: &Handle, hint: &str) -> Option<Handle> {
    let mut stack = vec![handle.clone()];
    while let Some(node) = stack.pop() {
        if let Some(class) = element_attr_value(&node, "class") {
            if class.split_whitespace().any(|c| c.contains(hint)) {
                return Some(node);
            }
        }
        stack.extend(node.children.borrow().iter().cloned());
    }
    None
}

fn element_attr_value(handle: &Handle, name: &str) -> Option<String> {
    let NodeData::Element { attrs, .. } = &handle.data else {
        return None;
    };
    attrs
        .borrow()
        .iter()
        .find(|a| a.name.local.as_ref() == name)
        .map(|a| a.value.to_string())
}

fn element_attr_eq(handle: &Handle, name: &str, want: &str) -> bool {
    element_attr_value(handle, name).is_some_and(|v| v == want)
}

fn truncate_text(text: &str, max_chars: Option<usize>) -> (String, bool) {
    let Some(max) = max_chars else {
        return (text.to_string(), false);
    };
    if text.chars().count() <= max {
        return (text.to_string(), false);
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let slice = &text[..end];
    let cut = slice
        .rfind("\n\n")
        .or_else(|| slice.rfind('\n'))
        .unwrap_or(end);
    let mut out = slice[..cut].trim_end().to_string();
    out.push_str("\n\n[truncated]");
    (out, true)
}

#[inline]
fn find_tag(handle: &Handle, tag: &str) -> Option<Handle> {
    let mut stack = vec![handle.clone()];
    while let Some(node) = stack.pop() {
        if element_is(&node, tag) {
            return Some(node);
        }
        let children = node.children.borrow();
        stack.extend(children.iter().cloned());
    }
    None
}

#[inline]
fn element_is(handle: &Handle, tag: &str) -> bool {
    matches!(
        &handle.data,
        NodeData::Element { name, .. } if name.local.as_ref() == tag
    )
}

#[inline]
fn inner_text(handle: &Handle, out: &mut String) {
    match &handle.data {
        NodeData::Text { contents } => out.push_str(contents.borrow().as_ref()),
        NodeData::Element { .. } => {
            for child in handle.children.borrow().iter() {
                inner_text(child, out);
            }
        }
        _ => {}
    }
}

#[inline]
fn skip_tag_name(tag: &str) -> bool {
    matches!(
        tag,
        "script" | "style" | "head" | "meta" | "link" | "title" | "noscript" | "svg"
            | "template" | "canvas" | "iframe" | "object" | "embed" | "source" | "track"
            | "map" | "area" | "base" | "nav" | "header" | "footer" | "aside"
    )
}

#[inline]
fn skip_attrs(attrs: &Attrs<'_>) -> bool {
    if matches!(
        attrs.role,
        Some("navigation" | "banner" | "contentinfo" | "complementary")
    ) {
        return true;
    }
    attrs.aria_hidden == Some("true") || attrs.hidden
}

#[inline]
fn is_block(tag: &str) -> bool {
    matches!(
        tag,
        "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol" | "li"
            | "blockquote" | "pre" | "section" | "article" | "main" | "br" | "hr" | "table"
            | "tr" | "figcaption"
    )
}

#[inline]
fn needs_break(out: &str) -> bool {
    !out.is_empty() && !out.ends_with('\n')
}

#[inline]
fn link_url_is_useful(href: &str) -> bool {
    !href.is_empty()
        && !href.starts_with('#')
        && !href.starts_with("javascript:")
        && href != "/"
        && (href.starts_with("http://") || href.starts_with("https://"))
}

#[inline]
fn push_words(out: &mut String, text: &str) {
    if text.as_bytes().iter().all(|b| b.is_ascii()) {
        push_words_ascii(out, text.as_bytes());
        return;
    }
    let mut space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !space && !out.is_empty() && !out.ends_with('\n') {
                out.push(' ');
                space = true;
            }
        } else {
            out.push(ch);
            space = false;
        }
    }
}

#[inline]
fn push_words_ascii(out: &mut String, text: &[u8]) {
    let mut space = false;
    for &b in text {
        if b.is_ascii_whitespace() {
            if !space && !out.is_empty() && !out.ends_with('\n') {
                out.push(' ');
                space = true;
            }
        } else {
            out.push(b as char);
            space = false;
        }
    }
}

fn normalize_in_place(out: &mut String) {
    let mut normalized = String::with_capacity(out.len());
    let mut after_blank = true;
    for line in out.lines() {
        let t = line.trim();
        if t.is_empty() {
            if !after_blank && !normalized.is_empty() {
                normalized.push('\n');
                after_blank = true;
            }
        } else {
            if !normalized.is_empty() {
                normalized.push('\n');
            }
            normalized.push_str(t);
            after_blank = false;
        }
    }
    *out = normalized;
}

fn trim_owned(s: String) -> String {
    let trimmed = s.trim();
    if trimmed.len() == s.len() {
        return s;
    }
    trimmed.to_string()
}

