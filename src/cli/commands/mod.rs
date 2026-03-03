use crate::cli::{
    ChainsCommands, Cli, Commands, DaemonCommands, PluginCommands, WorkspaceCommands,
    print_completions,
};
use std::path::PathBuf;
use tracing::{debug, info};
use wagner::{
    Agent, AgentChoice, AttachDetection, Config, Engine, RepoSource, RepoSpec, Result, Terminal,
    Tmux, Wagner, default_branch_for_task, derive_task_name, detect_attach_mode, plugins,
};

pub fn run(cli: Cli) -> Result<()> {
    if let Some(Commands::Completions { shell }) = &cli.command {
        print_completions(*shell);
        return Ok(());
    }

    let config = Config::load()?;
    debug!("Loaded config from {:?}", Config::config_path());

    if let Some(Commands::Daemon { command }) = cli.command {
        return cmd_daemon(command, config);
    }
    if let Some(ref cmd) = cli.command
        && let Some(result) = try_ipc_command(cmd)
    {
        return result;
    }

    let terminal = Tmux::with_config(config.terminal.clone());
    let agent_key = cli.agent.as_deref().unwrap_or(&config.default_agent);
    let agent = AgentChoice::from_key(agent_key)?;
    let wagner = Wagner::new(terminal, agent, config);

    match cli.command {
        Some(Commands::New {
            name,
            branch,
            repos,
            workspace,
        }) => cmd_new(
            &wagner,
            &name,
            branch.as_deref(),
            &repos,
            workspace.as_deref(),
        ),
        Some(Commands::List) => cmd_list(&wagner),
        Some(Commands::Delete { name, force }) => cmd_delete(&wagner, &name, force),
        Some(Commands::Add { .. }) => unreachable!(),
        Some(Commands::RenamePane {
            task,
            old_name,
            new_name,
        }) => cmd_rename_pane(&wagner, &task, &old_name, &new_name),
        Some(Commands::AddRepo { task, repo }) => cmd_add_repo(&wagner, &task, &repo),
        Some(Commands::RmRepo { task, repo }) => cmd_rm_repo(&wagner, &task, &repo),
        Some(Commands::Attach { task }) => cmd_attach(&wagner, task),
        Some(Commands::Cd { task, repo }) => cmd_cd(&wagner, &task, repo.as_deref()),
        Some(Commands::Completions { .. }) => unreachable!(),
        Some(Commands::Workspace { command }) => cmd_workspace(command),
        Some(Commands::Update { check }) => cmd_update(check),
        Some(Commands::Repair { dry_run, execute }) => {
            cmd_repair(&wagner.config, !execute || dry_run)
        }
        Some(Commands::Plugin { command }) => cmd_plugin(command),
        Some(Commands::Chains { command }) => cmd_chains(&wagner, command),
        Some(Commands::Claude { name }) => cmd_quick_launch(&wagner, Engine::ClaudeCode, name),
        Some(Commands::Codex { name }) => cmd_quick_launch(&wagner, Engine::Codex, name),
        Some(Commands::Terminal { name }) => cmd_quick_launch(&wagner, Engine::Terminal, name),
        Some(Commands::Start { paths, name }) => cmd_start(&wagner, paths, name),
        Some(Commands::Detach { task }) => cmd_detach(&wagner, task),
        Some(Commands::Daemon { .. }) => unreachable!(),
        Some(Commands::Status { .. })
        | Some(Commands::Send { .. })
        | Some(Commands::Approve { .. })
        | Some(Commands::Reject { .. })
        | Some(Commands::Output { .. })
        | Some(Commands::Resume { .. }) => unreachable!(),
        None => cmd_tui(wagner),
    }
}

fn cmd_new<T: Terminal, A: Agent>(
    wagner: &Wagner<T, A>,
    name: &str,
    branch: Option<&str>,
    repos: &[String],
    workspace: Option<&str>,
) -> Result<()> {
    let default_branch = branch
        .map(String::from)
        .unwrap_or_else(|| default_branch_for_task(name));

    let (specs, base_branch): (Vec<RepoSpec>, Option<String>) = if let Some(ws_name) = workspace {
        let ws = wagner.config.workspaces.get(ws_name).unwrap_or_else(|| {
            eprintln!("Error: Workspace '{}' not found in config", ws_name);
            eprintln!(
                "Configure workspaces in: {}",
                Config::config_path().display()
            );
            std::process::exit(1);
        });

        let specs = ws
            .repos
            .iter()
            .map(|(repo_name, path)| RepoSpec {
                name: repo_name.clone(),
                source: RepoSource::Local(shellexpand::tilde(path).into_owned().into()),
                branch: default_branch.clone(),
            })
            .collect();
        (specs, Some(ws.base_branch.clone()))
    } else if repos.is_empty() {
        match detect_git_repo() {
            Some((repo_path, repo_name)) => {
                debug!(repo = %repo_name, branch = %default_branch, "Auto-detected repo");
                (
                    vec![RepoSpec {
                        name: repo_name,
                        source: RepoSource::Local(repo_path),
                        branch: default_branch.clone(),
                    }],
                    None,
                )
            }
            None => {
                eprintln!("Error: Not in a git repository");
                eprintln!("Either run from inside a git repo, or specify --repos or --workspace");
                eprintln!("Usage: wagner new <name> --repos name:path:branch");
                std::process::exit(1);
            }
        }
    } else {
        (
            repos
                .iter()
                .map(|s| RepoSpec::parse(s, Some(&default_branch)))
                .collect::<Result<Vec<_>>>()?,
            None,
        )
    };

    debug!("Creating task '{}' with {} repos", name, specs.len());

    let task = wagner.create_task(name, &specs, base_branch.as_deref())?;

    info!(task = %task.name, path = %task.path.display(), "Task created");

    println!("Created task: {}", task.name);
    println!("  Path: {}", task.path.display());
    for repo in &task.repos {
        println!(
            "  {} ({}) -> {}",
            repo.name,
            repo.branch,
            repo.worktree.display()
        );
    }
    println!();
    println!("Run: wagner attach {}", task.name);

    Ok(())
}

