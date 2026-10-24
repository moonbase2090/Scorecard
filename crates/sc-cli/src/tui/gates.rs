// SPDX-License-Identifier: MIT
use super::*;
use lunatui_runtime::render_once;
use lunatui_term::KeyEvent;
use lunatui_test::{diff_matches, styled_snapshot, Oracle};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

fn report(rows: usize) -> Scorecard {
    let mut card = tests::findings_card();
    let finding = card.sections.findings[0].clone();
    card.sections.findings = (0..rows)
        .map(|index| Finding {
            id: format!("finding-{index}"),
            file: "Cargo.toml".into(),
            message: format!("Finding {index}: check this dependency"),
            ..finding.clone()
        })
        .collect();
    card.measures.gates = vec![
        sc_core::Gate {
            id: "types".into(),
            pass: true,
            enforced: true,
            reason: None,
        },
        sc_core::Gate {
            id: "tests".into(),
            pass: false,
            enforced: true,
            reason: Some("One test failed".into()),
        },
        sc_core::Gate {
            id: "sca".into(),
            pass: false,
            enforced: false,
            reason: Some("Dependency warning".into()),
        },
    ];
    card.engines.engines_run = vec!["compile".into(), "tests".into()];
    card.engines.engines_skipped = vec!["coverage".into(), "mutation".into()];
    card
}

#[test]
fn styled_browser_snapshots() {
    for (width, height) in [(60, 12), (100, 24)] {
        let mut browser = Browser::new(report(40));
        for (state, key) in [
            ("overview", None),
            ("failed_gate", Some(KeyCode::Down)),
            ("advisory_gate", Some(KeyCode::Down)),
            ("findings_first", Some(KeyCode::Tab)),
            ("findings_last", Some(KeyCode::End)),
            ("engines", Some(KeyCode::Tab)),
        ] {
            if let Some(key) = key {
                browser.update(&Event::Key(KeyEvent::press(key)));
            }
            let (grid, clusters) = render_once(&mut browser, width, height);
            diff_matches(
                &Grid::new(width, height),
                &grid,
                &clusters,
                ColorMode::TrueColor,
            )
            .unwrap();
            insta::assert_snapshot!(
                format!("{state}_{width}x{height}"),
                styled_snapshot(&grid, &clusters)
            );
        }
    }
}

struct Capture {
    bytes: Rc<RefCell<Vec<u8>>>,
}
impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = io::stdout().write(bytes)?;
        self.bytes.borrow_mut().extend_from_slice(&bytes[..written]);
        Ok(written)
    }
    fn flush(&mut self) -> io::Result<()> {
        io::stdout().flush()
    }
}

#[test]
#[ignore = "release-only PTY benchmark; run scripts/verify-tui-perf.sh"]
fn frame_budget() {
    let transcript = Rc::new(RefCell::new(Vec::new()));
    let root = std::env::current_dir().unwrap().canonicalize().unwrap();
    assert!(root.join("Cargo.toml").is_file());
    let budget = Duration::from_millis(16);
    let mut failures = Vec::new();
    for (width, height) in [(100, 24), (400, 120), (800, 240)] {
        for repaint in [true, false] {
            let mut browser = Browser::with_root(report(100_000), root.clone());
            browser.tab = 1;
            let mut frame = Frame::new(width, height);
            let mut maximum = Duration::ZERO;
            let mut total_bytes = 0;
            let mut oracle = Oracle::new(height, width);
            let mut session = Session::enter(Capture {
                bytes: transcript.clone(),
            })
            .unwrap();
            for sample in 0..30 {
                if repaint {
                    io::stdout().write_all(b"\x1b[2J\x1b[H").unwrap();
                    io::stdout().flush().unwrap();
                    oracle = Oracle::new(height, width);
                }
                transcript.borrow_mut().clear();
                let elapsed;
                {
                    let start = Instant::now();
                    if repaint {
                        frame = Frame::new(width, height);
                    }
                    browser.update(&Event::Key(KeyEvent::press(if sample % 2 == 0 {
                        KeyCode::End
                    } else {
                        KeyCode::Home
                    })));
                    frame.present(&mut browser, &mut session).unwrap();
                    elapsed = start.elapsed();
                }
                maximum = maximum.max(elapsed);
                if elapsed > budget {
                    failures.push(format!(
                        "{width}x{height} repaint={repaint} sample={sample}: {elapsed:?}"
                    ));
                }
                let bytes = transcript.borrow();
                total_bytes += bytes.len();
                assert!(bytes.len() > 50, "frame emitted no useful output");
                oracle.process(&bytes);
                // File links are a final styled overlay after grid encoding.
                let mut expected = frame.previous.clone();
                for link in &frame.previous_links {
                    assert_eq!(link.label, "Cargo.toml");
                    expected.set_style(Rect::new(link.x, link.y, 10, 1), link.style);
                }
                let errors = oracle.mismatches(&expected, &frame.clusters, true);
                assert!(errors.is_empty(), "{}", errors.join("\n"));
                assert!(oracle.contents().contains(if sample % 2 == 0 {
                    "Finding 99999:"
                } else {
                    "Finding 0:"
                }));
            }
            drop(session);
            eprintln!("SC_TUI_PERF {width}x{height} repaint={repaint} samples=30 max_ms={:.3} bytes={total_bytes} budget_ms=16", maximum.as_secs_f64() * 1000.0);
        }
    }
    assert!(
        failures.is_empty(),
        "frame budget exceeded:\n{}",
        failures.join("\n")
    );
}
