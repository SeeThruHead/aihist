use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

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

fn skills_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    Path::new(&home).join(".agents/skills")
}

pub fn run(skills_root: Option<&Path>) -> Result<Vec<String>> {
    let root = skills_root
        .map(|p| p.to_path_buf())
        .unwrap_or_else(skills_dir);

    let mut installed = Vec::new();

    for skill in SKILLS {
        let dir = root.join(skill.name);
        fs::create_dir_all(&dir)
            .with_context(|| format!("create {}", dir.display()))?;

        let dest = dir.join("SKILL.md");
        fs::write(&dest, skill.content)
            .with_context(|| format!("write {}", dest.display()))?;

        installed.push(dest.to_string_lossy().into_owned());
    }

    Ok(installed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn installs_all_three_skills() {
        let tmp = TempDir::new().unwrap();
        let installed = run(Some(tmp.path())).unwrap();
        assert_eq!(installed.len(), 3);
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
        let result = run(Some(tmp.path()));
        assert!(result.is_ok());
    }

    #[test]
    fn skill_content_has_frontmatter() {
        assert!(SEARCH_SKILL.starts_with("---\n"));
        assert!(SHOW_SKILL.starts_with("---\n"));
        assert!(MCP_SKILL.starts_with("---\n"));
    }

    #[test]
    fn skill_content_non_empty() {
        assert!(SEARCH_SKILL.len() > 200);
        assert!(SHOW_SKILL.len() > 200);
        assert!(MCP_SKILL.len() > 200);
    }
}