fn detect_git_repo() -> Option<(std::path::PathBuf, String)> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let repo_path = std::path::PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());

    let repo_name = repo_path.file_name()?.to_string_lossy().to_string();

    Some((repo_path, repo_name))
}

fn detect_task_from_cwd(config: &Config) -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let tasks_root = &config.tasks_root;

    if !cwd.starts_with(tasks_root) {
        return None;
    }

    let relative = cwd.strip_prefix(tasks_root).ok()?;
    let task_name = relative.components().next()?;

    let task_dir = tasks_root.join(task_name);
    if task_dir.join(".wagner").join("task.json").exists() {
        Some(task_name.as_os_str().to_string_lossy().to_string())
    } else {
        None
    }
}

fn cmd_list<T: Terminal, A: Agent>(wagner: &Wagner<T, A>) -> Result<()> {
    let tasks = wagner.list_tasks()?;
    debug!("Found {} tasks", tasks.len());

    if tasks.is_empty() {
        println!("No tasks found");
        println!("Create one with: wagner new <name>");
        println!("Or attach to existing repos: wagner start");
        return Ok(());
    }

    let now = chrono::Utc::now();
    for task in tasks {
        let repos_count = task.repos.len();
        let repos_label = if repos_count == 1 { "repo" } else { "repos" };
        let age = now.signed_duration_since(task.created_at);
        let created = match age.num_days() {
            0 => format!("{}h ago", age.num_hours().max(1)),
            1..=6 => format!("{}d ago", age.num_days()),
            _ => task.created_at.format("%Y-%m-%d").to_string(),
        };

        let kind_indicator = if task.is_attached() { "[A] " } else { "" };

        println!(
            "{}{:<20} {} {}  ({})  {}",
            kind_indicator,
            task.name,
            repos_count,
            repos_label,
            created,
            task.path.display()
        );
    }

    Ok(())
}

fn cmd_delete<T: Terminal, A: Agent>(wagner: &Wagner<T, A>, name: &str, force: bool) -> Result<()> {
    debug!(task = %name, force = %force, "Deleting task");

    if !force {
        println!("Delete task '{}'? This will remove worktrees.", name);
        println!("Use --force to also delete branches.");
        print!("Continue? [y/N] ");

        use std::io::{self, Write};
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            info!("Delete cancelled by user");
            println!("Cancelled");
            return Ok(());
        }
    }

    wagner.delete_task(name, force)?;
    info!(task = %name, "Task deleted");
    println!("Deleted task: {}", name);

    Ok(())
}

fn cmd_rename_pane<T: Terminal, A: Agent>(
    wagner: &Wagner<T, A>,
    task_name: &str,
    old_name: &str,
    new_name: &str,
) -> Result<()> {
    let mut task = wagner.store.load_task(task_name)?;
    if task.rename_pane(old_name, new_name) {
        wagner.store.save_task(&task)?;
        println!(
            "Renamed pane '{}' to '{}' in task '{}'",
            old_name, new_name, task_name
        );
        Ok(())
    } else {
        Err(wagner::WagnerError::Terminal(format!(
            "Cannot rename: pane '{}' not found or '{}' already exists",
            old_name, new_name
        )))
    }
}

fn cmd_add_repo<T: Terminal, A: Agent>(
    wagner: &Wagner<T, A>,
    task: &str,
    repo: &str,
) -> Result<()> {
    let task_data = wagner.get_task(task)?;
    let default_branch = task_data
        .repos
        .first()
        .map(|r| r.branch.clone())
        .unwrap_or_else(|| default_branch_for_task(task));

    let spec = RepoSpec::parse(repo, Some(&default_branch))?;
    debug!(task = %task, repo = %spec.name, "Adding repo to task");

    wagner.add_repo_to_task(task, &spec)?;
    info!(task = %task, repo = %spec.name, "Repo added");
    println!("Added repo {} to task {}", spec.name, task);

    Ok(())
}

fn cmd_rm_repo<T: Terminal, A: Agent>(wagner: &Wagner<T, A>, task: &str, repo: &str) -> Result<()> {
    debug!(task = %task, repo = %repo, "Removing repo from task");

    wagner.remove_repo_from_task(task, repo)?;
    info!(task = %task, repo = %repo, "Repo removed");
    println!("Removed repo {} from task {}", repo, task);

    Ok(())
}

