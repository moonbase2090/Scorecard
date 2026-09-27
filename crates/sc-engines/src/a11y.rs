// SPDX-License-Identifier: MPL-2.0
//! Static accessibility checks mapped to WCAG 2.2 success criteria.
//!
//! HTML comes from the web pack. JSX and TSX come from the node pack.
//! Nothing here opens a browser.

use sc_core::{Finding, Span};

use crate::html_doc::{parse, Elem};

pub struct Rule {
    pub id: &'static str,
    pub sc: &'static str,
}

const RULES: &[Rule] = &[
    Rule {
        id: "img-alt",
        sc: "1.1.1",
    },
    Rule {
        id: "label",
        sc: "1.3.1",
    },
    Rule {
        id: "name",
        sc: "4.1.2",
    },
    Rule {
        id: "heading-order",
        sc: "1.3.1",
    },
    Rule {
        id: "html-lang",
        sc: "3.1.1",
    },
    Rule {
        id: "document-title",
        sc: "2.4.2",
    },
    Rule {
        id: "duplicate-id",
        sc: "4.1.2",
    },
    Rule {
        id: "landmark",
        sc: "2.4.1",
    },
    Rule {
        id: "tabindex",
        sc: "2.4.3",
    },
    Rule {
        id: "autoplay",
        sc: "1.4.2",
    },
    Rule {
        id: "contrast",
        sc: "1.4.3",
    },
];

pub fn rule_sc(id: &str) -> Option<&'static str> {
    RULES.iter().find(|rule| rule.id == id).map(|rule| rule.sc)
}

#[cfg(test)]
pub fn check_html(file: &str, text: &str, disabled: &[String]) -> Vec<Finding> {
    let parsed = parse(text);
    check_elements(file, &parsed.elements, disabled, true)
}

pub fn check_jsx(file: &str, text: &str, disabled: &[String]) -> Vec<Finding> {
    let parsed = parse(&mask_expressions(text));
    check_elements(file, &parsed.elements, disabled, false)
}

pub fn check_elements(
    file: &str,
    elements: &[Elem],
    disabled: &[String],
    document: bool,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    if document {
        findings.extend(document_rules(file, elements, disabled));
    }
    let mut ids = Vec::new();
    let mut headings = Vec::new();
    for elem in elements {
        note_id(elem, &mut ids);
        note_heading(elem, &mut headings);
        findings.extend(element_rules(file, elem, elements, disabled));
    }
    findings.extend(duplicate_findings(file, &ids, disabled));
    findings.extend(heading_findings(file, &headings, disabled));
    if !rule_off(disabled, "contrast") {
        findings.extend(style_contrast(file, elements));
    }
    findings
}

fn rule_off(disabled: &[String], rule: &str) -> bool {
    disabled.iter().any(|item| item == rule)
}

fn document_rules(file: &str, elements: &[Elem], disabled: &[String]) -> Vec<Finding> {
    let mut findings = Vec::new();
    push_opt(&mut findings, lang_finding(file, elements, disabled));
    push_opt(&mut findings, title_finding(file, elements, disabled));
    push_opt(&mut findings, landmark_finding(file, elements, disabled));
    findings
}

fn lang_finding(file: &str, elements: &[Elem], disabled: &[String]) -> Option<Finding> {
    if rule_off(disabled, "html-lang") || html_has_lang(elements) {
        return None;
    }
    let line = elements
        .iter()
        .find(|elem| elem.name == "html")
        .map(|elem| elem.line)
        .unwrap_or(1);
    Some(finding(file, "html-lang", line, "html element has no lang"))
}

fn html_has_lang(elements: &[Elem]) -> bool {
    elements.iter().any(|elem| {
        elem.name == "html" && attr(elem, "lang").is_some_and(|value| !value.trim().is_empty())
    })
}

fn title_finding(file: &str, elements: &[Elem], disabled: &[String]) -> Option<Finding> {
    if rule_off(disabled, "document-title") || has_title(elements) {
        return None;
    }
    Some(finding(file, "document-title", 1, "page has no title"))
}

fn has_title(elements: &[Elem]) -> bool {
    elements
        .iter()
        .any(|elem| elem.name == "title" && !elem.text.trim().is_empty())
}

fn landmark_finding(file: &str, elements: &[Elem], disabled: &[String]) -> Option<Finding> {
    if rule_off(disabled, "landmark") || elements.iter().any(is_landmark) {
        return None;
    }
    Some(finding(
        file,
        "landmark",
        1,
        "page has no landmark (main, nav, header, footer, or aside)",
    ))
}

