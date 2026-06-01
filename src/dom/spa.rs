use markup5ever_rcdom::{Handle, NodeData};
use serde_json::Value;

const TEXT_KEYS: &[&str] = &[
    "name",
    "title",
    "headline",
    "description",
    "articleBody",
    "text",
    "content",
    "body",
    "summary",
    "subtitle",
    "abstract",
    "caption",
    "message",
    "about",
];

pub fn merge_visible(visible: &str, root: &Handle, raw_html: &str) -> String {
    if !is_thin(visible) {
        return visible.to_string();
    }
    let mut extra = String::new();
    if let Some(head) = find_tag(root, "head") {
        meta_description(&head, &mut extra);
    }
    extract_from_raw_html(raw_html, &mut extra);
    walk_scripts(root, &mut extra);
    walk_noscript(root, &mut extra);

    let visible = visible.trim();
    if extra.is_empty() {
        return visible.to_string();
    }

    let mut out = String::with_capacity(visible.len() + extra.len() + 32);
    if is_thin(visible) {
        out.push_str(&extra);
        if !visible.is_empty() && !out.contains(visible) {
            if !out.is_empty() {
                out.push_str("\n\n---\n\n");
            }
            out.push_str(visible);
        }
    } else {
        out.push_str(visible);
        append_unique_block(&mut out, &extra);
    }
    out
}

pub fn pick_title(title: String, root: &Handle) -> String {
    let mut meta_title = String::new();
    let mut _extra = String::new();
    if let Some(head) = find_tag(root, "head") {
        meta_text(&head, &mut meta_title, &mut _extra);
    }
    let title = title.trim();
    if !meta_title.is_empty() && (title.is_empty() || weak_title(title)) {
        return meta_title;
    }
    if title.is_empty() {
        return meta_title;
    }
    title.to_string()
}

fn weak_title(title: &str) -> bool {
    title.len() < 6
        || title.ends_with('…')
        || title.eq_ignore_ascii_case("loading...")
        || title.eq_ignore_ascii_case("loading")
}

fn is_thin(text: &str) -> bool {
    text.trim().len() < 320
}

fn meta_description(head: &Handle, out: &mut String) {
    let mut title = String::new();
    meta_text(head, &mut title, out);
}

fn meta_text(head: &Handle, title: &mut String, out: &mut String) {
    walk_elements(head, &mut |node, tag| {
        if tag != "meta" {
            return;
        }
        let Some(content) = element_attr(node, "content") else {
            return;
        };
        let content = content.trim();
        if content.is_empty() {
            return;
        }
        let key = element_attr(node, "property")
            .or_else(|| element_attr(node, "name"))
            .unwrap_or_default()
            .to_ascii_lowercase();

        if (title.is_empty() || weak_title(title))
            && (key == "og:title" || key == "twitter:title")
        {
            *title = content.to_string();
            return;
        }
        if matches!(
            key.as_str(),
            "og:description" | "twitter:description" | "description"
        ) {
            append_unique_block(out, content);
        }
    });
}

fn walk_scripts(root: &Handle, out: &mut String) {
    walk_elements(root, &mut |node, tag| {
        if tag != "script" {
            return;
        }
        let script_type = element_attr(node, "type").unwrap_or_default();
        let id = element_attr(node, "id").unwrap_or_default();
        let mut text = String::new();
        script_inner_text(node, &mut text);
        if text.is_empty() {
            return;
        }
        if script_type.eq_ignore_ascii_case("application/ld+json") {
            append_json_text(out, &text);
            return;
        }
        if id == "__NEXT_DATA__" || id == "__NUXT_DATA__" {
            append_json_text(out, &text);
            return;
        }
        for marker in [
            "window.__NUXT__",
            "window.__INITIAL_STATE__",
            "window.__PRELOADED_STATE__",
        ] {
            if let Some(json) = extract_assignment_json(&text, marker) {
                append_json_text(out, json);
            }
        }
    });
}

fn walk_noscript(root: &Handle, out: &mut String) {
    walk_elements(root, &mut |node, tag| {
        if tag != "noscript" {
            return;
        }
        let mut text = String::new();
        element_text(node, &mut text);
        let text = strip_tags(&text);
        append_unique_block(out, &text);
    });
}

fn extract_from_raw_html(html: &str, out: &mut String) {
    let mut pos = 0usize;
    while let Some(found) = html[pos..].find("<noscript") {
        pos += found;
        if let Some(inner) = extract_element_inner(html, pos, "noscript") {
            append_unique_block(out, &strip_tags(&inner));
        }
        pos += 1;
    }
    for id in ["__NEXT_DATA__", "__NUXT_DATA__", "__NUXT__"] {
        if let Some(body) = extract_script_inner(html, id) {
            append_json_text(out, &body);
        }
    }
    let mut search = 0usize;
    while let Some(pos) = html[search..].find("application/ld+json") {
        search += pos;
        if let Some(body) = extract_script_after_type(html, search) {
            append_json_text(out, &body);
        }
        search += 1;
    }
    for marker in [
        "window.__NUXT__",
        "window.__INITIAL_STATE__",
        "window.__PRELOADED_STATE__",
    ] {
        if let Some(json) = extract_assignment_json(html, marker) {
            append_json_text(out, json);
        }
    }
}

