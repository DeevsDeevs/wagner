use std::time::Duration;

use wagner::monitor::status::{ActivityKind, AgentStatus, CodexActivity, WaitReason};
use wagner::monitor::AgentDetector;
use wagner::CodexDetector;

#[test]
fn test_codex_detect_agent() {
    let detector = CodexDetector::default();
    assert!(detector.detect_agent("codex", ""));
    assert!(detector.detect_agent("", "OpenAI Codex"));
    assert!(detector.detect_agent("", "Ask Codex to do anything"));
}

#[test]
fn test_codex_detect_working() {
    let detector = CodexDetector::default();
    let status = detector.detect_status("", "• Working (0s • esc to interrupt)", true, Duration::from_secs(1));
    match status {
        AgentStatus::Active(activity) => {
            assert!(matches!(activity.kind, ActivityKind::Codex(CodexActivity::Working)));
        }
        other => panic!("expected active status, got {other:?}"),
    }
}

#[test]
fn test_codex_detect_streaming() {
    let detector = CodexDetector::default();
    let status = detector.detect_status("", "• Streaming response.", true, Duration::from_secs(1));
    match status {
        AgentStatus::Active(activity) => {
            assert!(matches!(activity.kind, ActivityKind::Codex(CodexActivity::Streaming)));
        }
        other => panic!("expected active status, got {other:?}"),
    }
}

#[test]
fn test_codex_detect_waiting() {
    let detector = CodexDetector::default();
    let status = detector.detect_status(
        "",
        "Press enter to confirm or esc to cancel",
        true,
        Duration::from_secs(1),
    );
    assert!(matches!(status, AgentStatus::Waiting(WaitReason::Approval)));
}