fn cmd_attach<T: Terminal, A: Agent>(wagner: &Wagner<T, A>, task: Option<String>) -> Result<()> {
    let task_name = task
        .or_else(|| detect_task_from_cwd(&wagner.config))
        .unwrap_or_else(|| {
            eprintln!("Error: Not inside a task directory");
            eprintln!("Either cd into a task, or specify: wagner attach <task>");
            std::process::exit(1);
        });

    debug!(task = %task_name, "Attaching to session");
    match wagner.resume_dead_agents(&task_name) {
        Ok(n) if n > 0 => info!(count = n, "Resumed dead agents"),
        Err(e) => debug!(%e, "Resume check skipped"),
        _ => {}
    }
    wagner.attach(&task_name, None)
}

fn cmd_quick_launch<T: Terminal, A: Agent>(
    wagner: &Wagner<T, A>,
    engine: Engine,
    name: Option<String>,
) -> Result<()> {
    wagner.quick_launch(engine, name.as_deref())
}

fn cmd_start<T: Terminal, A: Agent>(
    wagner: &Wagner<T, A>,
    paths: Vec<PathBuf>,
    name: Option<String>,
) -> Result<()> {
    let detection = detect_attach_mode(&paths);

    let repo_paths = match &detection {
        AttachDetection::SingleRepo(p) => vec![p.clone()],
        AttachDetection::MultiRepo(ps) => ps.clone(),
        AttachDetection::NoRepos => {
            eprintln!("Error: Could not determine current directory");
            eprintln!("Specify paths explicitly: wagner start ~/project1 ~/project2");
            std::process::exit(1);
        }
    };

    let task_name = name.unwrap_or_else(|| derive_task_name(&detection));

    debug!(task = %task_name, repos = repo_paths.len(), "Starting attached task");

    let task = wagner.attach_task(&task_name, repo_paths)?;

    info!(task = %task.name, "Attached task started");

    println!("Started task: {}", task.name);
    println!("  Path: {}", task.path.display());
    for repo in &task.repos {
        println!("  {} ({})", repo.name, repo.branch);
    }
    println!();
    println!("Run: wagner attach {}", task.name);
    println!("To stop: wagner detach {}", task.name);

    Ok(())
}

fn cmd_detach<T: Terminal, A: Agent>(wagner: &Wagner<T, A>, task: Option<String>) -> Result<()> {
    let task_name = task
        .or_else(|| detect_task_from_cwd(&wagner.config))
        .unwrap_or_else(|| {
            eprintln!("Error: Not inside a task directory");
            eprintln!("Either cd into a task, or specify: wagner detach <task>");
            std::process::exit(1);
        });

    debug!(task = %task_name, "Detaching task");

    wagner.detach_task(&task_name)?;
    info!(task = %task_name, "Task detached");
    println!("Detached task: {}", task_name);

    Ok(())
}

fn cmd_cd<T: Terminal, A: Agent>(
    wagner: &Wagner<T, A>,
    task_name: &str,
    repo: Option<&str>,
) -> Result<()> {
    let task = wagner.get_task(task_name)?;

    let worktree = if let Some(repo_name) = repo {
        task.repos
            .iter()
            .find(|r| r.name == repo_name)
            .map(|r| &r.worktree)
            .unwrap_or_else(|| {
                eprintln!("Repo '{}' not found in task '{}'", repo_name, task_name);
                eprintln!(
                    "Available repos: {}",
                    task.repos
                        .iter()
                        .map(|r| r.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                std::process::exit(1);
            })
    } else {
        task.repos.first().map(|r| &r.worktree).unwrap_or_else(|| {
            eprintln!("Task '{}' has no repos", task_name);
            std::process::exit(1);
        })
    };

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    std::process::Command::new(&shell)
        .current_dir(worktree)
        .status()?;

    Ok(())
}

fn cmd_tui<T: Terminal + 'static, A: Agent + 'static>(wagner: Wagner<T, A>) -> Result<()> {
    info!("Launching TUI");
    wagner::tui::run(wagner)
}

fn cmd_repair(config: &Config, dry_run: bool) -> Result<()> {
    println!(
        "{}",
        if dry_run {
            "Scanning for orphaned resources (dry run)..."
        } else {
            "Scanning and cleaning orphaned resources..."
        }
    );

    let mut found_issues = false;

    if config.tasks_root.exists() {
        for entry in std::fs::read_dir(&config.tasks_root)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            let task_json = path.join(".wagner").join("task.json");
            if !task_json.exists() {
                found_issues = true;
                println!("  Orphaned task directory: {}", path.display());

                if !dry_run {
                    cleanup_orphaned_dir(&path);
                    println!("    -> Removed");
                }
            }
        }
    }

    for (ws_name, ws) in &config.workspaces {
        for (repo_name, repo_path) in &ws.repos {
            let expanded = shellexpand::tilde(repo_path).into_owned();
            let repo_path = std::path::PathBuf::from(&expanded);

            if !repo_path.exists() {
                continue;
            }

            let output = std::process::Command::new("git")
                .args([
                    "-C",
                    &repo_path.to_string_lossy(),
                    "worktree",
                    "list",
                    "--porcelain",
                ])
                .output();

            if let Ok(output) = output
                && output.status.success()
            {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    if let Some(wt_path) = line.strip_prefix("worktree ") {
                        let wt_path = std::path::PathBuf::from(wt_path);

                        if wt_path.starts_with(&config.tasks_root)
                            && let Some(task_dir) = wt_path.parent()
                        {
                            let task_json = task_dir.join(".wagner").join("task.json");
                            if !task_json.exists() && task_dir != config.tasks_root {
                                found_issues = true;
                                println!(
                                    "  Orphaned worktree in {}/{}: {}",
                                    ws_name,
                                    repo_name,
                                    wt_path.display()
                                );

                                if !dry_run {
                                    let _ = std::process::Command::new("git")
                                        .args([
                                            "-C",
                                            &repo_path.to_string_lossy(),
                                            "worktree",
                                            "remove",
                                            "--force",
                                            &wt_path.to_string_lossy(),
                                        ])
                                        .output();
                                    println!("    -> Removed");
                                }
                            }
                        }
                    }
                }

                if !dry_run {
                    let _ = std::process::Command::new("git")
                        .args(["-C", &repo_path.to_string_lossy(), "worktree", "prune"])
                        .output();
                }
            }
        }
    }

    if !found_issues {
        println!("No orphaned resources found.");
    } else if dry_run {
        println!("\nRun `wagner repair --execute` to clean up these resources.");
    } else {
        println!("\nCleanup complete.");
    }

    Ok(())
}

