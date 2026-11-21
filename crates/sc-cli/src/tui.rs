// SPDX-License-Identifier: MIT
//! Read-only browser for the last completed analysis.

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use lunatui_core::{clip_display_prefix, ClusterTable, Grid, Rect, Size, Style, UnderlineStyle};
use lunatui_runtime::{is_quit, App, ControlFlow, Input};
use lunatui_term::{ColorMode, Encoder, Event, KeyCode, KeyKind, Session};
use lunatui_theme::{Theme, Token, Variant};
use lunatui_widgets::{Block, Paragraph, RenderCx, StatusBar, Tabs, TextWrap, Widget};
use sc_core::{Finding, Scorecard};

mod output;
use output::{file_link, inline_summary, Link};

pub(super) fn run(project: &Path, inline: bool) -> i32 {
    if !inline && (!io::stdin().is_terminal() || !io::stdout().is_terminal()) {
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
            eprintln!("Cannot open saved scorecard {}: {error}. Run sc analyze in the project directory, then retry sc tui.", clean(&path.display().to_string()));
            return 2;
        }
    };
    let root = match project.canonicalize() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("Cannot resolve project directory: {error}. Retry sc tui with an existing project path.");
            return 2;
        }
    };
    if inline {
        let stdout = io::stdout();
        let links = stdout.is_terminal() && std::env::var_os("TERM").is_some_and(|t| t != "dumb");
        return match stdout
            .lock()
            .write_all(inline_summary(&card, &root, links).as_bytes())
        {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("Cannot write saved summary: {error}. Retry sc tui --inline with a writable output.");
                2
            }
        };
    }
    let (width, height) = terminal_size::terminal_size()
        .map(|(w, h)| (w.0, h.0))
        .unwrap_or((80, 24));
    match browse(Browser::with_root(card, root), width, height) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("Cannot run sc tui: {error}. Retry in an interactive terminal, or run sc analyze --format json.");
            2
        }
    }
}

fn browse(mut browser: Browser, width: u16, height: u16) -> io::Result<()> {
    let mut input = Input::open()?;
    let mut session = Session::enter(io::stdout())?;
    let mut frame = Frame::new(width, height);
    loop {
        frame.present(&mut browser, &mut session)?;
        let Some(events) = input.next_frame()? else {
            return Ok(());
        };
        for event in events {
            if let Event::Resize(width, height) = event {
                frame.resize(width, height);
            }
            if browser.update(&event) == ControlFlow::Break {
                return Ok(());
            }
        }
    }
}

struct Frame {
    previous: Grid,
    encoder: Encoder,
    previous_links: Vec<Link>,
    clusters: ClusterTable,
    repaint: bool,
}

impl Frame {
    fn new(width: u16, height: u16) -> Self {
        Self {
            previous: Grid::new(width, height),
            encoder: Encoder::new(),
            previous_links: Vec::new(),
            clusters: ClusterTable::new(),
            repaint: false,
        }
    }

    fn resize(&mut self, width: u16, height: u16) {
        if width != 0
            && height != 0
            && (width, height) != (self.previous.width(), self.previous.height())
        {
            self.previous = Grid::new(width, height);
            self.previous_links.clear();
            self.repaint = true;
        }
    }

    fn present<W: Write>(
        &mut self,
        browser: &mut Browser,
        session: &mut Session<W>,
    ) -> io::Result<()> {
        let width = self.previous.width();
        let height = self.previous.height();
        let area = Rect::new(0, 0, width, height);
        let mut grid = Grid::new(width, height);
        browser.render(&mut grid, area, &mut self.clusters);
        let mut bytes = Vec::new();
        for link in &self.previous_links {
            link.append_plain_to(&mut bytes);
        }
        let contents = if self.repaint {
            self.encoder
                .encode_full(&grid, &self.clusters, ColorMode::TrueColor)
        } else {
            self.encoder
                .encode_diff(&self.previous, &grid, &self.clusters, ColorMode::TrueColor)
        };
        bytes.extend_from_slice(contents);
        for link in &browser.links {
            link.append_to(&mut bytes);
        }
        session.present_frame(&bytes)?;
        self.previous = grid;
        self.previous_links = std::mem::take(&mut browser.links);
        self.repaint = false;
        Ok(())
    }
}

struct Browser {
    card: Scorecard,
    gate: usize,
    tab: usize,
    theme: Theme,
    finding: usize,
    page_size: usize,
    detail: Option<String>,
    body_scroll: u16,
    body_height: u16,
    root: PathBuf,
    links: Vec<Link>,
}

impl Browser {
    #[cfg(test)]
    fn new(card: Scorecard) -> Self {
        Self::with_root(card, PathBuf::from("/demo"))
    }

