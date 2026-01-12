pub mod chains;

use crate::config::Config;
use crate::error::Result;

pub trait PluginData: Send + Sync {}

pub trait Plugin: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;

    fn data_dir(&self) -> &'static str;

    fn is_enabled(&self, config: &Config) -> bool;

    fn agent_skills(&self) -> &[&'static str];

    fn skill_source_dir(&self) -> Option<&'static str> {
        None
    }
}

pub fn builtin_plugins() -> Vec<Box<dyn Plugin>> {
    vec![Box::new(chains::ChainsPlugin)]
}

pub fn get_plugin(id: &str) -> Option<Box<dyn Plugin>> {
    builtin_plugins().into_iter().find(|p| p.id() == id)
}

pub fn install_skills(plugin: &dyn Plugin, _config: &Config) -> Result<()> {
    let skills = plugin.agent_skills();
    if skills.is_empty() {
        return Ok(());
    }

    let skill_source = plugin.skill_source_dir();
    if skill_source.is_none() {
        return Ok(());
    }

    let source_base = skill_source.unwrap();
    let target_dir = home_dir().join(".claude").join("commands");

    std::fs::create_dir_all(&target_dir)?;

    let source_dir = find_skill_source_dir(source_base);

    let mut installed = 0;
    let mut skipped = 0;

    for skill in skills {
        let target = target_dir.join(skill);

        if target.exists() {
            tracing::info!(skill = %skill, "Skill already exists, skipping");
            skipped += 1;
            continue;
        }

        if let Some(ref dir) = source_dir {
            let source = dir.join(skill);
            if source.exists() {
                std::fs::copy(&source, &target)?;
                tracing::info!(skill = %skill, "Installed skill");
                installed += 1;
            } else {
                tracing::warn!(source = %source.display(), "Skill source file not found");
            }
        } else {
            tracing::warn!(skill = %skill, "Could not find skill source directory");
        }
    }

    tracing::info!(installed, skipped, "Skill installation complete");
    Ok(())
}

fn find_skill_source_dir(relative_path: &str) -> Option<std::path::PathBuf> {
    let exe_path = std::env::current_exe().ok()?;

    let candidates = [
        exe_path.parent().and_then(|p| p.parent()).map(|p| p.join(relative_path)),
        exe_path.parent().map(|p| p.join(relative_path)),
        Some(std::path::PathBuf::from(relative_path)),
        std::env::var("CARGO_MANIFEST_DIR").ok().map(|p| std::path::PathBuf::from(p).join(relative_path)),
    ];

    for candidate in candidates.into_iter().flatten() {
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}

pub fn uninstall_skills(plugin: &dyn Plugin) -> Result<()> {
    let skills = plugin.agent_skills();
    let target_dir = home_dir().join(".claude").join("commands");

    for skill in skills {
        let target = target_dir.join(skill);
        if target.exists() {
            std::fs::remove_file(&target)?;
            tracing::info!(skill = %skill, "Uninstalled skill");
        }
    }

    Ok(())
}

fn home_dir() -> std::path::PathBuf {
    std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
}
