use wagner::model::Engine;
use wagner::{Agent, AgentChoice, Droid, WagnerError};

#[test]
fn test_agent_choice_claude() {
    let agent = AgentChoice::from_key("claude").unwrap();
    assert_eq!(
        agent.launch_command("test-id"),
        "claude --session-id test-id"
    );
    assert_eq!(agent.name(), "claude-code");
    assert_eq!(agent.engine(), Engine::ClaudeCode);
    assert!(matches!(agent, AgentChoice::Claude(_)));
}

#[test]
fn test_agent_choice_codex() {
    let agent = AgentChoice::from_key("codex").unwrap();
    assert_eq!(agent.launch_command("test-id"), "codex");
    assert_eq!(agent.name(), "codex");
    assert_eq!(agent.engine(), Engine::Codex);
    assert!(matches!(agent, AgentChoice::Codex(_)));
}

#[test]
fn test_agent_choice_invalid() {
    let err = AgentChoice::from_key("unknown").unwrap_err();
    assert!(matches!(err, WagnerError::InvalidAgent(_)));
}

#[test]
fn test_claude_predict_jsonl_path() {
    let agent = AgentChoice::from_key("claude").unwrap();
    let cwd = std::path::Path::new("/Users/test/myproject");
    let path = agent.predict_jsonl_path("abc-123", cwd);
    assert!(path.is_some());
    let path = path.unwrap();
    assert!(path.to_string_lossy().ends_with("abc-123.jsonl"));
    assert!(path.to_string_lossy().contains("-Users-test-myproject"));
}

#[test]
fn test_codex_predict_jsonl_path_is_none() {
    let agent = AgentChoice::from_key("codex").unwrap();
    let cwd = std::path::Path::new("/Users/test/myproject");
    assert!(agent.predict_jsonl_path("abc-123", cwd).is_none());
}

#[test]
fn test_claude_resume_command() {
    let agent = AgentChoice::from_key("claude").unwrap();
    assert_eq!(
        agent.resume_command("my-session"),
        "claude --resume my-session"
    );
}

#[test]
fn test_agent_choice_from_key_droid() {
    let agent = AgentChoice::from_key("droid").unwrap();
    assert_eq!(agent.launch_command("test-id"), "droid");
    assert_eq!(agent.name(), "droid");
    assert_eq!(agent.engine(), Engine::Droid);
    assert!(matches!(agent, AgentChoice::Droid(_)));
}

#[test]
fn test_droid_predict_jsonl_path() {
    let agent = AgentChoice::from_key("droid").unwrap();
    let cwd = std::path::Path::new("/Users/test/myproject");
    let path = agent.predict_jsonl_path("abc-123", cwd);
    assert!(path.is_some());
    let path = path.unwrap();
    let path_str = path.to_string_lossy();
    assert!(path_str.ends_with("abc-123.jsonl"));
    assert!(path_str.contains(".factory/sessions/"));
    assert!(path_str.contains("-Users-test-myproject"));
}

#[test]
fn test_droid_resume_command() {
    let agent = AgentChoice::from_key("droid").unwrap();
    assert_eq!(
        agent.resume_command("my-session"),
        "droid --resume my-session"
    );
}

#[test]
fn test_droid_agent_direct() {
    let agent = Droid::new();
    assert_eq!(agent.name(), "droid");
    assert_eq!(agent.engine(), Engine::Droid);
    assert_eq!(agent.launch_command("ses-1"), "droid");
    assert_eq!(agent.resume_command("ses-1"), "droid --resume ses-1");
}
