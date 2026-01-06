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
    /// Otherwise, specify repos with --repos or --workspace.
    New {
        /// Task name
        name: String,

        /// Branch name (defaults to feature/<name>)
        #[arg(short, long)]
        branch: Option<String>,

        /// Repo specifications (name:source:branch) for multi-repo tasks
        #[arg(short, long, value_delimiter = ',')]
        repos: Vec<String>,

        /// Use repos from a configured workspace
        #[arg(short, long)]
        workspace: Option<String>,
    },

    /// List all tasks
    #[command(visible_alias = "ls")]
    List,

    /// Delete a task
    #[command(visible_alias = "rm")]
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
    #[command(visible_alias = "a")]
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

    /// Manage workspaces
    #[command(visible_alias = "ws")]
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommands,
    },
}

#[derive(Subcommand)]
pub enum WorkspaceCommands {
    /// Add or update a workspace
    Add {
        /// Workspace name
        name: String,

        /// Repo mappings (name:path)
        #[arg(required = true)]
        repos: Vec<String>,
    },

    /// Add a repo to an existing workspace
    AddRepo {
        /// Workspace name
        workspace: String,

        /// Repo mapping (name:path)
        repo: String,
    },

    /// Remove a repo from a workspace
    RmRepo {
        /// Workspace name
        workspace: String,

        /// Repo name to remove
        repo: String,
    },

    /// List all workspaces
    #[command(visible_alias = "ls")]
    List,

    /// Remove a workspace
    #[command(visible_alias = "rm")]
    Remove {
        /// Workspace name
        name: String,
    },
}

pub fn print_completions(shell: Shell) {
    match shell {
        Shell::Zsh => print_zsh_completions(),
        _ => clap_complete::generate(shell, &mut Cli::command(), "wagner", &mut std::io::stdout()),
    }
}

fn print_zsh_completions() {
    print!(
        r#"#compdef wagner

_wagner_tasks() {{
    local -a tasks
    tasks=(${{(f)"$(wagner list 2>/dev/null | awk '{{print $1}}')"}} )
    _describe 'task' tasks
}}

_wagner_workspaces() {{
    local config_file="${{XDG_CONFIG_HOME:-$HOME/.config}}/wagner/config.json"
    if [[ -f "$config_file" ]]; then
        local -a workspaces
        workspaces=(${{(f)"$(grep -oP '"workspaces"\s*:\s*\{{[^}}]*' "$config_file" 2>/dev/null | grep -oP '"\K[^"]+(?="\s*:)' | head -20)"}} )
        _describe 'workspace' workspaces
    fi
}}

_wagner() {{
    local -a commands
    commands=(
        'new:Create a new task with worktrees'
        'list:List all tasks'
        'ls:List all tasks'
        'delete:Delete a task'
        'rm:Delete a task'
        'add:Add a new Claude pane to a task'
        'attach:Attach to a task tmux session'
        'a:Attach to a task tmux session'
        'completions:Generate shell completions'
        'workspace:Manage workspaces'
        'ws:Manage workspaces'
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
                        '*--repos=[Repo specs]:repo:' \
                        '-w[Workspace name]:workspace:_wagner_workspaces' \
                        '--workspace=[Workspace name]:workspace:_wagner_workspaces'
                    ;;
                delete|rm)
                    _arguments \
                        '1:task:_wagner_tasks' \
                        '-f[Force delete]' \
                        '--force[Force delete]'
                    ;;
                attach|a)
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
                workspace|ws)
                    local -a ws_commands
                    ws_commands=(
                        'add:Add or update a workspace'
                        'list:List all workspaces'
                        'ls:List all workspaces'
                        'remove:Remove a workspace'
                        'rm:Remove a workspace'
                    )
                    _arguments -C \
                        '1: :->ws_command' \
                        '*::arg:->ws_args'
                    case $state in
                        ws_command)
                            _describe 'workspace command' ws_commands
                            ;;
                        ws_args)
                            case $words[1] in
                                add)
                                    _arguments '1:workspace name:' '*:repo spec:'
                                    ;;
                                remove|rm)
                                    _arguments '1:workspace:_wagner_workspaces'
                                    ;;
                            esac
                            ;;
                    esac
                    ;;
            esac
            ;;
    esac
}}

compdef _wagner wagner
"#
    );
}

pub use commands::run;