    fn with_root(card: Scorecard, root: PathBuf) -> Self {
        Self {
            card,
            gate: 0,
            finding: 0,
            page_size: 1,
            detail: None,
            body_scroll: 0,
            body_height: 1,
            root,
            links: Vec::new(),
            tab: 0,
            theme: Theme::builtin(Variant::Dark),
        }
    }

    fn verdict(&self) -> String {
        if self.card.analyzer_error {
            "ERROR".into()
        } else {
            clean(&self.card.verdict).to_uppercase()
        }
    }

    fn tests_skipped(&self) -> bool {
        self.card
            .engines
            .engines_skipped
            .iter()
            .any(|e| e == "tests")
    }

    fn tests_ran(&self) -> bool {
        self.card.engines.engines_run.iter().any(|e| e == "tests")
    }

    fn gate_state(&self, gate: &sc_core::Gate) -> &'static str {
        if gate.id == "tests"
            && (gate.pass || !gate.enforced)
            && self.tests_skipped()
            && !self.tests_ran()
        {
            return "SKIP";
        }
        gate_state(gate)
    }

    fn body(&self) -> String {
        match self.tab {
            0 => {
                let mut lines = vec![format!("Saved analysis | {}", self.verdict())];
                if self.card.analyzer_error {
                    lines.push("Analysis could not finish. See Findings for details.".into());
                }
                if self.tests_skipped() {
                    let message = if self.tests_ran() {
                        "Tests were skipped in some packs. See the tests gate."
                    } else {
                        "Tests were skipped. See the tests gate for the reason."
                    };
                    lines.push(message.into());
                }
                lines.push(String::new());
                if let Some(gate) = self.card.measures.gates.get(self.gate) {
                    lines.push(format!("Selected gate: {}", clean(&gate.id)));
                    lines.push(format!("Result: {}", self.gate_state(gate)));
                    lines.push(format!(
                        "Enforced: {}",
                        if gate.enforced { "yes" } else { "no" }
                    ));
                    if let Some(reason) = &gate.reason {
                        lines.push(clean(reason));
                    }
                } else {
                    lines.push("No gates in this report".into());
                }
                lines.join("\n")
            }
            _ => format!(
                "Ran: {}\nSkipped: {}",
                list(&self.card.engines.engines_run),
                list(&self.card.engines.engines_skipped)
            ),
        }
    }

    fn update_detail(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => {
                self.detail = None;
                self.body_scroll = 0;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.body_scroll = self.body_scroll.saturating_add(1)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.body_scroll = self.body_scroll.saturating_sub(1)
            }
            KeyCode::PageDown => {
                self.body_scroll = self.body_scroll.saturating_add(self.body_height)
            }
            KeyCode::PageUp => self.body_scroll = self.body_scroll.saturating_sub(self.body_height),
            KeyCode::Home => self.body_scroll = 0,
            KeyCode::End => self.body_scroll = u16::MAX,
            _ => {}
        }
    }
}

fn line(text: impl AsRef<str>) -> StatusBar {
    StatusBar::new(clean(text.as_ref()))
}

fn list(items: &[String]) -> String {
    if items.is_empty() {
        "none".into()
    } else {
        clean(&items.join(", "))
    }
}

fn gate_state(gate: &sc_core::Gate) -> &'static str {
    match (gate.pass, gate.enforced) {
        (true, _) => "PASS",
        (false, true) => "FAIL",
        (false, false) => "REPORT",
    }
}

fn finding_detail(finding: &Finding) -> String {
    let mut rows = vec![
        format!("{} | {}", clean(&finding.rule), clean(&finding.severity)),
        format!("Location: {}", location_label(finding)),
    ];
    if let Some(symbol) = &finding.symbol {
        rows.push(format!("Symbol: {}", clean(symbol)));
    }
    rows.extend([String::new(), clean_multiline(&finding.message)]);
    if let Some(action) = &finding.suggested_action {
        rows.push(format!("\nNext step\n{}", clean_multiline(action)));
    }
    if !finding.evidence.is_null() && finding.evidence.as_object().is_none_or(|e| !e.is_empty()) {
        let evidence = match finding.evidence.as_str() {
            Some(text) => clean_multiline(text),
            None => serde_json::to_string_pretty(&finding.evidence)
                .unwrap_or_else(|_| finding.evidence.to_string()),
        };
        rows.push(format!("\nEvidence\n{evidence}"));
    }
    rows.join("\n")
}

fn clean_multiline(text: &str) -> String {
    text.lines().map(clean).collect::<Vec<_>>().join("\n")
}

