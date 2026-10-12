// SPDX-License-Identifier: MPL-2.0
//! Deterministic engines. They return findings and never print diagnostics.

mod a11y;
mod cargo_test;
mod clippy;
mod command;
mod compile;
mod coverage;
mod crap;
mod facts;
mod html_doc;
mod links;
mod manifest;
mod mutation;
mod pack;
mod pack_cov;
mod pipeline;
mod poly_cc;
mod python;
mod python_imports;
mod rust_toolchain;
mod scope;
mod secrets;
mod spec_check;
mod toolchain;
mod toolchain_plans;
mod weakening;

pub use pipeline::{
    analyze, analyze_with_generated_paths, AnalyzeOutput, AnalyzeRequest, ProgressCallback,
    RunStatus,
};
