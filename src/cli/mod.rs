mod commands;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "wagner")]
#[command(about = "Multi-repo task manager for Claude Code sessions")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create a new task with worktrees
    New {
        /// Task name
        name: String,

        /// Repo specifications (name:source:branch)
        #[arg(short, long, value_delimiter = ',')]
        repos: Vec<String>,
    },

    /// List all tasks
    List,

    /// Delete a task
    Delete {
        /// Task name
        name: String,

        /// Force delete (removes branches too)
        #[arg(short, long)]
        force: bool,
    },

    /// Add a new Claude pane to a task
    Add {
        /// Task name (defaults to current directory)
        task: Option<String>,

        /// Repo name within task
        repo: Option<String>,
    },

    /// Attach to a task's tmux session
    Attach {
        /// Task name
        task: String,
    },

    /// Send a message to a session
    Send {
        /// Session identifier (task/repo)
        session: String,

        /// Message to send
        message: String,
    },

    /// List chains for a task/repo
    Chains {
        /// Task name
        task: Option<String>,

        /// Repo name
        repo: Option<String>,
    },
}

pub use commands::run;
