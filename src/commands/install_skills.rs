use anyhow::{Context, Result};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

const SEARCH_SKILL: &str = include_str!("../skills/aihist-search.md");
const SHOW_SKILL: &str = include_str!("../skills/aihist-show.md");
const MCP_SKILL: &str = include_str!("../skills/aihist-mcp.md");

struct Skill {
    name: &'static str,
    content: &'static str,
}

const SKILLS: &[Skill] = &[
    Skill { name: "aihist-search", content: SEARCH_SKILL },
    Skill { name: "aihist-show",   content: SHOW_SKILL },
    Skill { name: "aihist-mcp",    content: MCP_SKILL },
];

struct HarnessTarget {
    name: &'static str,
    presence_check: &'static str,
    skills_dest: &'static str,
}

const HARNESS_TARGETS: &[HarnessTarget] = &[
    HarnessTarget { name: "agents-hub", presence_check: ".agents/skills",     skills_dest: ".agents/skills"          },
    HarnessTarget { name: "claude",     presence_check: ".claude",            skills_dest: ".claude/skills"          },
    HarnessTarget { name: "opencode",   presence_check: ".config/opencode",   skills_dest: ".config/opencode/skills" },
    HarnessTarget { name: "codex",      presence_check: ".codex",             skills_dest: ".codex/skills"           },
    HarnessTarget { name: "pi",         presence_check: ".pi/agent",          skills_dest: ".pi/agent/skills"        },
];

#[derive(Serialize)]
pub struct TargetResult {
    pub harness: String,
    pub skills_dir: String,
    pub installed: Vec<String>,
}

fn home() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/tmp"))
}

fn detect_targets() -> Vec<(&'static str, PathBuf)> {
    let h = home();
    HARNESS_TARGETS
        .iter()
        .filter(|t| h.join(t.presence_check).exists())
        .map(|t| (t.name, h.join(t.skills_dest)))
        .collect()
}

fn write_skill(dir: &Path, skill: &Skill) -> Result<String> {
    let skill_dir = dir.join(skill.name);
    fs::create_dir_all(&skill_dir)
        .with_context(|| format!("create {}", skill_dir.display()))?;
    let dest = skill_dir.join("SKILL.md");
    fs::write(&dest, skill.content)
        .with_context(|| format!("write {}", dest.display()))?;
    Ok(dest.to_string_lossy().into_owned())
}

fn write_skills_to(dir: &Path) -> Result<Vec<String>> {
    SKILLS.iter().map(|skill| write_skill(dir, skill)).collect()
}

fn install_to(harness: &str, dir: PathBuf) -> Result<TargetResult> {
    let installed = write_skills_to(&dir)?;
    Ok(TargetResult {
        harness: harness.to_string(),
        skills_dir: dir.to_string_lossy().into_owned(),
        installed,
    })
}

pub fn run(override_dir: Option<&Path>) -> Result<Vec<TargetResult>> {
    let targets: Vec<(&str, PathBuf)> = override_dir
        .map(|p| vec![("custom", p.to_path_buf())])
        .unwrap_or_else(detect_targets);

    targets.into_iter().map(|(h, d)| install_to(h, d)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn writes_skills_to_given_dir() {
        let tmp = TempDir::new().unwrap();
        let results = run(Some(tmp.path())).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].installed.len(), 3);
        let missing: Vec<_> = ["aihist-search", "aihist-show", "aihist-mcp"]
            .iter()
            .filter(|name| {
                let p = tmp.path().join(name).join("SKILL.md");
                !p.exists()
            })
            .collect();
        assert!(missing.is_empty(), "missing skill files: {missing:?}");
    }

    #[test]
    fn skill_content_matches_name() {
        let tmp = TempDir::new().unwrap();
        run(Some(tmp.path())).unwrap();
        let mismatches: Vec<_> = ["aihist-search", "aihist-show", "aihist-mcp"]
            .iter()
            .filter(|name| {
                let content = fs::read_to_string(tmp.path().join(name).join("SKILL.md")).unwrap();
                !content.contains(&format!("name: {name}"))
            })
            .collect();
        assert!(mismatches.is_empty(), "name mismatch: {mismatches:?}");
    }

    #[test]
    fn idempotent_on_second_run() {
        let tmp = TempDir::new().unwrap();
        run(Some(tmp.path())).unwrap();
        assert!(run(Some(tmp.path())).is_ok());
    }

    #[test]
    fn detect_targets_finds_each_harness() {
        let tmp = TempDir::new().unwrap();
        let h = tmp.path();
        [".claude", ".config/opencode", ".codex", ".pi/agent"]
            .iter()
            .for_each(|d| fs::create_dir_all(h.join(d)).unwrap());

        let old = std::env::var("HOME").ok();
        unsafe { std::env::set_var("HOME", h.to_str().unwrap()) };
        let targets = detect_targets();
        old.map(|v| unsafe { std::env::set_var("HOME", v) });

        let names: Vec<&str> = targets.iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"claude"),   "expected claude");
        assert!(names.contains(&"opencode"), "expected opencode");
        assert!(names.contains(&"codex"),    "expected codex");
        assert!(names.contains(&"pi"),       "expected pi");
    }

    #[test]
    fn detects_agents_hub() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agents/skills")).unwrap();

        let old = std::env::var("HOME").ok();
        unsafe { std::env::set_var("HOME", tmp.path().to_str().unwrap()) };
        let targets = detect_targets();
        old.map(|v| unsafe { std::env::set_var("HOME", v) });

        assert!(targets.iter().any(|(n, _)| *n == "agents-hub"));
    }

    #[test]
    fn skill_content_has_frontmatter() {
        assert!(SEARCH_SKILL.starts_with("---\n"));
        assert!(SHOW_SKILL.starts_with("---\n"));
        assert!(MCP_SKILL.starts_with("---\n"));
    }
}