fn note_id<'a>(elem: &'a Elem, ids: &mut Vec<(&'a str, u32)>) {
    if let Some(id) = attr(elem, "id") {
        if !id.is_empty() {
            ids.push((id, elem.line));
        }
    }
}

fn note_heading(elem: &Elem, headings: &mut Vec<(u32, u32)>) {
    if let Some(level) = heading_level(&elem.name) {
        headings.push((level, elem.line));
    }
}

fn element_rules(file: &str, elem: &Elem, elements: &[Elem], disabled: &[String]) -> Vec<Finding> {
    let mut findings = Vec::new();
    push_opt(&mut findings, img_alt(file, elem, disabled));
    push_opt(&mut findings, label_gap(file, elem, elements, disabled));
    push_opt(&mut findings, nameless(file, elem, disabled));
    push_opt(&mut findings, tabindex_gap(file, elem, disabled));
    push_opt(&mut findings, autoplay_gap(file, elem, disabled));
    push_opt(&mut findings, inline_contrast(file, elem, disabled));
    findings
}

fn push_opt(findings: &mut Vec<Finding>, item: Option<Finding>) {
    if let Some(item) = item {
        findings.push(item);
    }
}

fn img_alt(file: &str, elem: &Elem, disabled: &[String]) -> Option<Finding> {
    if elem.name != "img" || rule_off(disabled, "img-alt") || attr(elem, "alt").is_some() {
        return None;
    }
    Some(finding(file, "img-alt", elem.line, "img has no alt"))
}

fn label_gap(file: &str, elem: &Elem, elements: &[Elem], disabled: &[String]) -> Option<Finding> {
    if !needs_label(elem) || rule_off(disabled, "label") || labeled(elem, elements) {
        return None;
    }
    Some(finding(
        file,
        "label",
        elem.line,
        "form control has no label",
    ))
}

fn nameless(file: &str, elem: &Elem, disabled: &[String]) -> Option<Finding> {
    if !is_named_control(elem) || rule_off(disabled, "name") || has_accessible_name(elem) {
        return None;
    }
    Some(finding(
        file,
        "name",
        elem.line,
        "link or button has no accessible name",
    ))
}

fn is_named_control(elem: &Elem) -> bool {
    matches!(elem.name.as_str(), "a" | "button")
}

fn has_accessible_name(elem: &Elem) -> bool {
    !elem.text.trim().is_empty()
        || named_attr(elem, "aria-label")
        || named_attr(elem, "aria-labelledby")
}

fn named_attr(elem: &Elem, key: &str) -> bool {
    attr(elem, key).is_some_and(|value| !value.trim().is_empty())
}

fn tabindex_gap(file: &str, elem: &Elem, disabled: &[String]) -> Option<Finding> {
    if rule_off(disabled, "tabindex") || !positive_tabindex(elem) {
        return None;
    }
    Some(finding(file, "tabindex", elem.line, "positive tabindex"))
}

fn positive_tabindex(elem: &Elem) -> bool {
    attr(elem, "tabindex")
        .and_then(|value| value.trim().parse::<i32>().ok())
        .is_some_and(|value| value > 0)
}

fn autoplay_gap(file: &str, elem: &Elem, disabled: &[String]) -> Option<Finding> {
    if !is_media(elem) || rule_off(disabled, "autoplay") || !bare_autoplay(elem) {
        return None;
    }
    Some(finding(
        file,
        "autoplay",
        elem.line,
        "autoplay media has no controls",
    ))
}

fn is_media(elem: &Elem) -> bool {
    matches!(elem.name.as_str(), "video" | "audio")
}

fn bare_autoplay(elem: &Elem) -> bool {
    has_attr(elem, "autoplay") && !has_attr(elem, "controls")
}

fn inline_contrast(file: &str, elem: &Elem, disabled: &[String]) -> Option<Finding> {
    if rule_off(disabled, "contrast") {
        return None;
    }
    let (fg, bg) = inline_colors(elem)?;
    if contrast(fg, bg) >= 4.5 {
        return None;
    }
    Some(finding(
        file,
        "contrast",
        elem.line,
        "inline colors are below 4.5:1",
    ))
}

