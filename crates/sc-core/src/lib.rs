//! Scorecard contract, configuration, and score formulas.

mod config;
mod model;
mod score;

pub use config::{load_config_file, normalize_gates, resolve_config_path, Config, KNOWN_GATES};
pub use model::*;
pub use score::{compute_scores, crap_score, exceeds_threshold, verdict_fails};

pub const SCORECARD_VERSION: &str = "0.1";

pub fn new_scorecard_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
