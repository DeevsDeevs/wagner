use std::path::Path;
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

pub fn get_diff_files(repo_path: &Path, base: &str) -> Vec<DiffFile> {
    let output = Command::new("git")
        .args(["-C", &repo_path.to_string_lossy(), "diff", "--numstat", &format!("{}..HEAD", base)])
        .output();

    let Ok(output) = output else { return Vec::new() };
    if !output.status.success() {
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut files = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 3 {
            let additions = parts[0].parse().unwrap_or(0);
            let deletions = parts[1].parse().unwrap_or(0);
            let path = parts[2].to_string();

            let status = get_file_status(repo_path, base, &path);

            files.push(DiffFile {
                path,
                status,
                additions,
                deletions,
            });
        }
    }

    files
}

fn get_file_status(repo_path: &Path, base: &str, file_path: &str) -> char {
    let output = Command::new("git")
        .args([
            "-C", &repo_path.to_string_lossy(),
            "diff", "--name-status", &format!("{}..HEAD", base),
            "--", file_path
        ])
        .output();

    let Ok(output) = output else { return 'M' };
    if !output.status.success() {
        return 'M';
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.chars().next().unwrap_or('M')
}

pub fn get_repo_stats(repo_path: &Path, base: &str) -> RepoStats {
    let output = Command::new("git")
        .args(["-C", &repo_path.to_string_lossy(), "diff", "--shortstat", &format!("{}..HEAD", base)])
        .output();

    let Ok(output) = output else { return RepoStats::default() };
    if !output.status.success() {
        return RepoStats::default();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_shortstat(&stdout)
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
        } else if part.contains("deletion") {
            if let Some(n) = part.split_whitespace().next() {
                stats.deletions = n.parse().unwrap_or(0);
            }
        }
    }

    stats
}

pub fn get_diff_content(repo_path: &Path, base: &str, file_path: &str) -> Vec<String> {
    let output = Command::new("git")
        .args([
            "-C", &repo_path.to_string_lossy(),
            "diff", "--color=always", &format!("{}..HEAD", base),
            "--", file_path
        ])
        .output();

    let Ok(output) = output else { return Vec::new() };
    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(String::from)
        .collect()
}