fn duplicate_findings(file: &str, ids: &[(&str, u32)], disabled: &[String]) -> Vec<Finding> {
    if rule_off(disabled, "duplicate-id") {
        return Vec::new();
    }
    let mut findings = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for (id, line) in ids {
        if seen.contains(id) {
            findings.push(finding(
                file,
                "duplicate-id",
                *line,
                &format!("duplicate id `{id}`"),
            ));
        } else {
            seen.push(id);
        }
    }
    findings
}

fn heading_findings(file: &str, headings: &[(u32, u32)], disabled: &[String]) -> Vec<Finding> {
    if rule_off(disabled, "heading-order") {
        return Vec::new();
    }
    let mut findings = Vec::new();
    let mut previous = 0u32;
    for (level, line) in headings {
        if previous > 0 && *level > previous + 1 {
            findings.push(heading_skip(file, previous, *level, *line));
        }
        previous = *level;
    }
    findings
}

fn heading_skip(file: &str, previous: u32, level: u32, line: u32) -> Finding {
    finding(
        file,
        "heading-order",
        line,
        &format!("heading jumps from h{previous} to h{level}"),
    )
}

fn style_contrast(file: &str, elements: &[Elem]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for style in elements.iter().filter(|elem| elem.name == "style") {
        findings.extend(style_block(file, elements, &style.text));
    }
    findings
}

fn style_block(file: &str, elements: &[Elem], css: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (selector, body) in css_rules(css) {
        findings.extend(selector_hits(file, elements, selector, body));
    }
    findings
}

fn selector_hits(file: &str, elements: &[Elem], selector: &str, body: &str) -> Vec<Finding> {
    let Some(tag) = low_contrast_tag(selector, body) else {
        return Vec::new();
    };
    elements
        .iter()
        .filter(|elem| elem.name == tag)
        .map(|elem| finding(file, "contrast", elem.line, "styled colors are below 4.5:1"))
        .collect()
}

fn low_contrast_tag(selector: &str, body: &str) -> Option<String> {
    let (fg, bg) = colors_in(body)?;
    if contrast(fg, bg) >= 4.5 {
        return None;
    }
    let tag = selector.trim().to_ascii_lowercase();
    if tag.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        Some(tag)
    } else {
        None
    }
}

fn css_rules(text: &str) -> Vec<(&str, &str)> {
    text.split('}')
        .filter_map(|chunk| {
            let (selector, body) = chunk.split_once('{')?;
            Some((selector.trim(), body.trim()))
        })
        .collect()
}

fn finding(file: &str, rule: &str, line: u32, message: &str) -> Finding {
    let sc = rule_sc(rule).unwrap_or("");
    Finding {
        id: format!("a11y:{file}:{line}:{rule}"),
        rule: format!("a11y.{rule}"),
        engine: "a11y".into(),
        severity: "warning".into(),
        file: file.to_string(),
        span: Some(Span {
            start_line: line.max(1),
            start_col: 1,
            end_line: line.max(1),
            end_col: 2,
        }),
        symbol: None,
        message: format!("{sc} {message}"),
        evidence: serde_json::json!({"sc": sc, "rule": rule}),
        suggested_action: Some("Give the control an accessible name or fix the structure.".into()),
        disposition: String::new(),
    }
}

fn attr<'a>(elem: &'a Elem, key: &str) -> Option<&'a str> {
    elem.attrs
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(key))
        .map(|(_, value)| value.as_str())
}

fn has_attr(elem: &Elem, key: &str) -> bool {
    elem.attrs
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case(key))
}

fn heading_level(name: &str) -> Option<u32> {
    let rest = name.strip_prefix('h')?;
    let level: u32 = rest.parse().ok()?;
    (1..=6).contains(&level).then_some(level)
}

fn is_landmark(elem: &Elem) -> bool {
    matches!(
        elem.name.as_str(),
        "main" | "nav" | "header" | "footer" | "aside"
    ) || matches!(
        attr(elem, "role"),
        Some("main" | "navigation" | "banner" | "contentinfo" | "complementary")
    )
}

fn needs_label(elem: &Elem) -> bool {
    match elem.name.as_str() {
        "select" | "textarea" => true,
        "input" => !matches!(
            attr(elem, "type").unwrap_or("text"),
            "hidden" | "submit" | "button" | "reset" | "image"
        ),
        _ => false,
    }
}

fn labeled(elem: &Elem, elements: &[Elem]) -> bool {
    if attr(elem, "aria-label").is_some_and(|value| !value.trim().is_empty())
        || attr(elem, "aria-labelledby").is_some_and(|value| !value.trim().is_empty())
    {
        return true;
    }
    let Some(id) = attr(elem, "id") else {
        return false;
    };
    elements
        .iter()
        .any(|label| label.name == "label" && attr(label, "for").is_some_and(|target| target == id))
}

