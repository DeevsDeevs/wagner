pub mod adapter;
pub mod daemon;
pub mod ipc;
#[cfg(feature = "telegram")]
pub mod telegram;

use serde::{Deserialize, Serialize};

use crate::monitor::QuestionData;
use crate::monitor::status::{PaneStatus, SessionAggregateStatus, WaitReason};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressStep {
    pub tool_name: String,
    pub context: Option<String>,
    pub done: bool,
    pub ok: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum PaneOutputMode {
    Alerts,
    Stream,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreEvent {
    NeedsAttention {
        task_name: String,
        pane_name: String,
        pane_id: String,
        reason: WaitReason,
        output_tail: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        question_data: Option<Vec<QuestionData>>,
    },
    AgentIdle {
        task_name: String,
        pane_name: String,
        pane_id: String,
        output_tail: String,
        #[serde(default)]
        response_text: Option<String>,
    },
    AgentProgress {
        task_name: String,
        pane_name: String,
        pane_id: String,
        steps: Vec<ProgressStep>,
        pending: Option<ProgressStep>,
        step_count: usize,
    },
    AgentWorking {
        task_name: String,
        pane_name: String,
        pane_id: String,
        activity: String,
    },
    SessionStatusChanged {
        task_name: String,
        status: SessionAggregateStatus,
        #[serde(default)]
        panes: Vec<(String, String)>,
    },
    AgentResumed {
        task_name: String,
        pane_name: String,
        pane_id: String,
    },
    DaemonStarted {
        tasks: Vec<TaskSummary>,
    },
    DaemonStopping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSummary {
    pub name: String,
    pub repo_count: usize,
    pub pane_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreCommand {
    ListTasks,
    TaskStatus {
        task_name: String,
    },
    FullStatus,
    SendMessage {
        task_name: String,
        pane_name: Option<String>,
        message: String,
    },
    Approve {
        task_name: String,
        pane_name: Option<String>,
    },
    Reject {
        task_name: String,
        pane_name: Option<String>,
    },
    CaptureOutput {
        task_name: String,
        pane_name: Option<String>,
        lines: Option<usize>,
    },
    Resume {
        task_name: String,
        pane_name: Option<String>,
    },
    PluginList {
        plugin_id: String,
        task_name: Option<String>,
    },
    PluginGet {
        plugin_id: String,
        item_id: String,
    },
    AddPane {
        task_name: String,
        pane_name: Option<String>,
        #[serde(default)]
        agent: Option<String>,
        #[serde(default)]
        repo_name: Option<String>,
    },
    RenamePane {
        task_name: String,
        pane_name: String,
        new_name: String,
    },
    KillPane {
        task_name: String,
        pane_name: String,
    },
    SetPaneMode {
        task_name: String,
        pane_name: Option<String>,
        mode: PaneOutputMode,
    },
    Help,
}

impl CoreCommand {
    /// If this command has an empty `task_name`, fill it (and optionally `pane_name`)
    /// from reply context. Returns `None` when enrichment doesn't apply.
    #[must_use]
    pub fn with_reply_context(
        &self,
        ctx_task: &str,
        ctx_pane: Option<&str>,
    ) -> Option<CoreCommand> {
        let pane = |orig: &Option<String>| -> Option<String> {
            orig.clone().or_else(|| ctx_pane.map(str::to_string))
        };
        match self {
            CoreCommand::TaskStatus { task_name } if task_name.is_empty() => {
                Some(CoreCommand::TaskStatus {
                    task_name: ctx_task.to_string(),
                })
            }
            CoreCommand::SendMessage {
                task_name,
                pane_name,
                message,
            } if task_name.is_empty() => Some(CoreCommand::SendMessage {
                task_name: ctx_task.to_string(),
                pane_name: pane(pane_name),
                message: message.clone(),
            }),
            CoreCommand::Approve {
                task_name,
                pane_name,
            } if task_name.is_empty() => Some(CoreCommand::Approve {
                task_name: ctx_task.to_string(),
                pane_name: pane(pane_name),
            }),
            CoreCommand::Reject {
                task_name,
                pane_name,
            } if task_name.is_empty() => Some(CoreCommand::Reject {
                task_name: ctx_task.to_string(),
                pane_name: pane(pane_name),
            }),
            CoreCommand::CaptureOutput {
                task_name,
                pane_name,
                lines,
            } if task_name.is_empty() => Some(CoreCommand::CaptureOutput {
                task_name: ctx_task.to_string(),
                pane_name: pane(pane_name),
                lines: *lines,
            }),
            CoreCommand::Resume {
                task_name,
                pane_name,
            } if task_name.is_empty() => Some(CoreCommand::Resume {
                task_name: ctx_task.to_string(),
                pane_name: pane(pane_name),
            }),
            CoreCommand::KillPane { task_name, .. } if task_name.is_empty() => {
                Some(CoreCommand::KillPane {
                    task_name: ctx_task.to_string(),
                    pane_name: ctx_pane?.to_string(),
                })
            }
            CoreCommand::SetPaneMode {
                task_name,
                pane_name,
                mode,
            } if task_name.is_empty() => Some(CoreCommand::SetPaneMode {
                task_name: ctx_task.to_string(),
                pane_name: pane(pane_name),
                mode: *mode,
            }),
            CoreCommand::AddPane {
                task_name,
                pane_name,
                agent,
                repo_name,
            } if task_name.is_empty() => Some(CoreCommand::AddPane {
                task_name: ctx_task.to_string(),
                pane_name: pane(pane_name),
                agent: agent.clone(),
                repo_name: repo_name.clone(),
            }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(clippy::type_complexity)]
pub enum CoreResponse {
    TaskList {
        tasks: Vec<(TaskSummary, SessionAggregateStatus)>,
    },
    Status {
        task_name: String,
        summary: TaskSummary,
        status: SessionAggregateStatus,
        panes: Vec<(String, PaneStatus)>,
    },
    FullStatus {
        tasks: Vec<(
            TaskSummary,
            SessionAggregateStatus,
            Vec<(String, PaneStatus)>,
        )>,
    },
    Output {
        task_name: String,
        pane_name: String,
        content: String,
    },
    Confirmation {
        message: String,
    },
    PluginItems {
        plugin_id: String,
        items: Vec<crate::plugins::PluginItem>,
    },
    PluginDetail {
        plugin_id: String,
        detail: crate::plugins::PluginItemDetail,
    },
    ModeChanged {
        task_name: String,
        pane_name: String,
        mode: PaneOutputMode,
    },
    Error {
        message: String,
    },
    HelpText,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::status::*;
    use std::collections::HashMap;

    fn roundtrip_cmd(cmd: &CoreCommand) {
        let json = serde_json::to_string(cmd).unwrap();
        let back: CoreCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(format!("{:?}", cmd), format!("{:?}", back));
    }

    fn roundtrip_resp(resp: &CoreResponse) {
        let json = serde_json::to_string(resp).unwrap();
        let back: CoreResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(format!("{:?}", resp), format!("{:?}", back));
    }

    fn roundtrip_event(event: &CoreEvent) {
        let json = serde_json::to_string(event).unwrap();
        let back: CoreEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(format!("{:?}", event), format!("{:?}", back));
    }

    #[test]
    fn serde_core_command_all_variants() {
        let variants = vec![
            CoreCommand::ListTasks,
            CoreCommand::TaskStatus {
                task_name: "my-task".into(),
            },
            CoreCommand::FullStatus,
            CoreCommand::SendMessage {
                task_name: "t".into(),
                pane_name: Some("api".into()),
                message: "hello".into(),
            },
            CoreCommand::SendMessage {
                task_name: "t".into(),
                pane_name: None,
                message: "hello".into(),
            },
            CoreCommand::Approve {
                task_name: "t".into(),
                pane_name: Some("web".into()),
            },
            CoreCommand::Reject {
                task_name: "t".into(),
                pane_name: None,
            },
            CoreCommand::CaptureOutput {
                task_name: "t".into(),
                pane_name: Some("api".into()),
                lines: Some(50),
            },
            CoreCommand::CaptureOutput {
                task_name: "t".into(),
                pane_name: None,
                lines: None,
            },
            CoreCommand::Resume {
                task_name: "t".into(),
                pane_name: None,
            },
            CoreCommand::PluginList {
                plugin_id: "chains".into(),
                task_name: Some("my-task".into()),
            },
            CoreCommand::PluginGet {
                plugin_id: "chains".into(),
                item_id: "link-1".into(),
            },
            CoreCommand::AddPane {
                task_name: "t".into(),
                pane_name: Some("api".into()),
                agent: None,
                repo_name: None,
            },
            CoreCommand::AddPane {
                task_name: "t".into(),
                pane_name: None,
                agent: Some("codex".into()),
                repo_name: None,
            },
            CoreCommand::AddPane {
                task_name: "t".into(),
                pane_name: None,
                agent: Some("terminal".into()),
                repo_name: Some("backend".into()),
            },
            CoreCommand::RenamePane {
                task_name: "t".into(),
                pane_name: "old".into(),
                new_name: "new".into(),
            },
            CoreCommand::KillPane {
                task_name: "t".into(),
                pane_name: "api".into(),
            },
            CoreCommand::SetPaneMode {
                task_name: "t".into(),
                pane_name: Some("api".into()),
                mode: PaneOutputMode::Stream,
            },
            CoreCommand::Help,
        ];
        for cmd in &variants {
            roundtrip_cmd(cmd);
        }
    }

    #[test]
    fn serde_core_response_all_variants() {
        let variants = vec![
            CoreResponse::TaskList {
                tasks: vec![(
                    TaskSummary {
                        name: "t".into(),
                        repo_count: 2,
                        pane_count: 1,
                    },
                    SessionAggregateStatus::Working,
                )],
            },
            CoreResponse::Status {
                task_name: "t".into(),
                summary: TaskSummary {
                    name: "t".into(),
                    repo_count: 1,
                    pane_count: 3,
                },
                status: SessionAggregateStatus::Working,
                panes: vec![
                    (
                        "api".into(),
                        PaneStatus::Agent {
                            agent_type: AgentType::ClaudeCode,
                            status: AgentStatus::Active(Activity::new(ActivityKind::Claude(
                                ClaudeActivity::Thinking,
                            ))),
                        },
                    ),
                    ("shell".into(), PaneStatus::Terminal(TerminalStatus::Idle)),
                    ("unknown".into(), PaneStatus::Unknown),
                ],
            },
            CoreResponse::FullStatus {
                tasks: vec![(
                    TaskSummary {
                        name: "t".into(),
                        repo_count: 1,
                        pane_count: 1,
                    },
                    SessionAggregateStatus::NeedsAttention,
                    vec![(
                        "api".into(),
                        PaneStatus::Agent {
                            agent_type: AgentType::Codex,
                            status: AgentStatus::Waiting(WaitReason::Approval),
                        },
                    )],
                )],
            },
            CoreResponse::Output {
                task_name: "t".into(),
                pane_name: "api".into(),
                content: "some output".into(),
            },
            CoreResponse::Confirmation {
                message: "Done".into(),
            },
            CoreResponse::PluginItems {
                plugin_id: "chains".into(),
                items: vec![crate::plugins::PluginItem {
                    id: "1".into(),
                    name: "link".into(),
                    summary: "summary".into(),
                    metadata: HashMap::from([("key".into(), "val".into())]),
                }],
            },
            CoreResponse::PluginDetail {
                plugin_id: "chains".into(),
                detail: crate::plugins::PluginItemDetail {
                    item: crate::plugins::PluginItem {
                        id: "1".into(),
                        name: "link".into(),
                        summary: "summary".into(),
                        metadata: HashMap::new(),
                    },
                    content: "full content".into(),
                },
            },
            CoreResponse::ModeChanged {
                task_name: "t".into(),
                pane_name: "api".into(),
                mode: PaneOutputMode::Stream,
            },
            CoreResponse::Error {
                message: "something failed".into(),
            },
            CoreResponse::HelpText,
        ];
        for resp in &variants {
            roundtrip_resp(resp);
        }
    }

    #[test]
    fn serde_core_event_all_variants() {
        let variants = vec![
            CoreEvent::NeedsAttention {
                task_name: "t".into(),
                pane_name: "api".into(),
                pane_id: "%1".into(),
                reason: WaitReason::Question,
                output_tail: "tail".into(),
                question_data: None,
            },
            CoreEvent::AgentIdle {
                task_name: "t".into(),
                pane_name: "web".into(),
                pane_id: "%2".into(),
                output_tail: "done".into(),
                response_text: None,
            },
            CoreEvent::AgentWorking {
                task_name: "t".into(),
                pane_name: "api".into(),
                pane_id: "%3".into(),
                activity: "Thinking".into(),
            },
            CoreEvent::SessionStatusChanged {
                task_name: "t".into(),
                status: SessionAggregateStatus::Idle,
                panes: vec![],
            },
            CoreEvent::DaemonStarted {
                tasks: vec![TaskSummary {
                    name: "t".into(),
                    repo_count: 1,
                    pane_count: 2,
                }],
            },
            CoreEvent::AgentProgress {
                task_name: "t".into(),
                pane_name: "api".into(),
                pane_id: "%4".into(),
                steps: vec![ProgressStep {
                    tool_name: "Bash".into(),
                    context: Some("cargo test".into()),
                    done: true,
                    ok: true,
                }],
                pending: Some(ProgressStep {
                    tool_name: "Edit".into(),
                    context: Some("src/lib.rs".into()),
                    done: false,
                    ok: true,
                }),
                step_count: 2,
            },
            CoreEvent::AgentResumed {
                task_name: "t".into(),
                pane_name: "api".into(),
                pane_id: "%5".into(),
            },
            CoreEvent::DaemonStopping,
        ];
        for event in &variants {
            roundtrip_event(event);
        }
    }

    #[test]
    fn serde_needs_attention_with_question_data() {
        use crate::monitor::{QuestionData, QuestionOption};

        let event = CoreEvent::NeedsAttention {
            task_name: "t".into(),
            pane_name: "api".into(),
            pane_id: "%1".into(),
            reason: WaitReason::Question,
            output_tail: "Which DB?".into(),
            question_data: Some(vec![QuestionData {
                question: "Which DB?".into(),
                options: vec![
                    QuestionOption {
                        label: "Postgres".into(),
                        description: Some("SQL".into()),
                    },
                    QuestionOption {
                        label: "Mongo".into(),
                        description: None,
                    },
                ],
                multi_select: false,
            }]),
        };
        roundtrip_event(&event);

        // Also verify backward compat: None should not appear in JSON
        let event_none = CoreEvent::NeedsAttention {
            task_name: "t".into(),
            pane_name: "api".into(),
            pane_id: "%1".into(),
            reason: WaitReason::Question,
            output_tail: "tail".into(),
            question_data: None,
        };
        let json = serde_json::to_string(&event_none).unwrap();
        assert!(!json.contains("question_data"));
        let back: CoreEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(format!("{:?}", event_none), format!("{:?}", back));
    }

    #[test]
    fn serde_ipc_request_roundtrip() {
        use crate::transport::ipc::{IpcRequest, IpcResponse};

        let req = IpcRequest {
            command: CoreCommand::ListTasks,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: IpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(format!("{:?}", req), format!("{:?}", back));

        let resp = IpcResponse {
            response: CoreResponse::Error {
                message: "test".into(),
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: IpcResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(format!("{:?}", resp), format!("{:?}", back));
    }

    #[test]
    fn serde_nested_pane_status_all_activity_kinds() {
        let statuses = vec![
            PaneStatus::Agent {
                agent_type: AgentType::ClaudeCode,
                status: AgentStatus::Active(Activity::new(ActivityKind::Generic(
                    GenericActivity::Working,
                ))),
            },
            PaneStatus::Agent {
                agent_type: AgentType::ClaudeCode,
                status: AgentStatus::Active(Activity::new(ActivityKind::Claude(
                    ClaudeActivity::ToolBash,
                ))),
            },
            PaneStatus::Agent {
                agent_type: AgentType::Codex,
                status: AgentStatus::Active(Activity::new(ActivityKind::Codex(
                    CodexActivity::Streaming,
                ))),
            },
            PaneStatus::Agent {
                agent_type: AgentType::ClaudeCode,
                status: AgentStatus::Waiting(WaitReason::Permission),
            },
            PaneStatus::Agent {
                agent_type: AgentType::ClaudeCode,
                status: AgentStatus::Idle,
            },
            PaneStatus::Terminal(TerminalStatus::Active),
            PaneStatus::Terminal(TerminalStatus::Idle),
            PaneStatus::Unknown,
        ];
        for status in &statuses {
            let json = serde_json::to_string(status).unwrap();
            let back: PaneStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, &back);
        }
    }

    #[test]
    fn serde_error_response_backward_compat() {
        let json = r#"{"error":{"message":"something went wrong"}}"#;
        let resp: CoreResponse = serde_json::from_str(json).unwrap();
        match resp {
            CoreResponse::Error { message } => {
                assert_eq!(message, "something went wrong");
            }
            _ => panic!("expected Error variant"),
        }
    }
}