fn render_body(
    text: impl Into<String>,
    scroll: &mut u16,
    area: Rect,
    style: Style,
    cx: &mut RenderCx<'_>,
) {
    let paragraph = Paragraph::new(text)
        .wrap(TextWrap { trim: true })
        .style(style);
    let rows = paragraph.measure(Size::new(area.width, u16::MAX)).height;
    *scroll = (*scroll).min(rows.saturating_sub(area.height));
    cx.set_style(
        Rect::new(
            area.x,
            area.y,
            area.width,
            rows.saturating_sub(*scroll).min(area.height),
        ),
        style,
    );
    paragraph.scroll((*scroll, 0)).render(area, cx);
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
        if self.detail.is_some() {
            self.update_detail(key.code);
            return ControlFlow::Continue;
        }
        let previous_tab = self.tab;
        let (selected, len) = if self.tab == 1 {
            (&mut self.finding, self.card.sections.findings.len())
        } else {
            (&mut self.gate, self.card.measures.gates.len())
        };
        let previous_selection = *selected;
        let last = len.saturating_sub(1);
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => *selected = selected.saturating_add(1).min(last),
            KeyCode::Up | KeyCode::Char('k') => *selected = selected.saturating_sub(1),
            KeyCode::PageDown if self.tab == 1 => {
                *selected = selected.saturating_add(self.page_size).min(last)
            }
            KeyCode::PageUp if self.tab == 1 => *selected = selected.saturating_sub(self.page_size),
            KeyCode::PageDown => {
                self.body_scroll = self.body_scroll.saturating_add(self.body_height)
            }
            KeyCode::PageUp => self.body_scroll = self.body_scroll.saturating_sub(self.body_height),
            KeyCode::Home => *selected = 0,
            KeyCode::End => *selected = last,
            KeyCode::Enter if self.tab == 1 => {
                self.detail = self
                    .card
                    .sections
                    .findings
                    .get(*selected)
                    .map(finding_detail);
            }
            KeyCode::Tab if key.modifiers.is_shift() => self.tab = (self.tab + 2) % 3,
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => self.tab = (self.tab + 1) % 3,
            KeyCode::Left | KeyCode::Char('h') => self.tab = (self.tab + 2) % 3,
            _ => {}
        }
        if self.tab != previous_tab || *selected != previous_selection {
            self.body_scroll = 0;
        }
        ControlFlow::Continue
    }

    fn render(&mut self, grid: &mut Grid, area: Rect, clusters: &mut ClusterTable) {
        self.links.clear();
        let mut cx = RenderCx::new(grid, clusters, &self.theme);
        let base = Style::new()
            .fg(cx.token(Token::TextPrimary))
            .bg(cx.token(Token::Surface0));
        let accent = base
            .fg(cx.token(Token::Accent))
            .bg(cx.token(Token::Surface1));
        if area.width < 60 || area.height < 12 {
            cx.set_style(area, base);
            line("Open sc tui in a terminal at least 60x12")
                .style(base)
                .render(area, &mut cx);
            return;
        }
        let footer = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
        cx.set_style(footer, base);
        if let Some(detail) = &self.detail {
            let block = Block::bordered()
                .title("Finding detail")
                .style(base)
                .border_style(base)
                .title_style(accent);
            let panel = Rect::new(area.x, area.y, area.width, area.height - 1);
            block.render(panel, &mut cx);
            let body = block.inner(panel);
            self.body_height = body.height;
            render_body(detail.as_str(), &mut self.body_scroll, body, base, &mut cx);
            line("Esc back | j/k scroll | PgUp/PgDn | q quit")
                .style(accent)
                .render(footer, &mut cx);
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
                self.gate_state(gate),
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
        let body_area = Rect::new(
            content.x,
            content.y + 2,
            content.width,
            content.height.saturating_sub(2),
        );
        self.body_height = body_area.height;
        if self.tab == 1 {
            self.page_size = usize::from(body_area.height.saturating_sub(3)).max(1);
            render_findings(
                &self.card.sections.findings,
                self.finding,
                &self.root,
                body_area,
                &mut self.links,
                (base, accent),
                &mut cx,
            );
        } else {
            render_body(self.body(), &mut self.body_scroll, body_area, base, &mut cx);
        }
        let status = format!(
            "{} | {} gates | {} | Tab tabs | q quit",
            self.verdict(),
            self.card.measures.gates.len(),
            if self.tab == 1 {
                "Enter details"
            } else {
                "j/k gates | PgUp/PgDn"
            }
        );
        line(status).style(accent).render(footer, &mut cx);
    }
}

