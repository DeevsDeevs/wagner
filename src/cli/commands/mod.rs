use crate::cli::{Cli, Commands, WorkspaceCommands, print_completions};
use tracing::{debug, info};
use wagner::{
    Agent, ClaudeCode, Config, RepoSource, RepoSpec, Result, Terminal, Tmux, Wagner,
    default_branch_for_task,
};

pub fn run(cli: Cli) -> Result<()> {
    if let Some(Commands::Completions { shell }) = &cli.command {
        print_completions(*shell);
        return Ok(());
    }

    let config = Config::load()?;
    debug!("Loaded config from {:?}", Config::config_path());

    let terminal = Tmux::new();
    let agent = ClaudeCode::new();
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
        Some(Commands::Add { task, repo }) => cmd_add(&wagner, task, repo.as_deref()),
        Some(Commands::AddRepo { task, repo }) => cmd_add_repo(&wagner, &task, &repo),
        Some(Commands::RmRepo { task, repo }) => cmd_rm_repo(&wagner, &task, &repo),
        Some(Commands::Attach { task }) => cmd_attach(&wagner, task),
        Some(Commands::Completions { .. }) => unreachable!(),
        Some(Commands::Workspace { command }) => cmd_workspace(command),
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

        println!(
            "{:<20} {} {}  ({})  {}",
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

fn cmd_add<T: Terminal, A: Agent>(
    wagner: &Wagner<T, A>,
    task: Option<String>,
    repo: Option<&str>,
) -> Result<()> {
    let task_name = task
        .or_else(|| detect_task_from_cwd(&wagner.config))
        .unwrap_or_else(|| {
            eprintln!("Error: Not inside a task directory");
            eprintln!("Either cd into a task, or specify: wagner add <task>");
            std::process::exit(1);
        });

    debug!(task = %task_name, repo = ?repo, "Adding pane");

    let pane = wagner.add_pane(&task_name, repo)?;
    info!(task = %task_name, pane = %pane.0, "Pane created");
    println!("Created pane: {}", pane.0);

    Ok(())
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
    wagner.attach(&task_name)
}

fn cmd_tui<T: Terminal + 'static, A: Agent + 'static>(wagner: Wagner<T, A>) -> Result<()> {
    info!("Launching TUI");
    wagner::tui::run(wagner)
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
