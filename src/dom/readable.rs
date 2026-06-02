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
    let cap = opts
        .max_chars
        .unwrap_or_else(|| html.len().min(256 * 1024) / 4);
    let mut budget = TextBudget::new(opts.max_chars, cap);
    append_readable(&root, &mut budget);
    let mut out = budget.out;
    let was_truncated = budget.truncated;
    normalize_in_place(&mut out);

    let title = spa::pick_title(trim_owned(title), &dom.document);

    if was_truncated {
        finalize_truncated(&mut out);
        return Ok(ReadablePage {
            title,
            text: out,
            truncated: true,
        });
    }

    let thin = out.len() < 320;
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

struct TextBudget {
    out: String,
    chars: usize,
    max: Option<usize>,
    truncated: bool,
}

impl TextBudget {
    fn new(max: Option<usize>, capacity: usize) -> Self {
        Self {
            out: String::with_capacity(capacity),
            chars: 0,
            max,
            truncated: false,
        }
    }

    fn at_limit(&self) -> bool {
        self.truncated || self.max.is_some_and(|m| self.chars >= m)
    }

    /// Append one character. Returns `false` when the budget is exhausted.
    fn push_char(&mut self, ch: char) -> bool {
        if self.at_limit() {
            self.truncated = true;
            return false;
        }
        self.out.push(ch);
        self.chars += 1;
        if self.max.is_some_and(|m| self.chars >= m) {
            self.truncated = true;
            return false;
        }
        true
    }

    /// Append a string. Returns `false` when the budget is exhausted mid-string.
    fn push_str(&mut self, s: &str) -> bool {
        for ch in s.chars() {
            if !self.push_char(ch) {
                return false;
            }
        }
        true
    }
}

fn finalize_truncated(text: &mut String) {
    if !text.ends_with("[truncated]") {
        text.push_str("\n\n[truncated]");
    }
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
fn append_readable(handle: &Handle, budget: &mut TextBudget) -> bool {
    if budget.at_limit() {
        return false;
    }
    match &handle.data {
        NodeData::Text { contents } => push_words(budget, contents.borrow().as_ref()),
        NodeData::Element { name, attrs, .. } => {
            let tag = name.local.as_ref();
            if skip_tag_name(tag) {
                return true;
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
                return true;
            }
            if is_block(tag) && needs_break(&budget.out) {
                if !budget.push_char('\n') {
                    return false;
                }
            }
            let children = handle.children.borrow();
            match tag {
                "br" => budget.push_char('\n'),
                "hr" => {
                    if needs_break(&budget.out) && !budget.push_char('\n') {
                        return false;
                    }
                    budget.push_str("---\n")
                }
                "li" => {
                    if !budget.push_str("- ") {
                        return false;
                    }
                    for child in children.iter() {
                        if !append_readable(child, budget) {
                            return false;
                        }
                    }
                    budget.push_char('\n')
                }
                "img" => {
                    if let Some(alt) = parsed.alt {
                        let alt = alt.trim();
                        if !alt.is_empty() {
                            push_words(budget, alt)
                        } else {
                            true
                        }
                    } else {
                        true
                    }
                }
                "a" => {
                    for child in children.iter() {
                        if !append_readable(child, budget) {
                            return false;
                        }
                    }
                    if let Some(href) = parsed.href {
                        let href = href.trim();
                        if link_url_is_useful(href) && !href.starts_with('/') {
                            if !budget.push_str(" (") {
                                return false;
                            }
                            if !push_words(budget, href) {
                                return false;
                            }
                            budget.push_char(')')
                        } else {
                            true
                        }
                    } else {
                        true
                    }
                }
                "tr" => {
                    let mut first_cell = true;
                    for child in children.iter() {
                        if first_cell {
                            first_cell = false;
                        } else if !budget.push_char('\t') {
                            return false;
                        }
                        if !append_readable(child, budget) {
                            return false;
                        }
                    }
                    budget.push_char('\n')
                }
                "td" | "th" => {
                    for child in children.iter() {
                        if !append_readable(child, budget) {
                            return false;
                        }
                    }
                    true
                }
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    for child in children.iter() {
                        if !append_readable(child, budget) {
                            return false;
                        }
                    }
                    budget.push_str("\n\n")
                }
                _ => {
                    for child in children.iter() {
                        if !append_readable(child, budget) {
                            return false;
                        }
                    }
                    if matches!(
                        tag,
                        "p" | "div" | "blockquote" | "pre" | "section" | "article" | "main"
                            | "ul" | "ol" | "table" | "figcaption"
                    ) {
                        budget.push_char('\n')
                    } else {
                        true
                    }
                }
            }
        }
        _ => true,
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
fn push_words(budget: &mut TextBudget, text: &str) -> bool {
    if text.as_bytes().iter().all(|b| b.is_ascii()) {
        return push_words_ascii(budget, text.as_bytes());
    }
    let mut space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !space && !budget.out.is_empty() && !budget.out.ends_with('\n') {
                if !budget.push_char(' ') {
                    return false;
                }
                space = true;
            }
        } else if !budget.push_char(ch) {
            return false;
        } else {
            space = false;
        }
    }
    true
}

#[inline]
fn push_words_ascii(budget: &mut TextBudget, text: &[u8]) -> bool {
    let mut space = false;
    for &b in text {
        if b.is_ascii_whitespace() {
            if !space && !budget.out.is_empty() && !budget.out.ends_with('\n') {
                if !budget.push_char(' ') {
                    return false;
                }
                space = true;
            }
        } else if !budget.push_char(b as char) {
            return false;
        } else {
            space = false;
        }
    }
    true
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

