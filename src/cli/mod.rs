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

    /// Agent to launch (claude or codex)
    #[arg(long, global = true)]
    pub agent: Option<String>,
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

    /// Add a new agent pane to a task
    ///
    /// Auto-detects task when run from inside a task directory.
    Add {
        /// Task name (auto-detected if inside task dir)
        task: Option<String>,

        /// Repo name within task
        repo: Option<String>,

        /// Custom pane name (defaults to repo name)
        #[arg(long)]
        name: Option<String>,

        /// Agent engine (claude, codex, or terminal)
        #[arg(long)]
        agent: Option<String>,
    },

    /// Add a repo to an existing task
    AddRepo {
        /// Task name
        task: String,

        /// Repo specification (name:source:branch or name:source)
        repo: String,
    },

    /// Remove a repo from a task
    RmRepo {
        /// Task name
        task: String,

        /// Repo name to remove
        repo: String,
    },

    /// Attach to a task's tmux session
    ///
    /// Auto-detects task when run from inside a task directory.
    #[command(visible_alias = "a")]
    Attach {
        /// Task name (auto-detected if inside task dir)
        task: Option<String>,
    },

    /// Open a new shell in the task's worktree directory
    ///
    /// Spawns a subshell in the worktree. Exit with Ctrl+D or 'exit' to return.
    Cd {
        /// Task name
        task: String,

        /// Repo name (for multi-repo tasks, defaults to first repo)
        repo: Option<String>,
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

    /// Update wagner to the latest version
    Update {
        /// Check for updates without installing
        #[arg(long)]
        check: bool,
    },

    /// Clean up orphaned worktrees and task directories
    Repair {
        /// Show what would be cleaned up without making changes
        #[arg(long, default_value = "true")]
        dry_run: bool,

        /// Actually perform cleanup (use with caution)
        #[arg(long)]
        execute: bool,
    },

    /// Manage plugins
    Plugin {
        #[command(subcommand)]
        command: PluginCommands,
    },

    /// Manage chains (requires chains plugin enabled)
    Chains {
        #[command(subcommand)]
        command: ChainsCommands,
    },

    /// Launch Claude Code in the current directory
    Claude {
        /// Custom task name (defaults to directory name)
        #[arg(short, long)]
        name: Option<String>,
    },

    /// Launch Codex in the current directory
    Codex {
        /// Custom task name (defaults to directory name)
        #[arg(short, long)]
        name: Option<String>,
    },

    /// Launch a terminal pane in the current directory
    Terminal {
        /// Custom task name (defaults to directory name)
        #[arg(short, long)]
        name: Option<String>,
    },

    /// Start agent sessions on existing repos (no worktrees)
    ///
    /// Lightweight mode: manages tmux/agents without creating worktrees or branches.
    /// Auto-detects repos when run from inside a git repo or directory containing repos.
    #[command(visible_alias = "s")]
    Start {
        /// Repo paths (auto-detect if not specified)
        paths: Vec<std::path::PathBuf>,

        /// Task name (derived from repo/dir if not specified)
        #[arg(short, long)]
        name: Option<String>,
    },

    /// Stop tracking an attached task (leaves repos untouched)
    Detach {
        /// Task name (auto-detected if inside task dir)
        task: Option<String>,
    },

    /// Rename a pane within a task
    RenamePane {
        /// Task name
        task: String,

        /// Current pane name
        old_name: String,

        /// New pane name
        new_name: String,
    },

    /// Show task/pane status via daemon
    Status {
        /// Task name (shows all tasks if omitted)
        task: Option<String>,
    },

    /// Send message to a pane
    Send {
        /// Task name
        task: String,
        /// Message to send
        #[arg(trailing_var_arg = true, required = true)]
        message: Vec<String>,
        /// Target specific pane by name
        #[arg(short, long)]
        pane: Option<String>,
    },

    /// Approve tool use
    #[command(visible_alias = "y")]
    Approve {
        /// Task name (smart-pick if omitted)
        task: Option<String>,
        /// Pane name
        pane: Option<String>,
    },

    /// Reject tool use
    #[command(visible_alias = "n")]
    Reject {
        /// Task name
        task: String,
        /// Pane name
        pane: Option<String>,
    },

    /// Capture pane output
    Output {
        /// Task name
        task: String,
        /// Pane name
        pane: Option<String>,
        /// Number of lines
        #[arg(short, long)]
        lines: Option<usize>,
    },

    /// Resume a dead agent
    Resume {
        /// Task name
        task: String,
        /// Pane name
        pane: Option<String>,
    },

    /// Run the daemon for remote monitoring
    Daemon {
        #[command(subcommand)]
        command: DaemonCommands,
    },
}

