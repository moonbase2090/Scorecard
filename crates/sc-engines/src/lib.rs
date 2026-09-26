// SPDX-License-Identifier: MPL-2.0
//! Deterministic engines. They return findings and never print diagnostics.

mod cargo_test;
mod command;
mod compile;
mod coverage;
mod crap;
mod facts;
mod manifest;
mod mutation;
mod pack;
mod pack_cov;
mod pipeline;
mod poly_cc;
mod python;
mod scope;
mod secrets;
mod spec_check;
mod toolchain;

pub use pipeline::{analyze, AnalyzeOutput, AnalyzeRequest, RunStatus};
