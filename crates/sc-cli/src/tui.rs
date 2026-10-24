// SPDX-License-Identifier: MIT
//! Read-only browser for the last completed analysis.

use std::io::{self, IsTerminal};
use std::path::Path;

use lunatui_core::{ClusterTable, Grid, Rect, Style};
use lunatui_runtime::{is_quit, App, ControlFlow, RunOptions};
use lunatui_term::{Event, KeyCode, KeyKind};
use lunatui_theme::{Theme, Token, Variant};
use lunatui_widgets::{Block, RenderCx, StatusBar, Tabs, Widget};
use sc_core::Scorecard;

pub(super) fn run(project: &Path) -> i32 {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        eprintln!("sc tui requires an interactive terminal. For redirected output, run: sc analyze --format json");
        return 2;
    }
    let path = project.join(".sc/last-scorecard.json");
    let card = match std::fs::read(&path)
        .map_err(|e| e.to_string())
        .and_then(|bytes| serde_json::from_slice::<Scorecard>(&bytes).map_err(|e| e.to_string()))
    {
        Ok(card) => card,
        Err(error) => {
            eprintln!("Cannot open saved scorecard {}: {error}. Run sc analyze in the project directory, then retry sc tui.", path.display());
            return 2;
        }
    };
    let (width, height) = terminal_size::terminal_size()
        .map(|(w, h)| (w.0, h.0))
        .unwrap_or((80, 24));
    match lunatui_runtime::run(Browser::new(card), RunOptions::new(width, height)) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("Cannot run sc tui: {error}. Retry in an interactive terminal, or run sc analyze --format json.");
            2
        }
    }
}

struct Browser {
    card: Scorecard,
    gate: usize,
    tab: usize,
    theme: Theme,
}

impl Browser {
    fn new(card: Scorecard) -> Self {
        Self {
            card,
            gate: 0,
            tab: 0,
            theme: Theme::builtin(Variant::Dark),
        }
    }

    fn body(&self) -> Vec<String> {
        match self.tab {
            0 => {
                let mut lines = vec![
                    "Saved analysis".into(),
                    "Run sc analyze to refresh this report.".into(),
                    String::new(),
                ];
                if let Some(gate) = self.card.measures.gates.get(self.gate) {
                    lines.push(format!("Selected gate: {}", gate.id));
                    lines.push(format!("Result: {}", gate_state(gate)));
                    lines.push(format!(
                        "Enforced: {}",
                        if gate.enforced { "yes" } else { "no" }
                    ));
                    if let Some(reason) = &gate.reason {
                        lines.push(reason.clone());
                    }
                } else {
                    lines.push("No gates in this report".into());
                }
                lines
            }
            1 => vec![if self.card.sections.findings.is_empty() {
                "No findings in this report".into()
            } else {
                format!(
                    "Recorded findings: {}. Use sc analyze --format pretty to read them.",
                    self.card.sections.findings.len()
                )
            }],
            _ => vec![
                format!("Ran: {}", list(&self.card.engines.engines_run)),
                format!("Skipped: {}", list(&self.card.engines.engines_skipped)),
            ],
        }
    }
}

fn line(text: impl AsRef<str>) -> StatusBar {
    StatusBar::new(
        text.as_ref()
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect::<String>(),
    )
}

fn list(items: &[String]) -> String {
    if items.is_empty() {
        "none".into()
    } else {
        items.join(", ")
    }
}

fn gate_state(gate: &sc_core::Gate) -> &'static str {
    match (gate.pass, gate.enforced) {
        (true, _) => "PASS",
        (false, true) => "FAIL",
        (false, false) => "REPORT",
    }
}

