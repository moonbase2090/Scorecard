// SPDX-License-Identifier: MPL-2.0
//! HTML document parse for the web pack.
//!
//! `html5ever` builds the tree and reports parse errors with the current
//! line. A separate tag stack reports unclosed and misnested tags, because
//! the HTML parser closes many of those for you.

use std::cell::{Cell, RefCell};

use html5ever::interface::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::{local_name, ns, parse_document, Attribute, ParseOpts, QualName};
use sc_core::{Finding, Span};

#[derive(Debug, Clone)]
pub struct Elem {
    pub name: String,
    pub line: u32,
    pub col: u32,
    pub attrs: Vec<(String, String)>,
    /// Descendant text, excluding `script` and `style`.
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct InlineScript {
    pub line: u32,
    pub body: String,
}

#[derive(Debug)]
pub struct Parsed {
    pub elements: Vec<Elem>,
    pub scripts: Vec<InlineScript>,
    pub has_doctype: bool,
    pub errors: Vec<(u32, String)>,
}

pub fn parse(text: &str) -> Parsed {
    let parsed = parse_document(Sink::new(), ParseOpts::default()).one(text);
    let mut out = parsed;
    out.scripts = inline_scripts(text);
    out
}

pub fn html_findings(file: &str, text: &str) -> (Vec<Finding>, Parsed) {
    let parsed = parse(text);
    let mut findings = Vec::new();
    if !parsed.has_doctype {
        findings.push(html_finding(
            file,
            "html.doctype",
            1,
            1,
            "missing <!doctype html>",
        ));
    }
    if !has_viewport(&parsed.elements) {
        findings.push(html_finding(
            file,
            "html.viewport",
            1,
            1,
            "missing <meta name=\"viewport\">",
        ));
    }
    for (line, message) in &parsed.errors {
        findings.push(html_finding(
            file,
            "html.parse",
            *line,
            column_on(text, *line),
            message,
        ));
    }
    findings.extend(balance_findings(file, text));
    (findings, parsed)
}

fn html_finding(file: &str, rule: &str, line: u32, col: u32, message: &str) -> Finding {
    Finding {
        id: format!("html:{file}:{line}:{rule}"),
        rule: rule.to_string(),
        engine: "html".into(),
        severity: "error".into(),
        file: file.to_string(),
        span: Some(Span {
            start_line: line.max(1),
            start_col: col.max(1),
            end_line: line.max(1),
            end_col: col.max(1).saturating_add(1),
        }),
        symbol: None,
        message: message.to_string(),
        evidence: serde_json::json!({"rule": rule}),
        suggested_action: Some("Fix the markup so the page parses cleanly.".into()),
        disposition: String::new(),
    }
}

fn has_viewport(elements: &[Elem]) -> bool {
    elements.iter().any(|elem| {
        elem.name == "meta"
            && elem.attrs.iter().any(|(key, value)| {
                key.eq_ignore_ascii_case("name") && value.eq_ignore_ascii_case("viewport")
            })
            && elem
                .attrs
                .iter()
                .any(|(key, value)| key.eq_ignore_ascii_case("content") && !value.trim().is_empty())
    })
}

fn column_on(text: &str, line: u32) -> u32 {
    let source = text
        .lines()
        .nth(line.saturating_sub(1) as usize)
        .unwrap_or("");
    let lower = source.to_ascii_lowercase();
    lower.find('<').map(|index| index as u32 + 1).unwrap_or(1)
}

fn inline_scripts(text: &str) -> Vec<InlineScript> {
    let mut out = Vec::new();
    let lower_src = text.to_ascii_lowercase();
    let mut search_from = 0usize;
    while let Some(rel) = lower_src[search_from..].find("<script") {
        let start = search_from + rel;
        let before = &text[..start];
        let line = before.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1;
        let after = &text[start..];
        let Some(tag_end) = after.find('>') else {
            break;
        };
        let open = &after[..=tag_end];
        let open_lower = open.to_ascii_lowercase();
        search_from = start + tag_end + 1;
        if open_lower.contains("src=") {
            continue;
        }
        let body_at = start + tag_end + 1;
        let close_rel = lower_src[body_at..].find("</script>");
        let body_end = close_rel.map(|index| body_at + index).unwrap_or(text.len());
        let body = &text[body_at..body_end];
        let body_line = line + open.bytes().filter(|byte| *byte == b'\n').count() as u32;
        if !body.trim().is_empty() {
            out.push(InlineScript {
                line: body_line,
                body: body.to_string(),
            });
        }
        if close_rel.is_none() {
            break;
        }
    }
    out
}

const OPTIONAL_END: &[&str] = &[
    "html", "head", "body", "li", "dt", "dd", "p", "rt", "rp", "optgroup", "option", "colgroup",
    "caption", "thead", "tbody", "tfoot", "tr", "td", "th",
];

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

fn balance_findings(file: &str, text: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0usize;
    let mut line = 1u32;
    let mut col = 1u32;
    let mut stack: Vec<(String, u32, u32)> = Vec::new();

    while index < chars.len() {
        if chars[index] != '<' {
            bump(&chars, &mut index, &mut line, &mut col);
            continue;
        }
        let mark_line = line;
        let mark_col = col;
        if starts_with(&chars, index, "<!--") {
            skip_until(&chars, &mut index, &mut line, &mut col, "-->");
            continue;
        }
        if starts_with_ci(&chars, index, "<!doctype") || starts_with(&chars, index, "<!") {
            skip_gt(&chars, &mut index, &mut line, &mut col);
            continue;
        }
        if chars.get(index + 1) == Some(&'/') {
            bump(&chars, &mut index, &mut line, &mut col);
            bump(&chars, &mut index, &mut line, &mut col);
            let name = read_name(&chars, &mut index, &mut line, &mut col);
            skip_gt(&chars, &mut index, &mut line, &mut col);
            close_tag(&mut stack, &name, file, mark_line, mark_col, &mut findings);
            continue;
        }
        if !chars
            .get(index + 1)
            .is_some_and(|ch| ch.is_ascii_alphabetic())
        {
            bump(&chars, &mut index, &mut line, &mut col);
            continue;
        }
        bump(&chars, &mut index, &mut line, &mut col);
        let name = read_name(&chars, &mut index, &mut line, &mut col);
        let self_close = skip_to_gt(&chars, &mut index, &mut line, &mut col);
        if VOID.contains(&name.as_str()) || self_close && VOID.contains(&name.as_str()) {
            continue;
        }
        if matches!(name.as_str(), "script" | "style" | "textarea" | "title") {
            let end = format!("</{name}");
            skip_until_ci(&chars, &mut index, &mut line, &mut col, &end);
            skip_gt(&chars, &mut index, &mut line, &mut col);
            continue;
        }
        stack.push((name, mark_line, mark_col));
    }
    for (name, open_line, open_col) in stack {
        if OPTIONAL_END.contains(&name.as_str()) {
            continue;
        }
        findings.push(html_finding(
            file,
            "html.unclosed",
            open_line,
            open_col,
            &format!("unclosed <{name}>"),
        ));
    }
    findings
}

fn close_tag(
    stack: &mut Vec<(String, u32, u32)>,
    name: &str,
    file: &str,
    line: u32,
    col: u32,
    findings: &mut Vec<Finding>,
) {
    if stack.last().is_some_and(|(open, _, _)| open == name) {
        stack.pop();
        return;
    }
    if let Some(pos) = stack.iter().rposition(|(open, _, _)| open == name) {
        while stack.len() > pos + 1 {
            if let Some((open, open_line, open_col)) = stack.pop() {
                if !OPTIONAL_END.contains(&open.as_str()) {
                    findings.push(html_finding(
                        file,
                        "html.unclosed",
                        open_line,
                        open_col,
                        &format!("unclosed <{open}>"),
                    ));
                }
            }
        }
        stack.pop();
        findings.push(html_finding(
            file,
            "html.misnested",
            line,
            col,
            &format!("misnested </{name}>"),
        ));
        return;
    }
    findings.push(html_finding(
        file,
        "html.misnested",
        line,
        col,
        &format!("misnested </{name}>"),
    ));
}

fn bump(chars: &[char], index: &mut usize, line: &mut u32, col: &mut u32) {
    if chars.get(*index) == Some(&'\n') {
        *line += 1;
        *col = 1;
    } else {
        *col += 1;
    }
    *index += 1;
}

fn starts_with(chars: &[char], index: usize, needle: &str) -> bool {
    chars[index..]
        .iter()
        .zip(needle.chars())
        .all(|(a, b)| *a == b)
        && index + needle.chars().count() <= chars.len()
}

fn starts_with_ci(chars: &[char], index: usize, needle: &str) -> bool {
    chars[index..]
        .iter()
        .zip(needle.chars())
        .all(|(a, b)| a.eq_ignore_ascii_case(&b))
        && index + needle.chars().count() <= chars.len()
}

fn read_name(chars: &[char], index: &mut usize, line: &mut u32, col: &mut u32) -> String {
    let mut name = String::new();
    while chars
        .get(*index)
        .is_some_and(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == ':')
    {
        name.push(chars[*index].to_ascii_lowercase());
        bump(chars, index, line, col);
    }
    name
}

fn skip_gt(chars: &[char], index: &mut usize, line: &mut u32, col: &mut u32) {
    while *index < chars.len() && chars[*index] != '>' {
        bump(chars, index, line, col);
    }
    if *index < chars.len() {
        bump(chars, index, line, col);
    }
}

fn skip_to_gt(chars: &[char], index: &mut usize, line: &mut u32, col: &mut u32) -> bool {
    let mut quote = None;
    let mut prev = '\0';
    while *index < chars.len() {
        let ch = chars[*index];
        if let Some(q) = quote {
            bump(chars, index, line, col);
            if ch == q {
                quote = None;
            }
            prev = ch;
            continue;
        }
        match ch {
            '"' | '\'' => {
                quote = Some(ch);
                prev = ch;
                bump(chars, index, line, col);
            }
            '>' => {
                bump(chars, index, line, col);
                return prev == '/';
            }
            _ => {
                prev = ch;
                bump(chars, index, line, col);
            }
        }
    }
    false
}

fn skip_until(chars: &[char], index: &mut usize, line: &mut u32, col: &mut u32, needle: &str) {
    while *index < chars.len() && !starts_with(chars, *index, needle) {
        bump(chars, index, line, col);
    }
    for _ in needle.chars() {
        if *index < chars.len() {
            bump(chars, index, line, col);
        }
    }
}

fn skip_until_ci(chars: &[char], index: &mut usize, line: &mut u32, col: &mut u32, needle: &str) {
    while *index < chars.len() && !starts_with_ci(chars, *index, needle) {
        bump(chars, index, line, col);
    }
}

struct Node {
    kind: NodeKind,
    name: String,
    attrs: Vec<(String, String)>,
    line: u32,
    parent: Option<usize>,
    children: Vec<usize>,
    text: String,
    template: Option<usize>,
}

enum NodeKind {
    Document,
    Element,
    Other,
}

struct Sink {
    nodes: RefCell<Vec<Node>>,
    line: Cell<u32>,
    errors: RefCell<Vec<(u32, String)>>,
    has_doctype: Cell<bool>,
    dummy: QualName,
}

#[derive(Clone)]
struct NodeHandle {
    id: usize,
    name: QualName,
}

impl Sink {
    fn new() -> Self {
        Self {
            nodes: RefCell::new(vec![Node {
                kind: NodeKind::Document,
                name: String::new(),
                attrs: Vec::new(),
                line: 1,
                parent: None,
                children: Vec::new(),
                text: String::new(),
                template: None,
            }]),
            line: Cell::new(1),
            errors: RefCell::new(Vec::new()),
            has_doctype: Cell::new(false),
            dummy: QualName::new(None, ns!(html), local_name!("html")),
        }
    }