fn cleanup_orphaned_dir(path: &std::path::Path) {
    for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
        let subpath = entry.path();
        let git_file = subpath.join(".git");
        if subpath.is_dir()
            && git_file.exists()
            && git_file.is_file()
            && let Ok(content) = std::fs::read_to_string(&git_file)
            && let Some(gitdir) = content.strip_prefix("gitdir: ")
        {
            let gitdir = gitdir.trim();
            let gitdir_path = std::path::PathBuf::from(gitdir);
            if let Some(worktrees_dir) = gitdir_path.parent()
                && let Some(git_or_repo) = worktrees_dir.parent()
            {
                let main_repo = if git_or_repo
                    .file_name()
                    .map(|n| n == ".git")
                    .unwrap_or(false)
                {
                    git_or_repo.parent().map(|p| p.to_path_buf())
                } else {
                    Some(git_or_repo.to_path_buf())
                };

                if let Some(main_repo) = main_repo {
                    let _ = std::process::Command::new("git")
                        .args([
                            "-C",
                            &main_repo.to_string_lossy(),
                            "worktree",
                            "remove",
                            "--force",
                            &subpath.to_string_lossy(),
                        ])
                        .output();
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(path);
}

const REPO: &str = "DeevsDeevs/wagner";
const BINARY_NAME: &str = "wagner";

fn cmd_update(check_only: bool) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");

    println!("Checking for updates...");

    let latest = get_latest_version()?;

    if latest == current_version {
        println!("wagner is up to date (v{})", current_version);
        return Ok(());
    }

    println!("Current version: v{}", current_version);
    println!("Latest version:  v{}", latest);

    if check_only {
        println!("\nRun `wagner update` to install the latest version.");
        return Ok(());
    }

    println!("\nUpdating...");

    let platform = detect_platform()?;
    download_and_install(&latest, &platform)?;

    println!("\nwagner updated to v{}", latest);

    Ok(())
}

fn get_latest_version() -> Result<String> {
    let output = std::process::Command::new("curl")
        .args([
            "-fsSL",
            &format!("https://api.github.com/repos/{}/releases/latest", REPO),
        ])
        .output()?;

    if !output.status.success() {
        return Err(wagner::WagnerError::Update(
            "Failed to fetch latest version from GitHub".into(),
        ));
    }

    let body = String::from_utf8_lossy(&output.stdout);
    let version = body
        .lines()
        .find(|line| line.contains("\"tag_name\""))
        .and_then(|line| {
            let start = line.find('"')? + 1;
            let rest = &line[start..];
            let end = rest.find('"')?;
            let rest = &rest[end + 1..];
            let start = rest.find('"')? + 1;
            let rest = &rest[start..];
            let end = rest.find('"')?;
            Some(rest[..end].trim_start_matches('v').to_string())
        })
        .ok_or_else(|| {
            wagner::WagnerError::Update("Failed to parse version from GitHub response".into())
        })?;

    Ok(version)
}

fn detect_platform() -> Result<String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    let os_str = match os {
        "linux" => "linux",
        "macos" => "darwin",
        _ => {
            return Err(wagner::WagnerError::Update(format!(
                "Unsupported OS: {}",
                os
            )));
        }
    };

    let arch_str = match arch {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        _ => {
            return Err(wagner::WagnerError::Update(format!(
                "Unsupported architecture: {}",
                arch
            )));
        }
    };

    Ok(format!("{}-{}-{}", BINARY_NAME, os_str, arch_str))
}

fn download_and_install(version: &str, platform: &str) -> Result<()> {
    let current_exe = std::env::current_exe()?;
    let install_dir = current_exe
        .parent()
        .unwrap_or(std::path::Path::new("/usr/local/bin"));

    let download_url = format!(
        "https://github.com/{}/releases/download/v{}/{}.tar.gz",
        REPO, version, platform
    );

    println!("Downloading {}...", download_url);

    let tmpdir = std::env::temp_dir().join(format!("wagner-update-{}", std::process::id()));
    std::fs::create_dir_all(&tmpdir)?;

    let tarball = tmpdir.join(format!("{}.tar.gz", platform));
    let status = std::process::Command::new("curl")
        .args(["-fsSL", &download_url, "-o"])
        .arg(&tarball)
        .status()?;

    if !status.success() {
        std::fs::remove_dir_all(&tmpdir).ok();
        return Err(wagner::WagnerError::Update(
            "Failed to download release".into(),
        ));
    }

    println!("Extracting...");
    let status = std::process::Command::new("tar")
        .args(["-xzf"])
        .arg(&tarball)
        .arg("-C")
        .arg(&tmpdir)
        .status()?;

    if !status.success() {
        std::fs::remove_dir_all(&tmpdir).ok();
        return Err(wagner::WagnerError::Update(
            "Failed to extract release".into(),
        ));
    }

    let new_binary = tmpdir.join(BINARY_NAME);
    let target = install_dir.join(BINARY_NAME);

    println!("Installing to {}...", target.display());

    if target.exists() {
        std::fs::remove_file(&target)?;
    }
    std::fs::copy(&new_binary, &target)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))?;
    }

    std::fs::remove_dir_all(&tmpdir).ok();

    Ok(())
}

