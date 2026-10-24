// SPDX-License-Identifier: MIT
//! Terminal links and bounded output for saved reports.
use std::path::Path;

use lunatui_core::{clip_display_prefix, Color, Style};
use sc_core::{Finding, Scorecard};

use super::{clean, gate_state, location_label};

pub(super) struct Link {
    pub x: u16,
    pub y: u16,
    pub label: String,
    pub uri: String,
    pub style: Style,
}

impl Link {
    pub fn append_to(&self, bytes: &mut Vec<u8>) {
        self.append(bytes, true);
    }

    pub fn append_plain_to(&self, bytes: &mut Vec<u8>) {
        self.append(bytes, false);
    }

    fn append(&self, bytes: &mut Vec<u8>, hyperlink: bool) {
        let style = anstyle::Style::new()
            .fg_color(ansi_color(self.style.fg))
            .bg_color(ansi_color(self.style.bg))
            .underline();
        bytes.extend_from_slice(format!("\x1b[{};{}H{style}", self.y + 1, self.x + 1).as_bytes());
        if hyperlink {
            bytes.extend_from_slice(linked(&self.uri, &self.label).as_bytes());
        } else {
            bytes.extend_from_slice(self.label.as_bytes());
        }
        bytes.extend_from_slice(b"\x1b[0m");
    }
}

fn ansi_color(color: Color) -> Option<anstyle::Color> {
    match color {
        Color::Rgb(r, g, b) => Some(anstyle::RgbColor(r, g, b).into()),
        Color::Indexed(index) => Some(anstyle::Ansi256Color(index).into()),
        Color::Default | Color::Token(_) => None,
    }
}

fn linked(uri: &str, label: &str) -> String {
    format!("\x1b]8;;{uri}\x1b\\{label}\x1b]8;;\x1b\\")
}

pub(super) fn file_link(root: &Path, finding: &Finding) -> Option<String> {
    if finding.file.is_empty() || finding.file.chars().any(char::is_control) {
        return None;
    }
    let path = root.join(&finding.file).canonicalize().ok()?;
    if !path.starts_with(root) || !path.is_file() {
        return None;
    }
    let mut uri = url::Url::from_file_path(path).ok()?;
    if let Some(span) = finding.span.as_ref().filter(|s| s.start_line > 0) {
        uri.set_fragment(Some(&format!("L{}", span.start_line)));
    }
    Some(uri.into())
}

pub(super) fn inline_summary(card: &Scorecard, root: &Path, links: bool) -> String {
    let mut text = format!(
        "Scorecard {} | {} gates | {} findings\n",
        clean(&card.verdict).to_uppercase(),
        card.measures.gates.len(),
        card.sections.findings.len()
    );
    for gate in card.measures.gates.iter().take(8) {
        text.push_str(&format!(
            "{} {}\n",
            gate_state(gate),
            clip_display_prefix(&clean(&gate.id), 90)
        ));
    }
    if card.measures.gates.len() > 8 {
        text.push_str(&format!(
            "... {} more gates\n",
            card.measures.gates.len() - 8
        ));
    }
    for finding in card.sections.findings.iter().take(5) {
        let location = clean(&location_label(finding));
        let location = clip_display_prefix(&location, 60);
        let location = if links {
            file_link(root, finding)
                .map(|uri| linked(&uri, location))
                .unwrap_or_else(|| location.to_string())
        } else {
            location.to_string()
        };
        text.push_str(&format!(
            "{} {} {}\n  {}\n",
            clip_display_prefix(&clean(&finding.severity), 10),
            location,
            clip_display_prefix(&clean(&finding.rule), 40),
            clip_display_prefix(&clean(&finding.message), 100)
        ));
    }
    if card.sections.findings.len() > 5 {
        text.push_str(&format!(
            "... {} more findings; open sc tui to browse\n",
            card.sections.findings.len() - 5
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_gate_snapshot_keeps_advisory_status_and_limits_rows() {
        let mut card = Scorecard::skeleton("demo", 30);
        card.measures.gates = (0..9)
            .map(|index| sc_core::Gate {
                id: format!("gate-{index}"),
                pass: index == 0,
                enforced: index != 2,
                reason: None,
            })
            .collect();
        assert_eq!(inline_summary(&card, Path::new("/demo"), false),
            "Scorecard FAIL | 9 gates | 0 findings\nPASS gate-0\nFAIL gate-1\nREPORT gate-2\nFAIL gate-3\nFAIL gate-4\nFAIL gate-5\nFAIL gate-6\nFAIL gate-7\n... 1 more gates\n");
    }
}