    fn detach(&self, id: usize) {
        let mut nodes = self.nodes.borrow_mut();
        if let Some(parent) = nodes[id].parent.take() {
            nodes[parent].children.retain(|child| *child != id);
        }
    }
}

impl TreeSink for Sink {
    type Handle = NodeHandle;
    type Output = Parsed;
    type ElemName<'a> = &'a QualName;

    fn finish(self) -> Parsed {
        let nodes = self.nodes.into_inner();
        let elements = nodes
            .iter()
            .enumerate()
            .filter_map(|(id, node)| match node.kind {
                NodeKind::Element => Some(Elem {
                    name: node.name.clone(),
                    line: node.line.max(1),
                    col: 1,
                    attrs: node.attrs.clone(),
                    text: if node.name == "style" {
                        node.text.clone()
                    } else {
                        visible_text(&nodes, id)
                    },
                }),
                _ => None,
            })
            .collect();
        Parsed {
            elements,
            scripts: Vec::new(),
            has_doctype: self.has_doctype.get(),
            errors: self.errors.into_inner(),
        }
    }

    fn parse_error(&self, msg: std::borrow::Cow<'static, str>) {
        let line = self.line.get().max(1);
        self.errors.borrow_mut().push((line, msg.into_owned()));
    }

    fn get_document(&self) -> Self::Handle {
        NodeHandle {
            id: 0,
            name: self.dummy.clone(),
        }
    }

