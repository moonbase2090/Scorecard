// SPDX-License-Identifier: MPL-2.0
//! Scorecard contract, configuration, and score formulas.

mod config;
mod model;
mod score;

pub use config::{
    load_config_file, normalize_gates, resolve_config_path, user_config_file, user_config_path,
    Config, KNOWN_GATES,
};
pub use model::*;
pub use score::{
    compute_scores, crap_score, exceeds_threshold, exit_code_fails, report_verdict, verdict_fails,
};

pub const SCORECARD_VERSION: &str = "0.1";

pub fn new_scorecard_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
