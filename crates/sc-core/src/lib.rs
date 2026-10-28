// SPDX-License-Identifier: MIT
//! Scorecard contract, configuration, and score formulas.

mod baseline;
mod config;
mod model;
mod score;

pub use baseline::{gates_failing_on_new_findings, Baseline, BASELINE_VERSION};
pub use config::{
    load_config_file, normalize_gates, resolve_config_path, user_config_dir, user_config_file,
    user_config_path, user_profile_dir, Config, KNOWN_GATES, PROFILE_UNSET,
};
pub use model::*;
pub use score::{
    compute_scores, crap_score, exceeds_threshold, exit_code_fails, report_verdict, verdict_fails,
};

pub const SCORECARD_VERSION: &str = "0.1";

pub fn new_scorecard_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