fn cmd_workspace(command: WorkspaceCommands) -> Result<()> {
    let mut config = Config::load()?;

    match command {
        WorkspaceCommands::Add {
            name,
            repos,
            base_branch,
        } => {
            let mut ws = wagner::config::Workspace::default();
            if let Some(base) = base_branch {
                ws.base_branch = base;
            }

            for spec in repos {
                let parts: Vec<&str> = spec.splitn(2, ':').collect();
                if parts.len() != 2 {
                    eprintln!("Invalid repo spec: {}", spec);
                    eprintln!("Expected format: name:path");
                    std::process::exit(1);
                }
                ws.repos.insert(parts[0].to_string(), parts[1].to_string());
            }

            config.workspaces.insert(name.clone(), ws);
            config.save()?;

            let ws = &config.workspaces[&name];
            println!("Added workspace: {} (base: {})", name, ws.base_branch);
            for (repo_name, path) in &ws.repos {
                println!("  {}: {}", repo_name, path);
            }
        }
        WorkspaceCommands::AddRepo { workspace, repo } => {
            let parts: Vec<&str> = repo.splitn(2, ':').collect();
            if parts.len() != 2 {
                eprintln!("Invalid repo spec: {}", repo);
                eprintln!("Expected format: name:path");
                std::process::exit(1);
            }

            let ws = config.workspaces.entry(workspace.clone()).or_default();
            ws.repos.insert(parts[0].to_string(), parts[1].to_string());
            config.save()?;

            println!("Added {} to workspace {}", parts[0], workspace);
        }
        WorkspaceCommands::RmRepo { workspace, repo } => {
            let ws = config.workspaces.get_mut(&workspace).unwrap_or_else(|| {
                eprintln!("Workspace '{}' not found", workspace);
                std::process::exit(1);
            });

            if ws.repos.remove(&repo).is_none() {
                eprintln!("Repo '{}' not found in workspace '{}'", repo, workspace);
                std::process::exit(1);
            }

            config.save()?;
            println!("Removed {} from workspace {}", repo, workspace);
        }
        WorkspaceCommands::List => {
            if config.workspaces.is_empty() {
                println!("No workspaces configured");
                println!("Add one with: wagner workspace add <name> repo:path ...");
                return Ok(());
            }

            for (name, ws) in &config.workspaces {
                println!("{} (base: {})", name, ws.base_branch);
                for (repo_name, path) in &ws.repos {
                    println!("  {}: {}", repo_name, path);
                }
            }
        }
        WorkspaceCommands::Remove { name } => {
            if config.workspaces.remove(&name).is_none() {
                eprintln!("Workspace '{}' not found", name);
                std::process::exit(1);
            }

            config.save()?;
            println!("Removed workspace: {}", name);
        }
    }

    Ok(())
}

