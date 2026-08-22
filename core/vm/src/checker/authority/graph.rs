use super::*;

// Graph construction, seed extraction, higher-order specialization, and
// frozen reporting remain separate authority-analysis responsibilities.
include!("graph/build.rs");
include!("graph/seeds.rs");
include!("graph/specialization.rs");
include!("graph/report.rs");
