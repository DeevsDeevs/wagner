use super::data::{Chain, ChainLink, ChainSource, ChainsData, RepoChains};
use crate::error::Result;
use std::path::{Path, PathBuf};
use tracing::debug;

pub fn load_chains_from_path(path: &Path, source: ChainSource) -> Result<Vec<Chain>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let mut chains = Vec::new();

    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let chain_dir = entry.path();

        if !chain_dir.is_dir() {
            continue;
        }

        let chain_name = chain_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let links = load_links_from_chain_dir(&chain_dir)?;

        if !links.is_empty() {
            chains.push(Chain {
                name: chain_name,
                links,
                source: source.clone(),
            });
        }
    }

    chains.sort_by(|a, b| {
        let a_latest = a.latest_link().map(|l| &l.timestamp);
        let b_latest = b.latest_link().map(|l| &l.timestamp);
        b_latest.cmp(&a_latest)
    });

    Ok(chains)
}

fn load_links_from_chain_dir(chain_dir: &Path) -> Result<Vec<ChainLink>> {
    let mut links = Vec::new();

    for entry in std::fs::read_dir(chain_dir)? {
        let entry = entry?;
        let file_path = entry.path();

        if !file_path.is_file() {
            continue;
        }

        let file_name = match file_path.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };

        if !file_name.ends_with(".md") {
            continue;
        }

        if let Some(link) = parse_chain_link_filename(&file_path) {
            links.push(link);
        }
    }

    links.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

    Ok(links)
}

fn parse_chain_link_filename(file_path: &Path) -> Option<ChainLink> {
    let file_name = file_path.file_stem()?.to_string_lossy().to_string();

    let parts: Vec<&str> = file_name.splitn(2, '-').collect();
    if parts.len() < 2 {
        return None;
    }

    let timestamp_parts: Vec<&str> = file_name.split('-').collect();
    if timestamp_parts.len() < 5 {
        return None;
    }

    let timestamp = format!(
        "{}-{}-{}-{}",
        timestamp_parts[0], timestamp_parts[1], timestamp_parts[2], timestamp_parts[3]
    );
    let slug = timestamp_parts[4..].join("-");

    let (summary, next_step) = parse_chain_link_content(file_path);

    Some(ChainLink {
        timestamp,
        slug,
        file_path: file_path.to_path_buf(),
        summary,
        next_step,
    })
}

fn parse_chain_link_content(file_path: &Path) -> (Option<String>, Option<String>) {
    let content = match std::fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(_) => return (None, None),
    };

    let summary = extract_section(&content, "Primary Request and Intent");
    let next_step = extract_section(&content, "Next Step");

    (summary, next_step)
}

fn extract_section(content: &str, section_name: &str) -> Option<String> {
    let marker = format!("## ");
    let section_header = content.lines().find(|line| {
        line.starts_with(&marker) && line.to_lowercase().contains(&section_name.to_lowercase())
    })?;

    let start_idx = content.find(section_header)? + section_header.len();
    let rest = &content[start_idx..];

    let end_idx = rest
        .find("\n## ")
        .or_else(|| rest.find("\n---"))
        .unwrap_or(rest.len());

    let section_content = rest[..end_idx].trim();

    if section_content.is_empty() {
        None
    } else {
        let first_para = section_content
            .split("\n\n")
            .next()
            .unwrap_or(section_content);
        let truncated = if first_para.len() > 200 {
            format!("{}...", &first_para[..200])
        } else {
            first_para.to_string()
        };
        Some(truncated)
    }
}