fn cmd_chains<T: Terminal, A: Agent>(wagner: &Wagner<T, A>, command: ChainsCommands) -> Result<()> {
    use wagner::plugins::chains;

    if !wagner.config.plugins.chains.enabled {
        eprintln!("Error: Chains plugin is not enabled");
        eprintln!("Enable it with: wagner plugin enable chains");
        std::process::exit(1);
    }

    match command {
        ChainsCommands::List => {
            let data = chains::load_all_chains(&wagner.config.tasks_root, None)?;

            if data.total_chains() == 0 {
                println!("No chains found");
                println!("Create one with: /chain-link <name>");
                return Ok(());
            }

            for repo in &data.repos {
                println!("{} (repo)", repo.repo_name);
                for chain in &repo.chains {
                    let link_count = chain.link_count();
                    let link_label = if link_count == 1 { "link" } else { "links" };
                    let latest = chain
                        .latest_link()
                        .map(|l| l.timestamp.as_str())
                        .unwrap_or("");
                    println!(
                        "  {} [{} {}] {}",
                        chain.name, link_count, link_label, latest
                    );
                }
            }

            if !data.task_local.is_empty() {
                println!("\nTask-local (not synced)");
                for chain in &data.task_local {
                    let link_count = chain.link_count();
                    let link_label = if link_count == 1 { "link" } else { "links" };
                    println!("  {} [{} {}]", chain.name, link_count, link_label);
                }
            }
        }
        ChainsCommands::Promote { chain, task } => {
            let task_name = task
                .or_else(|| detect_task_from_cwd(&wagner.config))
                .unwrap_or_else(|| {
                    eprintln!("Error: Not inside a task directory");
                    eprintln!("Either cd into a task, or specify: wagner chains promote <chain> --task <task>");
                    std::process::exit(1);
                });

            let task_path = wagner.config.tasks_root.join(&task_name);
            let local_chain_dir = task_path.join(".claude").join("chains").join(&chain);

            if !local_chain_dir.exists() {
                eprintln!("Error: Chain '{}' not found in task '{}'", chain, task_name);
                std::process::exit(1);
            }

            if local_chain_dir.is_symlink() {
                eprintln!(
                    "Error: Chain '{}' is already at repo level (symlinked)",
                    chain
                );
                std::process::exit(1);
            }

            let plugins_link = task_path.join(".wagner").join("plugins");
            if !plugins_link.exists() || !plugins_link.is_symlink() {
                eprintln!(
                    "Error: Task '{}' doesn't have repo-level plugin storage set up",
                    task_name
                );
                eprintln!("This task may have been created before the chains plugin was enabled");
                std::process::exit(1);
            }

            let repo_chains_dir = if let Ok(target) = std::fs::read_link(&plugins_link) {
                if target.is_absolute() {
                    target.join("chains")
                } else {
                    plugins_link.parent().unwrap().join(&target).join("chains")
                }
            } else {
                eprintln!("Error: Could not resolve repo plugins directory");
                std::process::exit(1);
            };

            let target_chain_dir = repo_chains_dir.join(&chain);
            if target_chain_dir.exists() {
                eprintln!("Error: Chain '{}' already exists at repo level", chain);
                std::process::exit(1);
            }

            std::fs::create_dir_all(&repo_chains_dir)?;
            std::fs::rename(&local_chain_dir, &target_chain_dir)?;

            println!("Promoted chain '{}' to repo level", chain);
            println!("  From: {}", local_chain_dir.display());
            println!("  To:   {}", target_chain_dir.display());
        }
        ChainsCommands::Show { chain, link } => {
            let data = chains::load_all_chains(&wagner.config.tasks_root, None)?;

            let found_chain = data.all_chains().find(|c| c.name == chain);

            let chain_data = found_chain.unwrap_or_else(|| {
                eprintln!("Error: Chain '{}' not found", chain);
                std::process::exit(1);
            });

            let link_data = if let Some(idx) = link {
                chain_data.links.get(idx).unwrap_or_else(|| {
                    eprintln!("Error: Link {} not found in chain '{}'", idx, chain);
                    eprintln!("Chain has {} links", chain_data.links.len());
                    std::process::exit(1);
                })
            } else {
                chain_data.latest_link().unwrap_or_else(|| {
                    eprintln!("Error: Chain '{}' has no links", chain);
                    std::process::exit(1);
                })
            };

            let content = std::fs::read_to_string(&link_data.file_path)?;
            println!("{}", content);
        }
    }

    Ok(())
}

fn try_ipc_command(cmd: &Commands) -> Option<Result<()>> {
    match cmd {
        Commands::Add {
            task,
            repo,
            name,
            agent,
        } => Some(cmd_ipc_add(
            task.clone(),
            repo.clone(),
            name.clone(),
            agent.clone(),
        )),
        Commands::Status { task } => Some(cmd_ipc_status(task.clone())),
        Commands::Send {
            task,
            message,
            pane,
        } => Some(cmd_ipc_send(task.clone(), pane.clone(), message.clone())),
        Commands::Approve { task, pane } => Some(cmd_ipc_approve(task.clone(), pane.clone())),
        Commands::Reject { task, pane } => Some(cmd_ipc_reject(task.clone(), pane.clone())),
        Commands::Output { task, pane, lines } => {
            Some(cmd_ipc_output(task.clone(), pane.clone(), *lines))
        }
        Commands::Resume { task, pane } => Some(cmd_ipc_resume(task.clone(), pane.clone())),
        _ => None,
    }
}

