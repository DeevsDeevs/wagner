use std::fmt;
use std::time::Instant;

use crate::terminal::PaneHandle;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AgentType {
    ClaudeCode,
}

impl fmt::Display for AgentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClaudeCode => write!(f, "claude"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PaneStatus {
    Agent {
        agent_type: AgentType,
        status: AgentStatus,
    },
    Terminal(TerminalStatus),
    Unknown,
}

impl PaneStatus {
    pub fn icon(&self) -> char {
        match self {
            Self::Agent { status, .. } => status.icon(),
            Self::Terminal(s) => s.icon(),
            Self::Unknown => '?',
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Agent { status, .. } => status.label(),
            Self::Terminal(s) => s.label().to_string(),
            Self::Unknown => "Unknown".to_string(),
        }
    }

    pub fn is_waiting(&self) -> bool {
        matches!(self, Self::Agent { status: AgentStatus::Waiting(_), .. })
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self,
            Self::Agent { status: AgentStatus::Active(_), .. } | Self::Terminal(TerminalStatus::Active)
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentStatus {
    Starting,
    Active(Activity),
    Waiting(WaitReason),
    Idle,
    Error(String),
}

impl AgentStatus {
    pub fn icon(&self) -> char {
        match self {
            Self::Starting => '◐',
            Self::Active(a) => a.icon(),
            Self::Waiting(_) => '◉',
            Self::Idle => '○',
            Self::Error(_) => '✗',
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Starting => "Starting".to_string(),
            Self::Active(a) => a.label().to_string(),
            Self::Waiting(r) => format!("Waiting: {}", r.label()),
            Self::Idle => "Idle".to_string(),
            Self::Error(e) => format!("Error: {}", e),
        }
    }

    pub fn is_waiting(&self) -> bool {
        matches!(self, Self::Waiting(_))
    }

    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active(_) | Self::Starting)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalStatus {
    Active,
    Idle,
}

impl TerminalStatus {
    pub fn icon(&self) -> char {
        match self {
            Self::Active => '●',
            Self::Idle => '○',
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Idle => "Idle",
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitReason {
    Approval,
    Question,
    Permission,
    Input,
}

impl WaitReason {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Approval => "Approval",
            Self::Question => "Question",
            Self::Permission => "Permission",
            Self::Input => "Input",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Activity {
    pub kind: ActivityKind,
}

impl Activity {
    pub fn new(kind: ActivityKind) -> Self {
        Self { kind }
    }

    pub fn label(&self) -> &'static str {
        self.kind.label()
    }

    pub fn icon(&self) -> char {
        self.kind.icon()
    }

    pub fn generic_working() -> Self {
        Self::new(ActivityKind::Generic(GenericActivity::Working))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActivityKind {
    Generic(GenericActivity),
    Claude(ClaudeActivity),
}

impl ActivityKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Generic(a) => a.label(),
            Self::Claude(a) => a.label(),
        }
    }

    pub fn icon(&self) -> char {
        match self {
            Self::Generic(a) => a.icon(),
            Self::Claude(a) => a.icon(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenericActivity {
    Working,
}

impl GenericActivity {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Working => "Working",
        }
    }

    pub fn icon(&self) -> char {
        match self {
            Self::Working => '●',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaudeActivity {
    Thinking,
    Exploring,
    ToolBash,
    ToolEdit,
    ToolWrite,
    ToolRead,
    Subagent,
    WebSearch,
    WebFetch,
    Planning,
    TodoUpdate,
}

impl ClaudeActivity {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Thinking => "Thinking",
            Self::Exploring => "Exploring",
            Self::ToolBash => "Bash",
            Self::ToolEdit => "Edit",
            Self::ToolWrite => "Write",
            Self::ToolRead => "Read",
            Self::Subagent => "Subagent",
            Self::WebSearch => "Web Search",
            Self::WebFetch => "Web Fetch",
            Self::Planning => "Planning",
            Self::TodoUpdate => "Todo",
        }
    }

    pub fn icon(&self) -> char {
        match self {
            Self::Thinking => '◐',
            Self::Exploring => '◎',
            Self::ToolBash => '⚡',
            Self::ToolEdit => '✎',
            Self::ToolWrite => '✐',
            Self::ToolRead => '◈',
            Self::Subagent => '◇',
            Self::WebSearch => '◉',
            Self::WebFetch => '◈',
            Self::Planning => '◎',
            Self::TodoUpdate => '☐',
        }
    }
}

#[derive(Debug, Clone)]
pub struct TrackedPane {
    pub handle: PaneHandle,
    pub agent_type: Option<AgentType>,
    pub status: PaneStatus,
    pub output_hash: [u8; 32],
    pub last_change: Instant,
}

impl TrackedPane {
    pub fn new(handle: PaneHandle) -> Self {
        Self {
            handle,
            agent_type: None,
            status: PaneStatus::Unknown,
            output_hash: [0u8; 32],
            last_change: Instant::now(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionAggregateStatus {
    NeedsAttention,
    Working,
    Idle,
    Empty,
}

impl SessionAggregateStatus {
    pub fn from_panes(panes: &[TrackedPane]) -> Self {
        if panes.is_empty() {
            return Self::Empty;
        }

        let has_waiting = panes.iter().any(|p| p.status.is_waiting());
        let has_active = panes.iter().any(|p| p.status.is_active());

        if has_waiting {
            Self::NeedsAttention
        } else if has_active {
            Self::Working
        } else {
            Self::Idle
        }
    }

    pub fn icon(&self) -> char {
        match self {
            Self::NeedsAttention => '◉',
            Self::Working => '●',
            Self::Idle => '○',
            Self::Empty => '◌',
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::NeedsAttention => "Needs Attention",
            Self::Working => "Working",
            Self::Idle => "Idle",
            Self::Empty => "Empty",
        }
    }
}
