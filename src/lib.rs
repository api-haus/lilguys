//! lilguys — the daemon's insides, shared with the `lilguy` command that drives it.

pub mod app;
pub mod attention;
pub mod avatar;
pub mod config;
pub mod doctor;
pub mod gpu;
pub mod guy;
pub mod hypr;
pub mod locomotion;
pub mod log;
pub mod mind;
pub mod sensors;
pub mod service;
pub mod setup;
pub mod telemetry;
pub mod voice;

/// The only asset the daemon cannot start without. A check, not an assumption.
pub const FONT: &str = "/usr/share/fonts/noto/NotoSansMono-Regular.ttf";
