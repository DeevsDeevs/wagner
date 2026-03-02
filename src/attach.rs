use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
pub enum AttachDetection {
    SingleRepo(PathBuf),
    MultiRepo(Vec<PathBuf>),
    NoRepos,
}

pub fn detect_attach_mode(paths: &[PathBuf]) -> AttachDetection {
    if paths.is_empty() {
        let cwd = match std::env::current_dir() {
            Ok(p) => p,
            Err(_) => return AttachDetection::NoRepos,
        };

        if is_git_repo(&cwd) {
            return AttachDetection::SingleRepo(cwd);
        }

        let repos: Vec<PathBuf> = std::fs::read_dir(&cwd)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter(|e| is_git_repo(&e.path()))
            .map(|e| e.path())
            .collect();

        if !repos.is_empty() {
            return AttachDetection::MultiRepo(repos);
        }

        AttachDetection::SingleRepo(cwd)
    } else if paths.len() == 1 {
        AttachDetection::SingleRepo(paths[0].clone())
    } else {
        AttachDetection::MultiRepo(paths.to_vec())
    }
}

pub fn derive_task_name(detection: &AttachDetection) -> String {
    match detection {
        AttachDetection::SingleRepo(path) => {
            let repo_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "repo".to_string());
            let branch = get_current_branch(path).unwrap_or_else(|| "HEAD".to_string());
            format!("{}-{}", repo_name, sanitize_branch_name(&branch))
        }
        AttachDetection::MultiRepo(paths) => paths
            .first()
            .and_then(|p| p.parent())
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "attached".to_string()),
        AttachDetection::NoRepos => "attached".to_string(),
    }
}

fn sanitize_branch_name(branch: &str) -> String {
    branch
        .replace(['/', ' '], "-")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

pub fn is_git_repo(path: &Path) -> bool {
    path.join(".git").exists()
}

pub fn get_current_branch(path: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(path)
        .output()
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}