impl App for Browser {
    fn update(&mut self, event: &Event) -> ControlFlow {
        let Event::Key(key) = event else {
            return ControlFlow::Continue;
        };
        if key.kind == KeyKind::Release {
            return ControlFlow::Continue;
        }
        if is_quit(event) || (key.code == KeyCode::Char('c') && key.modifiers.is_control()) {
            return ControlFlow::Break;
        }
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.gate = self
                    .gate
                    .saturating_add(1)
                    .min(self.card.measures.gates.len().saturating_sub(1));
            }
            KeyCode::Up | KeyCode::Char('k') => self.gate = self.gate.saturating_sub(1),
            KeyCode::Tab if key.modifiers.is_shift() => self.tab = (self.tab + 2) % 3,
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => self.tab = (self.tab + 1) % 3,
            KeyCode::Left | KeyCode::Char('h') => self.tab = (self.tab + 2) % 3,
            KeyCode::Home => self.gate = 0,
            KeyCode::End => self.gate = self.card.measures.gates.len().saturating_sub(1),
            _ => {}
        }
        ControlFlow::Continue
    }

    fn render(&mut self, grid: &mut Grid, area: Rect, clusters: &mut ClusterTable) {
        let mut cx = RenderCx::new(grid, clusters, &self.theme);
        let base = Style::new()
            .fg(cx.token(Token::TextPrimary))
            .bg(cx.token(Token::Surface0));
        let accent = base
            .fg(cx.token(Token::Accent))
            .bg(cx.token(Token::Surface1));
        cx.set_style(area, base);
        if area.width < 60 || area.height < 12 {
            line("Open sc tui in a terminal at least 60x12")
                .style(base)
                .render(area, &mut cx);
            return;
        }
        let rail_width = 24.min(area.width / 3);
        let rail = Rect::new(area.x, area.y, rail_width, area.height - 1);
        let panel = Rect::new(
            area.x + rail_width,
            area.y,
            area.width - rail_width,
            area.height - 1,
        );
        let rail_block = Block::bordered().title("Gates");
        rail_block.render(rail, &mut cx);
        let inner = rail_block.inner(rail);
        let visible = usize::from(inner.height);
        let offset = self.gate.saturating_sub(visible.saturating_sub(1));
        for (row, gate) in self
            .card
            .measures
            .gates
            .iter()
            .skip(offset)
            .take(visible)
            .enumerate()
        {
            let label = format!(
                "{} {} {}",
                if offset + row == self.gate { ">" } else { " " },
                gate_state(gate),
                gate.id
            );
            line(label)
                .style(if offset + row == self.gate {
                    accent
                } else {
                    base
                })
                .render(
                    Rect::new(inner.x, inner.y + row as u16, inner.width, 1),
                    &mut cx,
                );
        }
        let block = Block::bordered().title("Scorecard");
        block.render(panel, &mut cx);
        let content = block.inner(panel);
        Tabs::new([
            "Overview".into(),
            format!("Findings [{}]", self.card.sections.findings.len()),
            format!("Engines [{}]", self.card.engines.engines_run.len()),
        ])
        .select(self.tab)
        .style(base)
        .highlight_style(accent)
        .render(Rect::new(content.x, content.y, content.width, 1), &mut cx);
        for (row, body_line) in self
            .body()
            .into_iter()
            .take(usize::from(content.height.saturating_sub(2)))
            .enumerate()
        {
            line(body_line).style(base).render(
                Rect::new(content.x, content.y + 2 + row as u16, content.width, 1),
                &mut cx,
            );
        }
        let status = format!(
            "{} | {} gates | j/k gates | Tab tabs | q quit | saved report",
            self.card.verdict.to_uppercase(),
            self.card.measures.gates.len()
        );
        line(status).style(accent).render(
            Rect::new(area.x, area.y + area.height - 1, area.width, 1),
            &mut cx,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lunatui_runtime::render_once;
    use lunatui_term::KeyEvent;

    #[test]
    fn navigation_renders_saved_gate_results_and_advisory_status() {
        let mut card = Scorecard::skeleton("demo", 30);
        card.measures.gates = vec![
            sc_core::Gate {
                id: "types".into(),
                pass: true,
                enforced: true,
                reason: None,
            },
            sc_core::Gate {
                id: "sca".into(),
                pass: false,
                enforced: false,
                reason: Some("Dependency warning".into()),
            },
        ];
        let mut browser = Browser::new(card);
        let (grid, clusters) = render_once(&mut browser, 80, 24);
        assert!(grid
            .display_text(&clusters)
            .contains("Selected gate: types"));
        browser.update(&Event::Key(KeyEvent::press(KeyCode::Down)));
        let (grid, clusters) = render_once(&mut browser, 80, 24);
        let text = grid.display_text(&clusters);
        assert!(text.contains("Selected gate: sca"));
        assert!(text.contains("Result: REPORT"));
        assert!(text.contains("Enforced: no"));
        assert!(text.contains("Dependency warning"));
        assert_eq!(
            grid.get(1, 2).unwrap().style().fg,
            lunatui_core::Color::Rgb(0x9e, 0xce, 0x6a)
        );
        browser.update(&Event::Key(KeyEvent::press(KeyCode::Tab)));
        let (grid, clusters) = render_once(&mut browser, 80, 24);
        assert!(grid
            .display_text(&clusters)
            .contains("No findings in this report"));
        browser.update(&Event::Key(KeyEvent::press(KeyCode::Tab)));
        let (grid, clusters) = render_once(&mut browser, 80, 24);
        assert!(grid.display_text(&clusters).contains("Ran: none"));
        browser.update(&Event::Key(KeyEvent::press(KeyCode::Right)));
        browser.update(&Event::Key(KeyEvent::press(KeyCode::Up)));
        let (grid, clusters) = render_once(&mut browser, 80, 24);
        assert!(grid
            .display_text(&clusters)
            .contains("Selected gate: types"));
    }

    #[test]
    fn small_layout_renders_recovery_hint() {
        let mut browser = Browser::new(Scorecard::skeleton("demo", 30));
        let (grid, clusters) = render_once(&mut browser, 50, 10);
        assert!(grid.display_text(&clusters).contains("at least 60x12"));
    }
}