fn inline_colors(elem: &Elem) -> Option<([u8; 3], [u8; 3])> {
    colors_in(attr(elem, "style")?)
}

fn colors_in(style: &str) -> Option<([u8; 3], [u8; 3])> {
    let mut fg = None;
    let mut bg = None;
    for part in style.split(';') {
        note_color(part, &mut fg, &mut bg);
    }
    Some((fg?, bg?))
}

fn note_color(part: &str, fg: &mut Option<[u8; 3]>, bg: &mut Option<[u8; 3]>) {
    let Some((key, value)) = part.split_once(':') else {
        return;
    };
    let value = value.trim();
    match key.trim().to_ascii_lowercase().as_str() {
        "color" => *fg = parse_color(value),
        "background-color" | "background" => *bg = parse_color(value),
        _ => {}
    }
}

fn parse_color(value: &str) -> Option<[u8; 3]> {
    let value = value.trim().trim_matches('"').trim_matches('\'');
    if let Some(hex) = value.strip_prefix('#') {
        return parse_hex(hex);
    }
    if let Some(body) = rgb_body(value) {
        return parse_rgb(body);
    }
    named_color(value)
}

fn parse_hex(hex: &str) -> Option<[u8; 3]> {
    match hex.len() {
        3 => parse_short_hex(hex),
        6 => parse_long_hex(hex),
        _ => None,
    }
}

fn parse_short_hex(hex: &str) -> Option<[u8; 3]> {
    let chars: Vec<char> = hex.chars().collect();
    Some([
        hex_byte(&[chars[0], chars[0]])?,
        hex_byte(&[chars[1], chars[1]])?,
        hex_byte(&[chars[2], chars[2]])?,
    ])
}

fn parse_long_hex(hex: &str) -> Option<[u8; 3]> {
    Some([
        hex_byte(&hex[0..2].chars().collect::<Vec<_>>())?,
        hex_byte(&hex[2..4].chars().collect::<Vec<_>>())?,
        hex_byte(&hex[4..6].chars().collect::<Vec<_>>())?,
    ])
}

fn rgb_body(value: &str) -> Option<&str> {
    value
        .strip_prefix("rgb(")
        .and_then(|rest| rest.strip_suffix(')'))
}

fn parse_rgb(body: &str) -> Option<[u8; 3]> {
    let mut parts = body.split(',');
    Some([
        rgb_channel(parts.next())?,
        rgb_channel(parts.next())?,
        rgb_channel(parts.next())?,
    ])
}

fn rgb_channel(part: Option<&str>) -> Option<u8> {
    part?.trim().parse().ok()
}

fn named_color(value: &str) -> Option<[u8; 3]> {
    let name = value.to_ascii_lowercase();
    if let Some(rgb) = neutral_named(&name) {
        return Some(rgb);
    }
    bright_named(&name)
}

fn neutral_named(name: &str) -> Option<[u8; 3]> {
    match name {
        "black" => Some([0, 0, 0]),
        "gray" | "grey" => Some([128, 128, 128]),
        _ => None,
    }
}

fn bright_named(name: &str) -> Option<[u8; 3]> {
    match name {
        "white" => Some([255, 255, 255]),
        "red" => Some([255, 0, 0]),
        _ => None,
    }
}

fn hex_byte(chars: &[char]) -> Option<u8> {
    let text: String = chars.iter().collect();
    u8::from_str_radix(&text, 16).ok()
}

