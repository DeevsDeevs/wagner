use wagner::{Agent, AgentChoice, WagnerError};

#[test]
fn test_agent_choice_claude() {
    let agent = AgentChoice::from_key("claude").unwrap();
    assert_eq!(agent.launch_command(), "claude");
    assert_eq!(agent.name(), "claude-code");
    assert!(matches!(agent, AgentChoice::Claude(_)));
}

#[test]
fn test_agent_choice_codex() {
    let agent = AgentChoice::from_key("codex").unwrap();
    assert_eq!(agent.launch_command(), "codex");
    assert_eq!(agent.name(), "codex");
    assert!(matches!(agent, AgentChoice::Codex(_)));
}

#[test]
fn test_agent_choice_invalid() {
    let err = AgentChoice::from_key("unknown").unwrap_err();
    assert!(matches!(err, WagnerError::InvalidAgent(_)));
}