    fn elem_name<'a>(&'a self, target: &'a Self::Handle) -> &'a QualName {
        &target.name
    }

    fn create_element(
        &self,
        name: QualName,
        attrs: Vec<Attribute>,
        flags: ElementFlags,
    ) -> Self::Handle {
        let local = name.local.to_string();
        let attrs = attrs
            .into_iter()
            .map(|attr| (attr.name.local.to_string(), attr.value.to_string()))
            .collect();
        let mut nodes = self.nodes.borrow_mut();
        let id = nodes.len();
        let line = self.line.get().max(1);
        if flags.template {
            let fragment = id + 1;
            nodes.push(Node {
                kind: NodeKind::Element,
                name: local,
                attrs,
                line,
                parent: None,
                children: Vec::new(),
                text: String::new(),
                template: Some(fragment),
            });
            nodes.push(Node {
                kind: NodeKind::Document,
                name: String::new(),
                attrs: Vec::new(),
                line: 1,
                parent: None,
                children: Vec::new(),
                text: String::new(),
                template: None,
            });
            return NodeHandle {
                id,
                name: name.clone(),
            };
        }
        nodes.push(Node {
            kind: NodeKind::Element,
            name: local,
            attrs,
            line,
            parent: None,
            children: Vec::new(),
            text: String::new(),
            template: None,
        });
        NodeHandle { id, name }
    }

