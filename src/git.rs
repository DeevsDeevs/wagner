use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct DiffFile {
    pub path: String,
    pub status: char,
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone, Default)]
pub struct RepoStats {
    pub additions: usize,
    pub deletions: usize,
    pub file_count: usize,
}

fn run_git(repo_path: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn resolve_base_ref(repo_path: &Path, base: &str) -> String {
    let check_ref =
        |r: &str| -> bool { run_git(repo_path, &["rev-parse", "--verify", r]).is_some() };

    if check_ref(base) {
        return base.to_string();
    }

    let origin_ref = format!("origin/{}", base);
    if check_ref(&origin_ref) {
        return origin_ref;
    }

    base.to_string()
}

pub fn get_diff_files(repo_path: &Path, base: &str) -> Vec<DiffFile> {
    let base_ref = resolve_base_ref(repo_path, base);
    let range = format!("{}..HEAD", base_ref);
    let Some(stdout) = run_git(repo_path, &["diff", "--numstat", &range]) else {
        return Vec::new();
    };

    stdout
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 {
                let additions = parts[0].parse().unwrap_or(0);
                let deletions = parts[1].parse().unwrap_or(0);
                let path = parts[2].to_string();
                let status = get_file_status(repo_path, &base_ref, &path);
                Some(DiffFile {
                    path,
                    status,
                    additions,
                    deletions,
                })
            } else {
                None
            }
        })
        .collect()
}

fn get_file_status(repo_path: &Path, base_ref: &str, file_path: &str) -> char {
    let range = format!("{}..HEAD", base_ref);
    run_git(
        repo_path,
        &["diff", "--name-status", &range, "--", file_path],
    )
    .and_then(|s| s.chars().next())
    .unwrap_or('M')
}

pub fn get_repo_stats(repo_path: &Path, base: &str) -> RepoStats {
    let base_ref = resolve_base_ref(repo_path, base);
    let range = format!("{}..HEAD", base_ref);
    run_git(repo_path, &["diff", "--shortstat", &range])
        .map(|s| parse_shortstat(&s))
        .unwrap_or_default()
}

fn parse_shortstat(s: &str) -> RepoStats {
    let mut stats = RepoStats::default();

    for part in s.split(',') {
        let part = part.trim();
        if part.contains("file") {
            if let Some(n) = part.split_whitespace().next() {
                stats.file_count = n.parse().unwrap_or(0);
            }
        } else if part.contains("insertion") {
            if let Some(n) = part.split_whitespace().next() {
                stats.additions = n.parse().unwrap_or(0);
            }
        } else if part.contains("deletion")
            && let Some(n) = part.split_whitespace().next()
        {
            stats.deletions = n.parse().unwrap_or(0);
        }
    }

    stats
}

pub fn get_diff_content(repo_path: &Path, base: &str, file_path: &str) -> Vec<String> {
    let base_ref = resolve_base_ref(repo_path, base);
    let range = format!("{}..HEAD", base_ref);
    run_git(
        repo_path,
        &["diff", "--color=always", &range, "--", file_path],
    )
    .map(|s| s.lines().map(String::from).collect())
    .unwrap_or_default()
}

/// Detect the current git repository from the working directory.
/// Returns `(repo_path, repo_name)` if inside a git repo.
pub fn detect_git_repo() -> Option<(PathBuf, String)> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let repo_path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let repo_name = repo_path.file_name()?.to_string_lossy().to_string();

    Some((repo_path, repo_name))
}