fn cmd_ipc_add(
    task: Option<String>,
    repo: Option<String>,
    name: Option<String>,
    agent: Option<String>,
) -> Result<()> {
    use wagner::transport::{CoreCommand, ipc};
    let config = Config::load()?;
    let task_name = task
        .or_else(|| detect_task_from_cwd(&config))
        .unwrap_or_else(|| {
            eprintln!("Error: Not inside a task directory");
            eprintln!("Either cd into a task, or specify: wagner add <task>");
            std::process::exit(1);
        });
    let cmd = CoreCommand::AddPane {
        task_name,
        pane_name: name,
        agent,
        repo_name: repo,
    };
    let response = ipc::daemon_execute(cmd)?;
    print_response(&response);
    Ok(())
}

fn cmd_ipc_status(task: Option<String>) -> Result<()> {
    use wagner::transport::{CoreCommand, ipc};
    let cmd = match task {
        Some(name) => CoreCommand::TaskStatus { task_name: name },
        None => CoreCommand::FullStatus,
    };
    let response = ipc::daemon_execute(cmd)?;
    print_response(&response);
    Ok(())
}

fn cmd_ipc_send(task: String, pane: Option<String>, message_parts: Vec<String>) -> Result<()> {
    use wagner::transport::{CoreCommand, ipc};
    let cmd = CoreCommand::SendMessage {
        task_name: task,
        pane_name: pane,
        message: message_parts.join(" "),
    };
    let response = ipc::daemon_execute(cmd)?;
    print_response(&response);
    Ok(())
}

fn cmd_ipc_approve(task: Option<String>, pane: Option<String>) -> Result<()> {
    use wagner::transport::{CoreCommand, ipc};
    let cmd = CoreCommand::Approve {
        task_name: task.unwrap_or_default(),
        pane_name: pane,
    };
    let response = ipc::daemon_execute(cmd)?;
    print_response(&response);
    Ok(())
}

fn cmd_ipc_reject(task: String, pane: Option<String>) -> Result<()> {
    use wagner::transport::{CoreCommand, ipc};
    let cmd = CoreCommand::Reject {
        task_name: task,
        pane_name: pane,
    };
    let response = ipc::daemon_execute(cmd)?;
    print_response(&response);
    Ok(())
}

fn cmd_ipc_output(task: String, pane: Option<String>, lines: Option<usize>) -> Result<()> {
    use wagner::transport::{CoreCommand, ipc};
    let cmd = CoreCommand::CaptureOutput {
        task_name: task,
        pane_name: pane,
        lines,
    };
    let response = ipc::daemon_execute(cmd)?;
    print_response(&response);
    Ok(())
}

fn cmd_ipc_resume(task: String, pane: Option<String>) -> Result<()> {
    use wagner::transport::{CoreCommand, ipc};
    let cmd = CoreCommand::Resume {
        task_name: task,
        pane_name: pane,
    };
    let response = ipc::daemon_execute(cmd)?;
    print_response(&response);
    Ok(())
}

fn print_response(response: &wagner::transport::CoreResponse) {
    use wagner::transport::CoreResponse;

    match response {
        CoreResponse::TaskList { tasks } => {
            if tasks.is_empty() {
                println!("No tasks");
                return;
            }
            for (summary, status) in tasks {
                println!(
                    "{} {:<20} {} repos, {} panes  [{}]",
                    status.icon(),
                    summary.name,
                    summary.repo_count,
                    summary.pane_count,
                    status.label(),
                );
            }
        }
        CoreResponse::Status {
            task_name, panes, ..
        } => {
            println!("{}", task_name);
            if panes.is_empty() {
                println!("  (no panes)");
                return;
            }
            for (name, status) in panes {
                println!("  {} {} [{}]", status.icon(), name, status.label());
            }
        }
        CoreResponse::FullStatus { tasks } => {
            if tasks.is_empty() {
                println!("No tasks");
                return;
            }
            for (summary, agg_status, panes) in tasks {
                println!(
                    "{} {}  [{}]",
                    agg_status.icon(),
                    summary.name,
                    agg_status.label(),
                );
                for (name, status) in panes {
                    println!("    {} {} [{}]", status.icon(), name, status.label());
                }
            }
        }
        CoreResponse::Output {
            task_name,
            pane_name,
            content,
        } => {
            println!("--- {} / {} ---", task_name, pane_name);
            println!("{}", content);
        }
        CoreResponse::Confirmation { message } => println!("{}", message),
        CoreResponse::Error { message } => eprintln!("Error: {}", message),
        CoreResponse::HelpText => println!("Wagner CLI - use --help for available commands"),
        _ => {}
    }
}

fn cmd_daemon(command: DaemonCommands, config: Config) -> Result<()> {
    match command {
        DaemonCommands::Start => {
            if config.daemon.telegram.is_none() {
                info!("No Telegram configured, running with log transport");
            }
            info!("Starting daemon");
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(wagner::transport::daemon::run_daemon(config))
        }
        DaemonCommands::Stop => stop_daemon(),
        DaemonCommands::Restart => {
            stop_daemon_and_wait();
            if config.daemon.telegram.is_none() {
                info!("No Telegram configured, running with log transport");
            }
            info!("Starting daemon");
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(wagner::transport::daemon::run_daemon(config))
        }
        DaemonCommands::Status => {
            match read_daemon_pid() {
                Some(pid_str) if daemon_alive(&pid_str) => {
                    println!("Daemon running (PID {pid_str})");
                }
                _ => println!("Daemon not running"),
            }
            Ok(())
        }
    }
}