    fn create_comment(&self, _text: StrTendril) -> Self::Handle {
        self.push_other()
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> Self::Handle {
        self.push_other()
    }

    fn append(&self, parent: &Self::Handle, child: NodeOrText<Self::Handle>) {
        match child {
            NodeOrText::AppendText(text) => {
                let mut nodes = self.nodes.borrow_mut();
                nodes[parent.id].text.push_str(&text);
            }
            NodeOrText::AppendNode(child) => {
                self.detach(child.id);
                let mut nodes = self.nodes.borrow_mut();
                nodes[child.id].parent = Some(parent.id);
                nodes[parent.id].children.push(child.id);
            }
        }
    }

    fn append_based_on_parent_node(
        &self,
        element: &Self::Handle,
        prev_element: &Self::Handle,
        child: NodeOrText<Self::Handle>,
    ) {
        let has_parent = self.nodes.borrow()[element.id].parent.is_some();
        let parent = if has_parent { element } else { prev_element };
        self.append(parent, child);
    }

    fn append_doctype_to_document(
        &self,
        _name: StrTendril,
        _public_id: StrTendril,
        _system_id: StrTendril,
    ) {
        self.has_doctype.set(true);
    }

    fn get_template_contents(&self, target: &Self::Handle) -> Self::Handle {
        let id = self.nodes.borrow()[target.id].template.unwrap_or(target.id);
        NodeHandle {
            id,
            name: self.dummy.clone(),
        }
    }

    fn same_node(&self, x: &Self::Handle, y: &Self::Handle) -> bool {
        x.id == y.id
    }

    fn set_quirks_mode(&self, _mode: QuirksMode) {}

    fn append_before_sibling(&self, sibling: &Self::Handle, new_node: NodeOrText<Self::Handle>) {
        let NodeOrText::AppendNode(new_node) = new_node else {
            return;
        };
        self.detach(new_node.id);
        let mut nodes = self.nodes.borrow_mut();
        let Some(parent) = nodes[sibling.id].parent else {
            return;
        };
        nodes[new_node.id].parent = Some(parent);
        let children = &mut nodes[parent].children;
        if let Some(pos) = children.iter().position(|child| *child == sibling.id) {
            children.insert(pos, new_node.id);
        } else {
            children.push(new_node.id);
        }
    }

    fn add_attrs_if_missing(&self, target: &Self::Handle, attrs: Vec<Attribute>) {
        let mut nodes = self.nodes.borrow_mut();
        let existing = &mut nodes[target.id].attrs;
        for attr in attrs {
            let key = attr.name.local.to_string();
            if existing.iter().any(|(name, _)| name == &key) {
                continue;
            }
            existing.push((key, attr.value.to_string()));
        }
    }

    fn remove_from_parent(&self, target: &Self::Handle) {
        self.detach(target.id);
    }

    fn reparent_children(&self, node: &Self::Handle, new_parent: &Self::Handle) {
        let mut nodes = self.nodes.borrow_mut();
        let children = std::mem::take(&mut nodes[node.id].children);
        for child in &children {
            nodes[*child].parent = Some(new_parent.id);
        }
        nodes[new_parent.id].children.extend(children);
    }

    fn set_current_line(&self, line_number: u64) {
        self.line
            .set(u32::try_from(line_number).unwrap_or(u32::MAX));
    }
}

impl Sink {
    fn push_other(&self) -> NodeHandle {
        let mut nodes = self.nodes.borrow_mut();
        let id = nodes.len();
        nodes.push(Node {
            kind: NodeKind::Other,
            name: String::new(),
            attrs: Vec::new(),
            line: self.line.get().max(1),
            parent: None,
            children: Vec::new(),
            text: String::new(),
            template: None,
        });
        NodeHandle {
            id,
            name: self.dummy.clone(),
        }
    }
}

fn visible_text(nodes: &[Node], id: usize) -> String {
    let node = &nodes[id];
    if matches!(node.name.as_str(), "script" | "style") {
        return String::new();
    }
    let mut out = node.text.clone();
    if node.name == "img" {
        if let Some((_, alt)) = node.attrs.iter().find(|(key, _)| key == "alt") {
            out.push_str(alt);
        }
    }
    for child in &node.children {
        out.push_str(&visible_text(nodes, *child));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_complete_page_has_no_html_findings() {
        let text = "<!doctype html><html lang=\"en\"><head><meta name=\"viewport\" content=\"width=device-width\"><title>Hi</title></head><body><main><p>Hello</p></main></body></html>\n";
        let (findings, parsed) = html_findings("index.html", text);
        assert!(parsed.has_doctype);
        assert!(
            findings.is_empty(),
            "{findings:?}\nerrors={:?}",
            parsed.errors
        );
    }

    #[test]
    fn unclosed_and_misnested_tags_keep_their_lines() {
        let text = "<!doctype html>\n<html>\n<body>\n<div>\n<span></div>\n</body>\n</html>\n";
        let (findings, _) = html_findings("index.html", text);
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == "html.unclosed"
                    && finding.message.contains("<span>")
                    && finding.span.as_ref().unwrap().start_line == 5),
            "{findings:?}"
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == "html.misnested"),
            "{findings:?}"
        );
    }
}