fn contrast(fg: [u8; 3], bg: [u8; 3]) -> f64 {
    let a = luminance(fg);
    let b = luminance(bg);
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

fn luminance(rgb: [u8; 3]) -> f64 {
    let channel = |value: u8| {
        let c = f64::from(value) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(rgb[0]) + 0.7152 * channel(rgb[1]) + 0.0722 * channel(rgb[2])
}

fn mask_expressions(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '{' {
            index = mask_brace(&chars, index, &mut out);
        } else {
            out.push(chars[index]);
            index += 1;
        }
    }
    out
}

fn mask_brace(chars: &[char], start: usize, out: &mut String) -> usize {
    let end = matching_brace(chars, start);
    write_masked(chars, start, end, out);
    end
}

fn matching_brace(chars: &[char], start: usize) -> usize {
    let mut index = start;
    let mut depth = 0i32;
    while index < chars.len() {
        depth += brace_delta(chars[index]);
        index += 1;
        if depth == 0 {
            break;
        }
    }
    index
}

fn brace_delta(ch: char) -> i32 {
    match ch {
        '{' => 1,
        '}' => -1,
        _ => 0,
    }
}

fn write_masked(chars: &[char], start: usize, end: usize, out: &mut String) {
    let inside = &chars[start + 1..end.saturating_sub(1)];
    if inside.contains(&'<') {
        copy_chars(out, &chars[start..end]);
        return;
    }
    out.push_str("\"x\"");
    keep_newlines(out, inside);
}

fn copy_chars(out: &mut String, chars: &[char]) {
    for ch in chars {
        out.push(*ch);
    }
}

fn keep_newlines(out: &mut String, chars: &[char]) {
    for ch in chars {
        if *ch == '\n' {
            out.push('\n');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_page_hits_the_wcag_rules() {
        let text = r#"<html>
<body>
<img src="a.png">
<input type="text">
<a href="/go"></a>
<h1>Title</h1>
<h3>Skipped</h3>
<div id="one"></div>
<div id="one"></div>
<button tabindex="2">Go</button>
<video autoplay src="a.mp4"></video>
<p style="color:#777777;background-color:#ffffff">Faint</p>
</body>
</html>
"#;
        let findings = check_html("index.html", text, &[]);
        let rules: Vec<&str> = findings
            .iter()
            .map(|finding| finding.rule.as_str())
            .collect();
        for rule in [
            "a11y.html-lang",
            "a11y.document-title",
            "a11y.landmark",
            "a11y.img-alt",
            "a11y.label",
            "a11y.name",
            "a11y.heading-order",
            "a11y.duplicate-id",
            "a11y.tabindex",
            "a11y.autoplay",
            "a11y.contrast",
        ] {
            assert!(rules.contains(&rule), "missing {rule} in {rules:?}");
        }
        assert!(findings.iter().all(|finding| finding.span.is_some()));
        assert!(findings
            .iter()
            .any(|finding| finding.evidence["sc"] == "1.1.1"));
    }

    #[test]
    fn a_labeled_page_is_clean_and_rules_can_be_disabled() {
        let text = r#"<!doctype html><html lang="en"><head><title>Hi</title></head><body>
<main>
<h1>Hi</h1>
<img src="a.png" alt="Mark">
<label for="name">Name</label><input id="name" type="text">
<a href="index.html">Home</a>
<p style="color:#222222;background-color:#ffffff">Text</p>
</main>
</body></html>"#;
        assert!(check_html("index.html", text, &[]).is_empty());
        let muted = check_html(
            "index.html",
            "<html><body><img src=\"a.png\"></body></html>",
            &[
                "img-alt".into(),
                "html-lang".into(),
                "document-title".into(),
                "landmark".into(),
            ],
        );
        assert!(muted.iter().all(|finding| finding.rule != "a11y.img-alt"));
    }

    #[test]
    fn jsx_img_without_alt_is_flagged() {
        let findings = check_jsx(
            "Widget.jsx",
            include_str!("../../../testdata/a11y_jsx/Widget.jsx"),
            &[],
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == "a11y.img-alt"),
            "{findings:?}"
        );
        assert!(findings
            .iter()
            .all(|finding| finding.rule != "a11y.html-lang"));
    }

    #[test]
    fn source_stays_within_the_uncovered_crap_bar() {
        let facts = sc_graph::inspect_source(include_str!("a11y.rs"), "a11y.rs").unwrap();
        for function in facts.functions {
            assert!(
                function.cc <= 5,
                "{} has CC {}, which fails CRAP 30 at zero coverage",
                function.symbol,
                function.cc
            );
        }
        let report =
            sc_graph::inspect_source(include_str!("../../sc-cli/src/report.rs"), "report.rs")
                .unwrap();
        for function in report.functions {
            if !function.symbol.contains("a11y") {
                continue;
            }
            assert!(
                function.cc <= 5,
                "{} has CC {}, which fails CRAP 30 at zero coverage",
                function.symbol,
                function.cc
            );
        }
    }

    #[test]
    fn contrast_ratio_matches_black_on_white() {
        let ratio = contrast([0, 0, 0], [255, 255, 255]);
        assert!((ratio - 21.0).abs() < 0.1, "{ratio}");
    }
}
