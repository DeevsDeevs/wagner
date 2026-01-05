mod commands;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

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

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
}

pub fn print_completions(shell: Shell) {
    match shell {
        Shell::Zsh => print_zsh_completions(),
        _ => clap_complete::generate(
            shell,
            &mut Cli::command(),
            "wagner",
            &mut std::io::stdout(),
        ),
    }
}

fn print_zsh_completions() {
    print!(r#"#compdef wagner

_wagner_tasks() {{
    local -a tasks
    tasks=(${{(f)"$(wagner list 2>/dev/null | awk '{{print $1}}')"}} )
    _describe 'task' tasks
}}

_wagner() {{
    local -a commands
    commands=(
        'new:Create a new task with worktrees'
        'list:List all tasks'
        'delete:Delete a task'
        'add:Add a new Claude pane to a task'
        'attach:Attach to a task tmux session'
        'completions:Generate shell completions'
    )

    _arguments -C \
        '1: :->command' \
        '*::arg:->args'

    case $state in
        command)
            _describe 'command' commands
            ;;
        args)
            case $words[1] in
                new)
                    _arguments \
                        '1:task name:' \
                        '-b[Branch name]:branch:' \
                        '--branch=[Branch name]:branch:' \
                        '*-r[Repo specs]:repo:' \
                        '*--repos=[Repo specs]:repo:'
                    ;;
                delete)
                    _arguments \
                        '1:task:_wagner_tasks' \
                        '-f[Force delete]' \
                        '--force[Force delete]'
                    ;;
                attach)
                    _arguments '1:task:_wagner_tasks'
                    ;;
                add)
                    _arguments \
                        '1:task:_wagner_tasks' \
                        '2:repo:'
                    ;;
                completions)
                    _arguments '1:shell:(bash zsh fish powershell elvish)'
                    ;;
            esac
            ;;
    esac
}}

compdef _wagner wagner
"#);
}

pub use commands::run;
