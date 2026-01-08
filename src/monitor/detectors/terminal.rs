use std::time::Duration;

use crate::monitor::detector::IDLE_THRESHOLD;
use crate::monitor::status::{PaneStatus, TerminalStatus};

pub struct TerminalDetector;

impl TerminalDetector {
    pub fn detect_status(output_changed: bool, since_change: Duration) -> PaneStatus {
        if output_changed || since_change < IDLE_THRESHOLD {
            PaneStatus::Terminal(TerminalStatus::Active)
        } else {
            PaneStatus::Terminal(TerminalStatus::Idle)
        }
    }
}
