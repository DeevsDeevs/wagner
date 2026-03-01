pub mod adapter;
pub mod daemon;
#[cfg(feature = "telegram")]
pub mod telegram;

use crate::monitor::status::{PaneStatus, SessionAggregateStatus, WaitReason};

#[derive(Debug, Clone)]
pub enum CoreEvent {
    NeedsAttention {
        task_name: String,
        pane_id: String,
        pane_title: String,
        reason: WaitReason,
        output_tail: String,
    },
    AgentIdle {
        task_name: String,
        pane_id: String,
        pane_title: String,
        output_tail: String,
    },
    AgentWorking {
        task_name: String,
        pane_id: String,
        pane_title: String,
        activity: String,
    },
    SessionStatusChanged {
        task_name: String,
        status: SessionAggregateStatus,
    },
    AgentResumed {
        task_name: String,
        pane_id: String,
        pane_title: String,
    },
    DaemonStarted {
        tasks: Vec<TaskSummary>,
    },
    DaemonStopping,
}

#[derive(Debug, Clone)]
pub struct TaskSummary {
    pub name: String,
    pub repo_count: usize,
    pub pane_count: usize,
}

#[derive(Debug, Clone)]
pub enum CoreCommand {
    ListTasks,
    TaskStatus { task_name: String },
    FullStatus,
    SendMessage {
        task_name: String,
        pane_id: Option<String>,
        message: String,
    },
    Approve {
        task_name: String,
        pane_id: Option<String>,
    },
    Reject {
        task_name: String,
        pane_id: Option<String>,
    },
    CaptureOutput {
        task_name: String,
        pane_id: Option<String>,
        lines: Option<usize>,
    },
    Resume {
        task_name: String,
        pane_id: Option<String>,
    },
    PluginList {
        plugin_id: String,
        task_name: Option<String>,
    },
    PluginGet {
        plugin_id: String,
        item_id: String,
    },
    Help,
}

#[derive(Debug, Clone)]
pub enum CoreResponse {
    TaskList {
        tasks: Vec<(TaskSummary, SessionAggregateStatus)>,
    },
    Status {
        task_name: String,
        panes: Vec<(String, PaneStatus)>,
    },
    FullStatus {
        tasks: Vec<(TaskSummary, SessionAggregateStatus, Vec<(String, PaneStatus)>)>,
    },
    Output {
        task_name: String,
        pane_id: String,
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
    Error {
        message: String,
    },
    HelpText,
}
