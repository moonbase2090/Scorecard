// SPDX-License-Identifier: MPL-2.0
//! Time `crap_score` across a fixed table of functions.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use sc_core::crap_score;

/// Cyclomatic complexity and line coverage, one pair per function.
const FUNCTIONS: [(u32, f64); 32] = [
    (1, 1.0),
    (1, 0.0),
    (2, 0.5),
    (3, 0.9),
    (4, 0.25),
    (5, 0.0),
    (5, 1.0),
    (6, 0.0),
    (6, 0.5),
    (8, 0.75),
    (10, 0.0),
    (10, 0.4),
    (10, 1.0),
    (12, 0.0),
    (12, 0.8),
    (15, 0.33),
    (18, 0.6),
    (20, 0.1),
    (25, 0.5),
    (30, 0.95),
    (35, 0.2),
    (40, 0.0),
    (40, 0.7),
    (50, 0.85),
    (60, 0.15),
    (70, 0.45),
    (80, 0.2),
    (90, 0.99),
    (100, 0.0),
    (100, 1.0),
    (120, 0.55),
    (200, 0.3),
];

fn crap_score_table(c: &mut Criterion) {
    c.bench_function("crap_score_table", |b| {
        b.iter(|| {
            let mut total = 0.0;
            for (complexity, coverage) in FUNCTIONS {
                total += crap_score(black_box(complexity), black_box(coverage));
            }
            total
        });
    });
}

criterion_group!(benches, crap_score_table);
criterion_main!(benches);