fn extract_element_inner(html: &str, start: usize, tag: &str) -> Option<String> {
    let open = html[start..].find('>')? + start + 1;
    let close = html[open..].find(&format!("</{tag}>"))? + open;
    Some(html[open..close].to_string())
}

fn extract_script_inner(html: &str, id: &str) -> Option<String> {
    let needle = format!("id=\"{id}\"");
    let start = html.find(&needle).or_else(|| html.find(&format!("id='{id}'")))?;
    let open = html[start..].find('>')? + start + 1;
    let close = html[open..].find("</script>")? + open;
    Some(html[open..close].trim().to_string())
}

fn extract_script_after_type(html: &str, type_pos: usize) -> Option<String> {
    let open = html[type_pos..].find('>')? + type_pos + 1;
    let close = html[open..].find("</script>")? + open;
    let body = html[open..close].trim();
    if body.starts_with('{') || body.starts_with('[') {
        Some(body.to_string())
    } else {
        None
    }
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    normalize_ws(&out)
}

fn normalize_ws(s: &str) -> String {
    let mut out = String::new();
    let mut space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !space && !out.is_empty() {
                out.push(' ');
                space = true;
            }
        } else {
            out.push(ch);
            space = false;
        }
    }
    out.trim().to_string()
}

fn walk_elements<F>(handle: &Handle, f: &mut F)
where
    F: FnMut(&Handle, &str),
{
    let mut stack = vec![handle.clone()];
    while let Some(node) = stack.pop() {
        if let NodeData::Element { name, .. } = &node.data {
            let tag = name.local.as_ref();
            f(&node, tag);
            let children = node.children.borrow();
            stack.extend(children.iter().cloned());
        }
    }
}

fn element_attr(node: &Handle, name: &str) -> Option<String> {
    let NodeData::Element { attrs, .. } = &node.data else {
        return None;
    };
    attrs
        .borrow()
        .iter()
        .find(|a| a.name.local.as_ref() == name)
        .map(|a| a.value.to_string())
}

fn find_tag(handle: &Handle, tag: &str) -> Option<Handle> {
    let mut stack = vec![handle.clone()];
    while let Some(node) = stack.pop() {
        if matches!(
            &node.data,
            NodeData::Element { name, .. } if name.local.as_ref() == tag
        ) {
            return Some(node);
        }
        let children = node.children.borrow();
        stack.extend(children.iter().cloned());
    }
    None
}

fn script_inner_text(handle: &Handle, out: &mut String) {
    match &handle.data {
        NodeData::Text { contents } => out.push_str(contents.borrow().as_ref()),
        NodeData::Element { .. } => {
            for child in handle.children.borrow().iter() {
                script_inner_text(child, out);
            }
        }
        _ => {}
    }
}

fn element_text(handle: &Handle, out: &mut String) {
    match &handle.data {
        NodeData::Text { contents } => push_lineish(out, contents.borrow().as_ref()),
        NodeData::Element { name, .. } => {
            let tag = name.local.as_ref();
            if matches!(tag, "script" | "style") {
                return;
            }
            for child in handle.children.borrow().iter() {
                element_text(child, out);
            }
            if matches!(tag, "p" | "div" | "br" | "li" | "h1" | "h2" | "h3") {
                out.push('\n');
            }
        }
        _ => {}
    }
}

fn extract_assignment_json<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    let start = text.find(marker)? + marker.len();
    let rest = text[start..].trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    json_slice(rest)
}

fn json_slice(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_string {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return text.get(start..=i);
                }
            }
            _ => {}
        }
    }
    None
}

fn append_json_text(out: &mut String, json: &str) {
    let Ok(value) = serde_json::from_str::<Value>(json) else {
        return;
    };
    let mut parts = Vec::new();
    harvest_json_strings(&value, &mut parts, 0);
    for part in parts {
        append_unique_block(out, &part);
    }
}

fn harvest_json_strings(value: &Value, out: &mut Vec<String>, depth: usize) {
    if depth > 10 {
        return;
    }
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                if TEXT_KEYS.contains(&k.as_str()) {
                    push_json_string(v, out);
                } else if k == "@graph" || k == "props" || k == "pageProps" || k == "data" {
                    harvest_json_strings(v, out, depth + 1);
                } else if depth < 4 {
                    harvest_json_strings(v, out, depth + 1);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                harvest_json_strings(item, out, depth + 1);
            }
        }
        _ => push_json_string(value, out),
    }
}

fn push_json_string(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(s) => {
            let s = s.trim();
            if s.len() >= 20 && s.chars().filter(|c| c.is_alphabetic()).count() >= 10 {
                out.push(s.to_string());
            }
        }
        Value::Array(items) => {
            for item in items {
                push_json_string(item, out);
            }
        }
        _ => {}
    }
}

fn append_unique_block(out: &mut String, chunk: &str) {
    let chunk = chunk.trim();
    if chunk.len() < 16 {
        return;
    }
    if out.contains(chunk) {
        return;
    }
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(chunk);
}

fn push_lineish(out: &mut String, text: &str) {
    let t = text.trim();
    if t.is_empty() {
        return;
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push(' ');
    }
    out.push_str(t);
}

