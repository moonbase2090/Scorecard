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
    if document && !disabled.iter().any(|rule| rule == "html-lang") {
        let lang = elements.iter().find(|elem| elem.name == "html");
        let ok = lang
            .is_some_and(|elem| attr(elem, "lang").is_some_and(|value| !value.trim().is_empty()));
        if !ok {
            let line = lang.map(|elem| elem.line).unwrap_or(1);
            findings.push(finding(file, "html-lang", line, "html element has no lang"));
        }
    }
    if document && !disabled.iter().any(|rule| rule == "document-title") {
        let titled = elements
            .iter()
            .any(|elem| elem.name == "title" && !elem.text.trim().is_empty());
        if !titled {
            findings.push(finding(file, "document-title", 1, "page has no title"));
        }
    }
    if document
        && !disabled.iter().any(|rule| rule == "landmark")
        && !elements.iter().any(is_landmark)
    {
        findings.push(finding(
            file,
            "landmark",
            1,
            "page has no landmark (main, nav, header, footer, or aside)",
        ));
    }
    let mut ids: Vec<(&str, u32)> = Vec::new();
    let mut headings: Vec<(u32, u32)> = Vec::new();
    for elem in elements {
        if let Some(id) = attr(elem, "id") {
            if !id.is_empty() {
                ids.push((id, elem.line));
            }
        }
        if let Some(level) = heading_level(&elem.name) {
            headings.push((level, elem.line));
        }
        if elem.name == "img"
            && !disabled.iter().any(|rule| rule == "img-alt")
            && attr(elem, "alt").is_none()
        {
            findings.push(finding(file, "img-alt", elem.line, "img has no alt"));
        }
        if needs_label(elem)
            && !disabled.iter().any(|rule| rule == "label")
            && !labeled(elem, elements)
        {
            findings.push(finding(
                file,
                "label",
                elem.line,
                "form control has no label",
            ));
        }
        if matches!(elem.name.as_str(), "a" | "button")
            && !disabled.iter().any(|rule| rule == "name")
            && elem.text.trim().is_empty()
            && attr(elem, "aria-label").is_none()
            && attr(elem, "aria-labelledby").is_none()
        {
            findings.push(finding(
                file,
                "name",
                elem.line,
                "link or button has no accessible name",
            ));
        }
        if !disabled.iter().any(|rule| rule == "tabindex") {
            if let Some(value) = attr(elem, "tabindex") {
                if value.trim().parse::<i32>().unwrap_or(0) > 0 {
                    findings.push(finding(file, "tabindex", elem.line, "positive tabindex"));
                }
            }
        }
        if matches!(elem.name.as_str(), "video" | "audio")
            && !disabled.iter().any(|rule| rule == "autoplay")
            && has_attr(elem, "autoplay")
            && !has_attr(elem, "controls")
        {
            findings.push(finding(
                file,
                "autoplay",
                elem.line,
                "autoplay media has no controls",
            ));
        }
        if !disabled.iter().any(|rule| rule == "contrast") {
            if let Some((fg, bg)) = inline_colors(elem) {
                if contrast(fg, bg) < 4.5 {
                    findings.push(finding(
                        file,
                        "contrast",
                        elem.line,
                        "inline colors are below 4.5:1",
                    ));
                }
            }
        }
    }
    if !disabled.iter().any(|rule| rule == "duplicate-id") {
        let mut seen: Vec<&str> = Vec::new();
        for (id, line) in ids {
            if seen.contains(&id) {
                findings.push(finding(
                    file,
                    "duplicate-id",
                    line,
                    &format!("duplicate id `{id}`"),
                ));
            } else {
                seen.push(id);
            }
        }
    }
    if !disabled.iter().any(|rule| rule == "heading-order") {
        let mut previous = 0u32;
        for (level, line) in headings {
            if previous > 0 && level > previous + 1 {
                findings.push(finding(
                    file,
                    "heading-order",
                    line,
                    &format!("heading jumps from h{previous} to h{level}"),
                ));
            }
            previous = level;
        }
    }
    if !disabled.iter().any(|rule| rule == "contrast") {
        findings.extend(style_contrast(file, elements));
    }
    findings
}

fn style_contrast(file: &str, elements: &[Elem]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for style in elements.iter().filter(|elem| elem.name == "style") {
        for (selector, body) in css_rules(&style.text) {
            let Some((fg, bg)) = colors_in(body) else {
                continue;
            };
            if contrast(fg, bg) >= 4.5 {
                continue;
            }
            let tag = selector.trim().to_ascii_lowercase();
            if !tag.chars().all(|ch| ch.is_ascii_alphanumeric()) {
                continue;
            }
            for elem in elements.iter().filter(|elem| elem.name == tag) {
                findings.push(finding(
                    file,
                    "contrast",
                    elem.line,
                    "styled colors are below 4.5:1",
                ));
            }
        }
    }
    findings
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
        let Some((key, value)) = part.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        if key == "color" {
            fg = parse_color(value);
        } else if key == "background-color" || key == "background" {
            bg = parse_color(value);
        }
    }
    Some((fg?, bg?))
}

fn parse_color(value: &str) -> Option<[u8; 3]> {
    let value = value.trim().trim_matches('"').trim_matches('\'');
    if let Some(hex) = value.strip_prefix('#') {
        return match hex.len() {
            3 => {
                let chars: Vec<char> = hex.chars().collect();
                Some([
                    hex_byte(&[chars[0], chars[0]])?,
                    hex_byte(&[chars[1], chars[1]])?,
                    hex_byte(&[chars[2], chars[2]])?,
                ])
            }
            6 => Some([
                hex_byte(&hex[0..2].chars().collect::<Vec<_>>())?,
                hex_byte(&hex[2..4].chars().collect::<Vec<_>>())?,
                hex_byte(&hex[4..6].chars().collect::<Vec<_>>())?,
            ]),
            _ => None,
        };
    }
    if let Some(rest) = value
        .strip_prefix("rgb(")
        .and_then(|rest| rest.strip_suffix(')'))
    {
        let mut parts = rest.split(',');
        return Some([
            parts.next()?.trim().parse().ok()?,
            parts.next()?.trim().parse().ok()?,
            parts.next()?.trim().parse().ok()?,
        ]);
    }
    Some(match value.to_ascii_lowercase().as_str() {
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        "red" => [255, 0, 0],
        "gray" | "grey" => [128, 128, 128],
        _ => return None,
    })
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
            let start = index;
            let mut depth = 0;
            while index < chars.len() {
                match chars[index] {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        index += 1;
                        if depth == 0 {
                            break;
                        }
                        continue;
                    }
                    _ => {}
                }
                index += 1;
            }
            let inside = &chars[start + 1..index.saturating_sub(1)];
            if inside.contains(&'<') {
                for ch in &chars[start..index] {
                    out.push(*ch);
                }
            } else {
                out.push_str("\"x\"");
                for ch in inside {
                    if *ch == '\n' {
                        out.push('\n');
                    }
                }
            }
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
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
    fn contrast_ratio_matches_black_on_white() {
        let ratio = contrast([0, 0, 0], [255, 255, 255]);
        assert!((ratio - 21.0).abs() < 0.1, "{ratio}");
    }
}