#[derive(Subcommand)]
pub enum DaemonCommands {
    /// Start the daemon (foreground)
    Start,
    /// Stop a running daemon
    Stop,
    /// Stop and restart the daemon
    Restart,
    /// Check if daemon is running
    Status,
}

#[derive(Subcommand)]
pub enum PluginCommands {
    /// List all available plugins
    #[command(visible_alias = "ls")]
    List,

    /// Enable a plugin
    Enable {
        /// Plugin ID (e.g., chains)
        plugin: String,
    },

    /// Disable a plugin
    Disable {
        /// Plugin ID
        plugin: String,
    },

    /// Install agent skills for enabled plugins
    InstallSkills,
}

#[derive(Subcommand)]
pub enum ChainsCommands {
    /// List all chains
    #[command(visible_alias = "ls")]
    List,

    /// Promote a task-local chain to repo level
    Promote {
        /// Chain name to promote
        chain: String,

        /// Task name (auto-detected if inside task dir)
        #[arg(short, long)]
        task: Option<String>,
    },

    /// Show chain content
    Show {
        /// Chain name
        chain: String,

        /// Link index (latest if not specified)
        #[arg(short, long)]
        link: Option<usize>,
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

        /// Base branch for diffs (defaults to main)
        #[arg(short, long)]
        base_branch: Option<String>,
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

_wagner_task_repos() {{
    local task="$words[2]"
    [[ -z "$task" ]] && return
    local config_file="${{XDG_CONFIG_HOME:-$HOME/.config}}/wagner/config.json"
    local tasks_root="$(grep -oP '"tasks_root"\s*:\s*"\K[^"]+' "$config_file" 2>/dev/null)"
    tasks_root="${{tasks_root:-$HOME/.wagner/tasks}}"
    tasks_root="${{tasks_root/#\~/$HOME}}"
    local task_file="$tasks_root/$task/.wagner/task.json"
    if [[ -f "$task_file" ]]; then
        local -a repos
        repos=(${{(f)"$(grep -oP '"name"\s*:\s*"\K[^"]+' "$task_file" 2>/dev/null | head -20)"}} )
        _describe 'repo' repos
    fi
}}

_wagner_plugins() {{
    local -a plugins
    plugins=('chains:Session workflow chains for context across conversations')
    _describe 'plugin' plugins
}}

_wagner_chains() {{
    local -a chains
    chains=(${{(f)"$(wagner chains list 2>/dev/null | grep -E '^\s+\S' | awk '{{print $1}}')"}} )
    _describe 'chain' chains
}}

_wagner() {{
    local -a commands
    commands=(
        'new:Create a new task with worktrees'
        'list:List all tasks'
        'ls:List all tasks'
        'delete:Delete a task'
        'rm:Delete a task'
        'add:Add a new agent pane to a task'
        'add-repo:Add a repo to a task'
        'rm-repo:Remove a repo from a task'
        'attach:Attach to a task tmux session'
        'a:Attach to a task tmux session'
        'cd:Open shell in task worktree'
        'completions:Generate shell completions'
        'workspace:Manage workspaces'
        'ws:Manage workspaces'
        'update:Update wagner to latest version'
        'repair:Clean up orphaned worktrees'
        'plugin:Manage plugins'
        'chains:Manage chains (requires chains plugin)'
        'claude:Launch Claude Code in the current directory'
        'codex:Launch Codex in the current directory'
        'terminal:Launch a terminal pane in the current directory'
        'start:Start agent sessions on existing repos'
        's:Start agent sessions on existing repos'
        'detach:Stop tracking an attached task'
        'status:Show task/pane status via daemon'
        'send:Send message to a pane'
        'approve:Approve tool use'
        'y:Approve tool use'
        'reject:Reject tool use'
        'n:Reject tool use'
        'output:Capture pane output'
        'resume:Resume a dead agent'
        'daemon:Run the daemon for remote monitoring'
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
                cd)
                    _arguments '1:task:_wagner_tasks' '2:repo:_wagner_task_repos'
                    ;;
                add)
                    _arguments \
                        '1:task:_wagner_tasks' \
                        '2:repo:_wagner_task_repos'
                    ;;
                add-repo)
                    _arguments \
                        '1:task:_wagner_tasks' \
                        '2:repo spec:'
                    ;;
                rm-repo)
                    _arguments \
                        '1:task:_wagner_tasks' \
                        '2:repo:_wagner_task_repos'
                    ;;
                completions)
                    _arguments '1:shell:(bash zsh fish powershell elvish)'
                    ;;
                update)
                    _arguments '--check[Check for updates without installing]'
                    ;;
                workspace|ws)
                    local -a ws_commands
                    ws_commands=(
                        'add:Add or update a workspace'
                        'add-repo:Add a repo to a workspace'
                        'rm-repo:Remove a repo from a workspace'
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
                                    _arguments \
                                        '1:workspace name:' \
                                        '-b[Base branch]:branch:' \
                                        '--base-branch=[Base branch]:branch:' \
                                        '*:repo spec:'
                                    ;;
                                add-repo)
                                    _arguments '1:workspace:_wagner_workspaces' '2:repo spec:'
                                    ;;
                                rm-repo)
                                    _arguments '1:workspace:_wagner_workspaces' '2:repo name:'
                                    ;;
                                remove|rm)
                                    _arguments '1:workspace:_wagner_workspaces'
                                    ;;
                            esac
                            ;;
                    esac
                    ;;
                plugin)
                    local -a plugin_commands
                    plugin_commands=(
                        'list:List all available plugins'
                        'ls:List all available plugins'
                        'enable:Enable a plugin'
                        'disable:Disable a plugin'
                        'install-skills:Install agent skills for enabled plugins'
                    )
                    _arguments -C \
                        '1: :->plugin_command' \
                        '*::arg:->plugin_args'
                    case $state in
                        plugin_command)
                            _describe 'plugin command' plugin_commands
                            ;;
                        plugin_args)
                            case $words[1] in
                                enable|disable)
                                    _arguments '1:plugin:_wagner_plugins'
                                    ;;
                            esac
                            ;;
                    esac
                    ;;
                chains)
                    local -a chains_commands
                    chains_commands=(
                        'list:List all chains'
                        'ls:List all chains'
                        'promote:Promote a task-local chain to repo level'
                        'show:Show chain content'
                    )
                    _arguments -C \
                        '1: :->chains_command' \
                        '*::arg:->chains_args'
                    case $state in
                        chains_command)
                            _describe 'chains command' chains_commands
                            ;;
                        chains_args)
                            case $words[1] in
                                promote)
                                    _arguments \
                                        '1:chain:_wagner_chains' \
                                        '-t[Task name]:task:_wagner_tasks' \
                                        '--task=[Task name]:task:_wagner_tasks'
                                    ;;
                                show)
                                    _arguments \
                                        '1:chain:_wagner_chains' \
                                        '-l[Link index]:link:' \
                                        '--link=[Link index]:link:'
                                    ;;
                            esac
                            ;;
                    esac
                    ;;
                claude|codex|terminal)
                    _arguments \
                        '-n[Task name]:name:' \
                        '--name=[Task name]:name:'
                    ;;
                start|s)
                    _arguments \
                        '*:repo path:_files -/' \
                        '-n[Task name]:name:' \
                        '--name=[Task name]:name:'
                    ;;
                detach)
                    _arguments '1:task:_wagner_tasks'
                    ;;
                status)
                    _arguments '1:task:_wagner_tasks'
                    ;;
                send)
                    _arguments \
                        '1:task:_wagner_tasks' \
                        '-p[Pane name]:pane:' \
                        '--pane=[Pane name]:pane:' \
                        '*:message:'
                    ;;
                approve|y)
                    _arguments '1:task:_wagner_tasks' '2:pane:'
                    ;;
                reject|n)
                    _arguments '1:task:_wagner_tasks' '2:pane:'
                    ;;
                output)
                    _arguments \
                        '1:task:_wagner_tasks' \
                        '2:pane:' \
                        '-l[Number of lines]:lines:' \
                        '--lines=[Number of lines]:lines:'
                    ;;
                resume)
                    _arguments '1:task:_wagner_tasks' '2:pane:'
                    ;;
                daemon)
                    local -a daemon_commands
                    daemon_commands=(
                        'start:Start the daemon (foreground)'
                        'stop:Stop a running daemon'
                        'restart:Stop and restart the daemon'
                        'status:Check if daemon is running'
                    )
                    _describe 'daemon command' daemon_commands
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
