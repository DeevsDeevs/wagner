pub mod daemon;
#[cfg(feature = "telegram")]
pub mod telegram;

use crate::monitor::status::{PaneStatus, SessionAggregateStatus, WaitReason};

pub trait Transport: Send + Sync + 'static {
    fn name(&self) -> &str;
    async fn send_event(
        &self,
        event: &TransportEvent,
    ) -> crate::Result<Option<MessageRef>>;
    async fn edit_message(
        &self,
        msg_ref: &MessageRef,
        event: &TransportEvent,
    ) -> crate::Result<Option<MessageRef>>;
    async fn send_response(
        &self,
        response: &CommandResponse,
        reply_to: Option<&MessageRef>,
    ) -> crate::Result<Option<MessageRef>>;
    async fn poll_commands(&self) -> crate::Result<Vec<(RemoteCommand, MessageRef)>>;
}

#[derive(Debug, Clone)]
pub struct ActionButton {
    pub label: String,
    pub callback_data: String,
}

pub struct RenderedMessage {
    pub text: String,
    pub buttons: Vec<Vec<ActionButton>>,
}

#[derive(Debug, Clone)]
pub enum TransportEvent {
    NeedsAttention {
        task_name: String,
        pane_id: String,
        pane_title: String,
        reason: WaitReason,
        output_tail: String,
        actions: Vec<Vec<ActionButton>>,
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
        actions: Vec<Vec<ActionButton>>,
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
pub enum RemoteCommand {
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
    ReplyInput {
        reply_to_message_id: i32,
        text: String,
    },
    Callback {
        data: String,
        source_message_id: i32,
    },
    Resume {
        task_name: String,
        pane_id: Option<String>,
    },
    Focus {
        task_name: String,
        pane_id: Option<String>,
        sticky: bool,
    },
    Unfocus,
    Help,
    Unknown {
        text: String,
    },
}

#[derive(Debug, Clone)]
pub enum CommandResponse {
    TaskList {
        tasks: Vec<(TaskSummary, SessionAggregateStatus)>,
    },
    Status {
        task_name: String,
        panes: Vec<(String, PaneStatus)>,
        actions: Vec<Vec<ActionButton>>,
    },
    FullStatus {
        tasks: Vec<(TaskSummary, SessionAggregateStatus, Vec<(String, PaneStatus)>)>,
        actions: Vec<Vec<ActionButton>>,
    },
    Output {
        task_name: String,
        pane_id: String,
        content: String,
    },
    Confirmation {
        message: String,
        actions: Vec<Vec<ActionButton>>,
    },
    Error {
        message: String,
    },
    HelpText,
}

#[derive(Debug, Clone)]
pub struct MessageRef {
    pub chat_id: i64,
    pub message_id: i32,
    pub edit_in_place: bool,
}
