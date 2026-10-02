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
    Skill { name: "aihist-show", content: SHOW_SKILL },
    Skill { name: "aihist-mcp", content: MCP_SKILL },
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
    let candidates: &[(&'static str, &'static str)] = &[
        ("agents-hub",  ".agents/skills"),
        ("claude",      ".claude/skills"),
        ("opencode",    ".config/opencode/skills"),
        ("codex",       ".codex/skills"),
        ("pi",          ".pi/agent/skills"),
    ];

    let presence_checks: &[(&'static str, &'static str)] = &[
        ("agents-hub",  ".agents/skills"),
        ("claude",      ".claude"),
        ("opencode",    ".config/opencode"),
        ("codex",       ".codex"),
        ("pi",          ".pi/agent"),
    ];

    candidates
        .iter()
        .zip(presence_checks.iter())
        .filter_map(|((name, dest), (_, check))| {
            if h.join(check).exists() {
                Some((*name, h.join(dest)))
            } else {
                None
            }
        })
        .collect()
}

fn write_skills_to(dir: &Path) -> Result<Vec<String>> {
    let mut written = Vec::new();
    for skill in SKILLS {
        let skill_dir = dir.join(skill.name);
        fs::create_dir_all(&skill_dir)
            .with_context(|| format!("create {}", skill_dir.display()))?;
        let dest = skill_dir.join("SKILL.md");
        fs::write(&dest, skill.content)
            .with_context(|| format!("write {}", dest.display()))?;
        written.push(dest.to_string_lossy().into_owned());
    }
    Ok(written)
}

pub fn run(override_dir: Option<&Path>) -> Result<Vec<TargetResult>> {
    let targets: Vec<(&str, PathBuf)> = match override_dir {
        Some(p) => vec![("custom", p.to_path_buf())],
        None => detect_targets(),
    };

    targets
        .into_iter()
        .map(|(harness, dir)| {
            let installed = write_skills_to(&dir)?;
            Ok(TargetResult {
                harness: harness.to_string(),
                skills_dir: dir.to_string_lossy().into_owned(),
                installed,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_harness_dirs(root: &Path, names: &[&str]) {
        for name in names {
            fs::create_dir_all(root.join(name)).unwrap();
        }
    }

    #[test]
    fn writes_skills_to_given_dir() {
        let tmp = TempDir::new().unwrap();
        let results = run(Some(tmp.path())).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].installed.len(), 3);
        for name in &["aihist-search", "aihist-show", "aihist-mcp"] {
            let skill_md = tmp.path().join(name).join("SKILL.md");
            assert!(skill_md.exists(), "{skill_md:?} not found");
            let content = fs::read_to_string(&skill_md).unwrap();
            assert!(content.contains(&format!("name: {name}")));
        }
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

        make_harness_dirs(h, &[".claude", ".config/opencode", ".codex", ".pi/agent"]);

        let old_home = std::env::var("HOME").ok();
        unsafe { std::env::set_var("HOME", h.to_str().unwrap()) };

        let targets = detect_targets();
        let names: Vec<&str> = targets.iter().map(|(n, _)| *n).collect();

        if let Some(v) = old_home {
            unsafe { std::env::set_var("HOME", v) };
        }

        assert!(names.contains(&"claude"),   "expected claude");
        assert!(names.contains(&"opencode"), "expected opencode");
        assert!(names.contains(&"codex"),    "expected codex");
        assert!(names.contains(&"pi"),       "expected pi");
    }

    #[test]
    fn detects_agents_hub() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agents/skills")).unwrap();

        let old_home = std::env::var("HOME").ok();
        unsafe { std::env::set_var("HOME", tmp.path().to_str().unwrap()) };

        let targets = detect_targets();
        let names: Vec<&str> = targets.iter().map(|(n, _)| *n).collect();

        if let Some(v) = old_home {
            unsafe { std::env::set_var("HOME", v) };
        }

        assert!(names.contains(&"agents-hub"));
    }

    #[test]
    fn skill_content_has_frontmatter() {
        assert!(SEARCH_SKILL.starts_with("---\n"));
        assert!(SHOW_SKILL.starts_with("---\n"));
        assert!(MCP_SKILL.starts_with("---\n"));
    }
}