fn stop_daemon() -> Result<()> {
    let Some(pid_str) = read_daemon_pid() else {
        println!("Daemon not running (no PID file)");
        return Ok(());
    };
    if !daemon_alive(&pid_str) {
        println!("Daemon not running (stale PID file, removing)");
        let _ = std::fs::remove_file(wagner::transport::daemon::pid_path());
        return Ok(());
    }
    let sent = std::process::Command::new("kill")
        .args(["-TERM", &pid_str])
        .status()
        .is_ok_and(|s| s.success());
    if sent {
        println!("Sent SIGTERM to daemon (PID {pid_str})");
    } else {
        println!("Failed to send SIGTERM to daemon (PID {pid_str})");
    }
    Ok(())
}

fn stop_daemon_and_wait() {
    let Some(pid_str) = read_daemon_pid() else {
        println!("Daemon not running, starting fresh");
        return;
    };
    if !daemon_alive(&pid_str) {
        println!("Daemon not running (stale PID file, removing)");
        let _ = std::fs::remove_file(wagner::transport::daemon::pid_path());
        return;
    }
    let sent = std::process::Command::new("kill")
        .args(["-TERM", &pid_str])
        .status()
        .is_ok_and(|s| s.success());
    if !sent {
        println!("Failed to send SIGTERM to daemon (PID {pid_str})");
        return;
    }
    println!("Sent SIGTERM to daemon (PID {pid_str}), waiting...");
    for _ in 0..50 {
        if !daemon_alive(&pid_str) {
            println!("Daemon stopped");
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    println!("Daemon did not stop within 5s, proceeding anyway");
}

fn read_daemon_pid() -> Option<String> {
    let path = wagner::transport::daemon::pid_path();
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
}

fn daemon_alive(pid_str: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid_str])
        .status()
        .is_ok_and(|s| s.success())
}

fn cmd_plugin(command: PluginCommands) -> Result<()> {
    let mut config = Config::load()?;

    match command {
        PluginCommands::List => {
            let all_plugins = plugins::builtin_plugins();

            if all_plugins.is_empty() {
                println!("No plugins available");
                return Ok(());
            }

            println!("Available plugins:\n");
            for plugin in all_plugins {
                let status = if plugin.is_enabled(&config) {
                    "enabled"
                } else {
                    "disabled"
                };
                println!("  {} [{}]", plugin.id(), status);
                println!("    {}", plugin.description());
                println!();
            }
        }
        PluginCommands::Enable { plugin: plugin_id } => {
            let plugin = plugins::get_plugin(&plugin_id).unwrap_or_else(|| {
                eprintln!("Plugin '{}' not found", plugin_id);
                eprintln!("Run `wagner plugin list` to see available plugins");
                std::process::exit(1);
            });

            match plugin_id.as_str() {
                "chains" => config.plugins.chains.enabled = true,
                _ => {
                    eprintln!("Unknown plugin: {}", plugin_id);
                    std::process::exit(1);
                }
            }

            config.save()?;

            info!(plugin = %plugin_id, "Plugin enabled");
            println!("Enabled plugin: {}", plugin_id);

            let skills = plugin.agent_skills();
            if !skills.is_empty() {
                println!("\nThis plugin provides agent skills: {}", skills.join(", "));
                println!("If you don't have these from another source (e.g., agent-system),");
                println!("install them with: wagner plugin install-skills");
            }
        }
        PluginCommands::Disable { plugin: plugin_id } => {
            let plugin = plugins::get_plugin(&plugin_id).unwrap_or_else(|| {
                eprintln!("Plugin '{}' not found", plugin_id);
                eprintln!("Run `wagner plugin list` to see available plugins");
                std::process::exit(1);
            });

            match plugin_id.as_str() {
                "chains" => config.plugins.chains.enabled = false,
                _ => {
                    eprintln!("Unknown plugin: {}", plugin_id);
                    std::process::exit(1);
                }
            }

            config.save()?;

            info!(plugin = %plugin_id, "Plugin disabled");
            println!("Disabled plugin: {}", plugin_id);

            let skills = plugin.agent_skills();
            if !skills.is_empty() {
                println!("\nNote: Agent skills were not removed from ~/.claude/commands/");
                println!("Remove them manually if desired: {}", skills.join(", "));
            }
        }
        PluginCommands::InstallSkills => {
            let all_plugins = plugins::builtin_plugins();
            let mut installed = 0;

            for plugin in all_plugins {
                if plugin.is_enabled(&config) {
                    if let Err(e) = plugins::install_skills(plugin.as_ref(), &config) {
                        eprintln!(
                            "Warning: Failed to install skills for {}: {}",
                            plugin.id(),
                            e
                        );
                    } else {
                        installed += 1;
                        println!("Installed skills for: {}", plugin.id());
                    }
                }
            }

            if installed == 0 {
                println!("No enabled plugins with skills to install");
                println!("Enable a plugin with: wagner plugin enable <plugin>");
            }
        }
    }

    Ok(())
}
