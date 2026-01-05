use std::time::Duration;

use crate::monitor::status::{PaneStatus, TerminalStatus};

pub struct TerminalDetector;

impl TerminalDetector {
    pub fn new() -> Self {
        Self
    }

    pub fn detect_status(output_changed: bool, since_change: Duration) -> PaneStatus {
        if output_changed || since_change < Duration::from_secs(2) {
            PaneStatus::Terminal(TerminalStatus::Active)
        } else {
            PaneStatus::Terminal(TerminalStatus::Idle)
        }
    }
}

impl Default for TerminalDetector {
    fn default() -> Self {
        Self::new()
    }
}
