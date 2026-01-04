use crate::cli::{Cli, Commands};
use tracing::{debug, info, warn};
use wagner::{Agent, ClaudeCode, Config, RepoSpec, Result, Terminal, Tmux, Wagner};

pub fn run(cli: Cli) -> Result<()> {
    let config = Config::load()?;
    debug!("Loaded config from {:?}", Config::config_path());

    let terminal = Tmux::new();
    let agent = ClaudeCode::new();
    let wagner = Wagner::new(terminal, agent, config);

    match cli.command {
        Some(Commands::New { name, repos }) => cmd_new(&wagner, &name, &repos),
        Some(Commands::List) => cmd_list(&wagner),
        Some(Commands::Delete { name, force }) => cmd_delete(&wagner, &name, force),
        Some(Commands::Add { task, repo }) => cmd_add(&wagner, task.as_deref(), repo.as_deref()),
        Some(Commands::Attach { task }) => cmd_attach(&wagner, &task),
        Some(Commands::Send { session, message }) => cmd_send(&wagner, &session, &message),
        Some(Commands::Chains { task, repo }) => cmd_chains(&wagner, task.as_deref(), repo.as_deref()),
        None => cmd_tui(&wagner),
    }
}

fn cmd_new<T: Terminal, A: Agent>(
    wagner: &Wagner<T, A>,
    name: &str,
    repos: &[String],
) -> Result<()> {
    debug!("Creating task '{}' with {} repos", name, repos.len());

    let specs: Vec<RepoSpec> = repos
        .iter()
        .map(|s| RepoSpec::parse(s))
        .collect::<Result<Vec<_>>>()?;

    if specs.is_empty() {
        warn!("No repos specified");
        eprintln!("Error: At least one repo is required");
        eprintln!("Usage: wagner new <name> --repos name:path:branch");
        std::process::exit(1);
    }

    let task = wagner.create_task(name, &specs)?;

    info!(task = %task.name, path = %task.path.display(), "Task created");

    println!("Created task: {}", task.name);
    println!("  Path: {}", task.path.display());
    println!("  Repos:");
    for repo in &task.repos {
        println!("    {} -> {} ({})", repo.name, repo.worktree.display(), repo.branch);
    }
    println!();
    println!("Attach with: wagner attach {}", task.name);

    Ok(())
}

fn cmd_list<T: Terminal, A: Agent>(wagner: &Wagner<T, A>) -> Result<()> {
    let tasks = wagner.list_tasks()?;
    debug!("Found {} tasks", tasks.len());

    if tasks.is_empty() {
        println!("No tasks found");
        println!("Create one with: wagner new <name> --repos name:path:branch");
        return Ok(());
    }

    for task in tasks {
        let repos_count = task.repos.len();
        let repos_label = if repos_count == 1 { "repo" } else { "repos" };

        println!(
            "{:<20} {} {}  {}",
            task.name,
            repos_count,
            repos_label,
            task.path.display()
        );
    }

    Ok(())
}

fn cmd_delete<T: Terminal, A: Agent>(
    wagner: &Wagner<T, A>,
    name: &str,
    force: bool,
) -> Result<()> {
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
    task: Option<&str>,
    repo: Option<&str>,
) -> Result<()> {
    let task_name = task.unwrap_or_else(|| {
        warn!("Task name not provided");
        eprintln!("Error: Task name required (or run from within a task directory)");
        std::process::exit(1);
    });

    debug!(task = %task_name, repo = ?repo, "Adding pane");

    let pane = wagner.add_pane(task_name, repo)?;
    info!(task = %task_name, pane = %pane.0, "Pane created");
    println!("Created pane: {}", pane.0);

    Ok(())
}

fn cmd_attach<T: Terminal, A: Agent>(wagner: &Wagner<T, A>, task: &str) -> Result<()> {
    debug!(task = %task, "Attaching to session");
    wagner.attach(task)
}

fn cmd_send<T: Terminal, A: Agent>(
    _wagner: &Wagner<T, A>,
    _session: &str,
    _message: &str,
) -> Result<()> {
    warn!("Send command not yet implemented");
    println!("Send command not yet implemented");
    Ok(())
}

fn cmd_chains<T: Terminal, A: Agent>(
    _wagner: &Wagner<T, A>,
    _task: Option<&str>,
    _repo: Option<&str>,
) -> Result<()> {
    warn!("Chains command not yet implemented");
    println!("Chains command not yet implemented");
    Ok(())
}

fn cmd_tui<T: Terminal, A: Agent>(_wagner: &Wagner<T, A>) -> Result<()> {
    info!("TUI not yet implemented");
    println!("TUI not yet implemented. Use subcommands:");
    println!("  wagner new <name> --repos ...");
    println!("  wagner list");
    println!("  wagner attach <task>");
    println!("  wagner delete <task>");
    Ok(())
}
