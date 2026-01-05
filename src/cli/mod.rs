mod commands;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "wagner")]
#[command(about = "Multi-repo task manager for agents sessions")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create a new task with worktrees
    ///
    /// When run inside a git repo, automatically uses that repo.
    /// Otherwise, specify repos with --repos.
    New {
        /// Task name
        name: String,

        /// Branch name (defaults to task/<name>)
        #[arg(short, long)]
        branch: Option<String>,

        /// Repo specifications (name:source:branch) for multi-repo tasks
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
    ///
    /// Auto-detects task when run from inside a task directory.
    Add {
        /// Task name (auto-detected if inside task dir)
        task: Option<String>,

        /// Repo name within task
        repo: Option<String>,
    },

    /// Attach to a task's tmux session
    ///
    /// Auto-detects task when run from inside a task directory.
    Attach {
        /// Task name (auto-detected if inside task dir)
        task: Option<String>,
    },

}

pub use commands::run;