fn render_findings(
    findings: &[Finding],
    selected: usize,
    root: &Path,
    area: Rect,
    links: &mut Vec<Link>,
    styles: (Style, Style),
    cx: &mut RenderCx<'_>,
) {
    let (base, accent) = styles;
    if findings.is_empty() {
        line("No findings in this report")
            .style(base)
            .render(area, cx);
        return;
    }
    let visible = usize::from(area.height.saturating_sub(3));
    let offset = selected.saturating_sub(visible.saturating_sub(1));
    let location_width = (area.width / 2).max(1);
    line(format!(
        "{:<10}{:<width$}Rule",
        "Severity",
        "File:line",
        width = usize::from(location_width)
    ))
    .style(accent)
    .render(Rect::new(area.x, area.y, area.width, 1), cx);
    for (row, finding) in findings.iter().enumerate().skip(offset).take(visible) {
        let y = area.y + 1 + (row - offset) as u16;
        let style = if row == selected { accent } else { base };
        let severity = clean(&finding.severity);
        line(format!(
            "{} {}",
            if row == selected { ">" } else { " " },
            severity
        ))
        .style(style)
        .render(Rect::new(area.x, y, 10, 1), cx);
        let label = location_label(finding);
        let label = clip_display_prefix(&label, location_width).to_string();
        let uri = file_link(root, finding);
        let label_style = if uri.is_some() {
            style.with_attrs(style.attrs.set_underline(UnderlineStyle::Single))
        } else {
            style
        };
        line(&label)
            .style(label_style)
            .render(Rect::new(area.x + 10, y, location_width, 1), cx);
        if let Some(uri) = uri {
            links.push(Link {
                x: area.x + 10,
                y,
                label,
                uri,
                style: label_style,
            });
        }
        let x = area.x + 10 + location_width;
        line(&finding.rule).style(style).render(
            Rect::new(x, y, area.width.saturating_sub(10 + location_width), 1),
            cx,
        );
    }
    let detail = &findings[selected];
    line(format!(
        "{}/{} | PgUp/PgDn Home/End",
        selected + 1,
        findings.len()
    ))
    .style(accent)
    .render(
        Rect::new(area.x, area.y + area.height - 2, area.width, 1),
        cx,
    );
    line(&detail.message).style(base).render(
        Rect::new(area.x, area.y + area.height - 1, area.width, 1),
        cx,
    );
}

fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn location_label(finding: &Finding) -> String {
    let file = if finding.file.is_empty() {
        "(no file)"
    } else {
        &finding.file
    };
    match finding.span.as_ref().filter(|s| s.start_line > 0) {
        Some(span) => format!("{}:{}", clean(file), span.start_line),
        None => clean(file),
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

    pub(super) fn findings_card() -> Scorecard {
        let mut card = Scorecard::skeleton("demo", 30);
        card.sections.findings = (0..6)
            .map(|index| Finding {
                id: format!("finding-{index}"),
                rule: "lint.failed".into(),
                engine: "lint".into(),
                severity: "warning".into(),
                file: format!("src/{index}.rs"),
                span: None,
                symbol: None,
                message: format!("Message {index}"),
                evidence: serde_json::Value::Null,
                suggested_action: None,
                disposition: "ask".into(),
            })
            .collect();
        card
    }

    fn findings_snapshot(browser: &mut Browser) -> Vec<String> {
        let (grid, clusters) = render_once(browser, 100, 12);
        grid.display_text(&clusters)
            .lines()
            .skip(3)
            .take(7)
            .map(|line| {
                line.chars()
                    .skip(25)
                    .take(74)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn findings_viewport_snapshot_at_start_and_end() {
        let mut browser = Browser::new(findings_card());
        browser.update(&Event::Key(KeyEvent::press(KeyCode::Tab)));
        assert_eq!(
            findings_snapshot(&mut browser),
            [
                "Severity  File:line                            Rule",
                "> warning src/0.rs                             lint.failed",
                "  warning src/1.rs                             lint.failed",
                "  warning src/2.rs                             lint.failed",
                "  warning src/3.rs                             lint.failed",
                "1/6 | PgUp/PgDn Home/End",
                "Message 0",
            ]
        );
        browser.update(&Event::Key(KeyEvent::press(KeyCode::End)));
        assert_eq!(
            findings_snapshot(&mut browser),
            [
                "Severity  File:line                            Rule",
                "  warning src/2.rs                             lint.failed",
                "  warning src/3.rs                             lint.failed",
                "  warning src/4.rs                             lint.failed",
                "> warning src/5.rs                             lint.failed",
                "6/6 | PgUp/PgDn Home/End",
                "Message 5",
            ]
        );
        browser.update(&Event::Key(KeyEvent::press(KeyCode::Tab)));
        browser.update(&Event::Key(KeyEvent::press(KeyCode::Tab)));
        let (grid, clusters) = render_once(&mut browser, 100, 12);
        assert!(grid
            .display_text(&clusters)
            .contains("No gates in this report"));
    }

    #[test]
    fn small_layout_renders_recovery_hint() {
        let mut browser = Browser::new(Scorecard::skeleton("demo", 30));
        let (grid, clusters) = render_once(&mut browser, 50, 10);
        assert!(grid.display_text(&clusters).contains("at least 60x12"));
    }
}

#[cfg(test)]
mod gates;