pub fn load_all_chains(tasks_root: &Path, task_name: Option<&str>) -> Result<ChainsData> {
    let mut data = ChainsData::default();
    let mut seen_repos: std::collections::HashMap<PathBuf, usize> =
        std::collections::HashMap::new();

    if !tasks_root.exists() {
        return Ok(data);
    }

    for entry in std::fs::read_dir(tasks_root)? {
        let entry = entry?;
        let task_path = entry.path();

        if !task_path.is_dir() {
            continue;
        }

        let current_task_name = task_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        if let Some(filter_task) = task_name {
            if current_task_name != filter_task {
                continue;
            }
        }

        let task_json = task_path.join(".wagner").join("task.json");
        if !task_json.exists() {
            continue;
        }

        let plugins_link = task_path.join(".wagner").join("plugins");
        if plugins_link.is_symlink() {
            if let Ok(target) = std::fs::read_link(&plugins_link) {
                let chains_dir = if target.is_absolute() {
                    target.join("chains")
                } else {
                    plugins_link.parent().unwrap().join(&target).join("chains")
                };

                if chains_dir.exists() {
                    let repo_path = chains_dir
                        .parent()
                        .and_then(|p| p.parent())
                        .unwrap_or(&chains_dir)
                        .to_path_buf();

                    if !seen_repos.contains_key(&repo_path) {
                        let repo_name = repo_path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "unknown".to_string());

                        let chains = load_chains_from_path(
                            &chains_dir,
                            ChainSource::Repo(repo_path.clone()),
                        )?;

                        seen_repos.insert(repo_path.clone(), data.repos.len());
                        data.repos.push(RepoChains {
                            repo_name,
                            repo_path,
                            chains,
                        });
                    }
                }
            }
        }

        let local_chains_dir = task_path.join(".claude").join("chains");
        if local_chains_dir.exists() && !local_chains_dir.is_symlink() {
            let local_chains = load_chains_from_path(
                &local_chains_dir,
                ChainSource::TaskLocal(task_path.clone()),
            )?;

            for mut chain in local_chains {
                chain.name = format!("{}/{}", current_task_name, chain.name);
                data.task_local.push(chain);
            }
        }

        for entry in std::fs::read_dir(&task_path)
            .into_iter()
            .flatten()
            .flatten()
        {
            let repo_path = entry.path();
            if !repo_path.is_dir() {
                continue;
            }
            let repo_name = repo_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if repo_name.starts_with('.') {
                continue;
            }
            let repo_chains_dir = repo_path.join(".claude").join("chains");
            if repo_chains_dir.exists() && !repo_chains_dir.is_symlink() {
                let repo_chains = load_chains_from_path(
                    &repo_chains_dir,
                    ChainSource::TaskLocal(repo_path.clone()),
                )?;
                for mut chain in repo_chains {
                    chain.name = format!("{}/{}/{}", current_task_name, repo_name, chain.name);
                    data.task_local.push(chain);
                }
            }
        }
    }

    debug!(
        repos = data.repos.len(),
        task_local = data.task_local.len(),
        "Loaded chains"
    );

    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_chain_file(dir: &std::path::Path, chain_name: &str, filename: &str, content: &str) {
        let chain_dir = dir.join(chain_name);
        std::fs::create_dir_all(&chain_dir).unwrap();
        std::fs::write(chain_dir.join(filename), content).unwrap();
    }

    #[test]
    fn test_load_chains_from_empty_path() {
        let temp_dir = TempDir::new().unwrap();
        let chains = load_chains_from_path(
            &temp_dir.path().join("nonexistent"),
            ChainSource::Repo(temp_dir.path().to_path_buf()),
        )
        .unwrap();
        assert!(chains.is_empty());
    }

    #[test]
    fn test_load_chains_from_path_with_chains() {
        let temp_dir = TempDir::new().unwrap();
        let chains_dir = temp_dir.path().join("chains");
        std::fs::create_dir_all(&chains_dir).unwrap();

        create_chain_file(
            &chains_dir,
            "my-feature",
            "2025-01-12-1030-initial-setup.md",
            "# Chain Link Summary\n\n## 1. Primary Request and Intent\nImplement feature X\n\n## 9. Next Step\nAdd tests",
        );

        let chains = load_chains_from_path(
            &chains_dir,
            ChainSource::Repo(temp_dir.path().to_path_buf()),
        )
        .unwrap();

        assert_eq!(chains.len(), 1);
        assert_eq!(chains[0].name, "my-feature");
        assert_eq!(chains[0].links.len(), 1);
        assert_eq!(chains[0].links[0].timestamp, "2025-01-12-1030");
        assert_eq!(chains[0].links[0].slug, "initial-setup");
    }

    #[test]
    fn test_load_chains_multiple_links() {
        let temp_dir = TempDir::new().unwrap();
        let chains_dir = temp_dir.path().join("chains");
        std::fs::create_dir_all(&chains_dir).unwrap();

        create_chain_file(
            &chains_dir,
            "my-feature",
            "2025-01-10-0900-first-link.md",
            "# First link",
        );
        create_chain_file(
            &chains_dir,
            "my-feature",
            "2025-01-11-1400-second-link.md",
            "# Second link",
        );
        create_chain_file(
            &chains_dir,
            "my-feature",
            "2025-01-12-1030-third-link.md",
            "# Third link",
        );

        let chains = load_chains_from_path(
            &chains_dir,
            ChainSource::Repo(temp_dir.path().to_path_buf()),
        )
        .unwrap();

        assert_eq!(chains.len(), 1);
        assert_eq!(chains[0].links.len(), 3);
        assert_eq!(chains[0].links[0].timestamp, "2025-01-10-0900");
        assert_eq!(chains[0].links[2].timestamp, "2025-01-12-1030");
        assert_eq!(chains[0].latest_link().unwrap().slug, "third-link");
    }

    #[test]
    fn test_load_chains_multiple_chains() {
        let temp_dir = TempDir::new().unwrap();
        let chains_dir = temp_dir.path().join("chains");
        std::fs::create_dir_all(&chains_dir).unwrap();

        create_chain_file(
            &chains_dir,
            "feature-a",
            "2025-01-10-0900-setup.md",
            "# Feature A",
        );
        create_chain_file(
            &chains_dir,
            "feature-b",
            "2025-01-12-1400-setup.md",
            "# Feature B",
        );

        let chains = load_chains_from_path(
            &chains_dir,
            ChainSource::Repo(temp_dir.path().to_path_buf()),
        )
        .unwrap();

        assert_eq!(chains.len(), 2);
        assert_eq!(chains[0].name, "feature-b");
        assert_eq!(chains[1].name, "feature-a");
    }

    #[test]
    fn test_extract_section_primary_request() {
        let content = r#"# Chain Link Summary

## 1. Primary Request and Intent
Implement a new authentication system with OAuth support.

## 2. Key Technical Concepts
- OAuth 2.0
- JWT tokens
"#;
        let section = extract_section(content, "Primary Request and Intent");
        assert!(section.is_some());
        assert!(section.unwrap().contains("authentication system"));
    }

    #[test]
    fn test_extract_section_next_step() {
        let content = r#"## 8. Current Work
Working on tests

## 9. Next Step
Add integration tests for the auth handler
"#;
        let section = extract_section(content, "Next Step");
        assert!(section.is_some());
        assert!(section.unwrap().contains("integration tests"));
    }

    #[test]
    fn test_extract_section_not_found() {
        let content = "# Some content\nNo sections here";
        let section = extract_section(content, "Nonexistent Section");
        assert!(section.is_none());
    }

    #[test]
    fn test_chain_data_total_chains() {
        let data = ChainsData {
            repos: vec![RepoChains {
                repo_name: "repo1".to_string(),
                repo_path: PathBuf::from("/repo1"),
                chains: vec![
                    Chain {
                        name: "chain1".to_string(),
                        links: vec![],
                        source: ChainSource::Repo(PathBuf::from("/repo1")),
                    },
                    Chain {
                        name: "chain2".to_string(),
                        links: vec![],
                        source: ChainSource::Repo(PathBuf::from("/repo1")),
                    },
                ],
            }],
            task_local: vec![Chain {
                name: "local1".to_string(),
                links: vec![],
                source: ChainSource::TaskLocal(PathBuf::from("/task1")),
            }],
        };

        assert_eq!(data.total_chains(), 3);
    }
}
