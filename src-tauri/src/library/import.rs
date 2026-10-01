//! Import existing agent configuration from a workspace into the Library:
//! `.cursor/rules`, `.claude/skills`, `.claude/agents`, `CLAUDE.md`,
//! `AGENTS.md`, Copilot instructions and `.goosehints`. Files we generated
//! ourselves (listed in managed.json) and our managed blocks are skipped.

use super::sync::{load_managed, outside_block};
use super::{copy_dir, join_frontmatter, slugify, split_frontmatter, Kind};
use anyhow::Result;
use serde_yaml::{Mapping, Value};
use std::path::Path;

const SHARED: &[(&str, &str)] = &[
    ("CLAUDE.md", "From CLAUDE.md"),
    ("AGENTS.md", "From AGENTS.md"),
    (".github/copilot-instructions.md", "From Copilot instructions"),
    (".goosehints", "From .goosehints"),
];

fn ours(ws: &Path, rel: &str) -> bool {
    load_managed(ws).files.contains_key(rel)
}

fn list_dir(ws: &Path, rel: &str, f: impl Fn(&Path) -> bool) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(ws.join(rel)) else { return vec![] };
    let mut v: Vec<String> = rd
        .flatten()
        .filter(|e| f(&e.path()))
        .map(|e| format!("{rel}/{}", e.file_name().to_string_lossy()))
        .filter(|r| !ours(ws, r) && !ours(ws, &format!("{r}/SKILL.md")))
        .collect();
    v.sort();
    v
}

/// Human-readable list of what can be imported (empty = nothing to offer).
pub fn candidates(ws: &Path) -> Vec<String> {
    let mut out = vec![];
    let rules = list_dir(ws, ".cursor/rules", |p| p.extension().is_some_and(|x| x == "mdc" || x == "md"));
    if !rules.is_empty() {
        out.push(format!(".cursor/rules ({})", rules.len()));
    }
    let skills = list_dir(ws, ".claude/skills", |p| p.join("SKILL.md").exists());
    if !skills.is_empty() {
        out.push(format!(".claude/skills ({})", skills.len()));
    }
    let agents = list_dir(ws, ".claude/agents", |p| p.extension().is_some_and(|x| x == "md"));
    if !agents.is_empty() {
        out.push(format!(".claude/agents ({})", agents.len()));
    }
    for (rel, _) in SHARED {
        if let Ok(s) = std::fs::read_to_string(ws.join(rel)) {
            if !outside_block(&s).is_empty() {
                out.push(rel.to_string());
            }
        }
    }
    out
}

fn write_item(root: &Path, kind: Kind, id: &str, meta: Mapping, body: &str) -> Result<bool> {
    let p = match kind {
        Kind::Skill => root.join("skills").join(id).join("SKILL.md"),
        _ => root.join(kind.dir()).join(format!("{id}.md")),
    };
    if p.exists() {
        return Ok(false); // never overwrite an existing Library item
    }
    std::fs::create_dir_all(p.parent().unwrap())?;
    std::fs::write(p, join_frontmatter(&meta, body))?;
    Ok(true)
}

/// Imports everything found into `root` (a Library scope). Returns the count imported.
pub fn import_all(ws: &Path, root: &Path) -> Result<usize> {
    let mut n = 0;
    for rel in list_dir(ws, ".cursor/rules", |p| p.extension().is_some_and(|x| x == "mdc" || x == "md")) {
        let raw = std::fs::read_to_string(ws.join(&rel))?;
        let (fm, body) = split_frontmatter(&raw);
        let id = slugify(Path::new(&rel).file_stem().unwrap().to_string_lossy().as_ref());
        let mut meta = Mapping::new();
        let desc = fm.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
        meta.insert("displayName".into(), super::humanize(&id).into());
        if !desc.is_empty() {
            meta.insert("description".into(), desc.into());
        }
        let globs: Vec<Value> = match fm.get("globs") {
            Some(Value::String(s)) => s.split(',').map(|g| Value::String(g.trim().into())).filter(|g| g.as_str() != Some("")).collect(),
            Some(Value::Sequence(s)) => s.clone(),
            _ => vec![],
        };
        let always = fm.get("alwaysApply").and_then(|v| v.as_bool()).unwrap_or(globs.is_empty());
        if !globs.is_empty() {
            meta.insert("appliesTo".into(), Value::Sequence(globs));
        }
        meta.insert("alwaysOn".into(), always.into());
        n += write_item(root, Kind::Rule, &id, meta, &body)? as usize;
    }
    for rel in list_dir(ws, ".claude/skills", |p| p.join("SKILL.md").exists()) {
        let id = slugify(Path::new(&rel).file_name().unwrap().to_string_lossy().as_ref());
        let dst = root.join("skills").join(&id);
        if !dst.exists() {
            copy_dir(&ws.join(&rel), &dst)?;
            n += 1;
        }
    }
    for rel in list_dir(ws, ".claude/agents", |p| p.extension().is_some_and(|x| x == "md")) {
        let raw = std::fs::read_to_string(ws.join(&rel))?;
        let (fm, body) = split_frontmatter(&raw);
        let id = slugify(fm.get("name").and_then(|v| v.as_str()).unwrap_or_else(|| Path::new(&rel).file_stem().unwrap().to_str().unwrap_or("agent")));
        let mut meta = Mapping::new();
        meta.insert("displayName".into(), super::humanize(&id).into());
        meta.insert("description".into(), fm.get("description").cloned().unwrap_or(Value::String(String::new())));
        meta.insert("tier".into(), "any".into());
        if let Some(t) = fm.get("tools").and_then(|v| v.as_str()) {
            let tools: Vec<Value> = t.split(',').map(|x| Value::String(x.trim().into())).collect();
            meta.insert("tools".into(), Value::Sequence(tools));
        }
        if let Some(m) = fm.get("model") {
            meta.insert("model".into(), m.clone());
        }
        n += write_item(root, Kind::Agent, &id, meta, &body)? as usize;
    }
    for (rel, label) in SHARED {
        let Ok(s) = std::fs::read_to_string(ws.join(rel)) else { continue };
        let text = outside_block(&s);
        if text.is_empty() {
            continue;
        }
        let id = format!("imported-{}", slugify(rel.trim_start_matches('.')));
        let mut meta = Mapping::new();
        meta.insert("displayName".into(), (*label).into());
        meta.insert("alwaysOn".into(), true.into());
        n += write_item(root, Kind::Rule, &id, meta, &text)? as usize;
    }
    Ok(n)
}

/// Pulls a user's edit of a generated file back into the Library item it
/// came from (the "import instead of overwrite" path).
pub fn import_edit(ws: &Path, rel: &str, items: &[super::Item]) -> Result<Option<String>> {
    let raw = std::fs::read_to_string(ws.join(rel))?;
    let (_, body) = split_frontmatter(&raw);
    let body = body
        .lines()
        .filter(|l| !(l.starts_with("<!-- Generated by ") && l.ends_with("-->")))
        .collect::<Vec<_>>()
        .join("\n");
    let stem = Path::new(rel).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = stem.trim_end_matches(".agent").trim_start_matches("agent-").to_string();
    let id = if rel.ends_with("SKILL.md") {
        Path::new(rel).parent().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
    } else {
        stem
    };
    let Some(item) = items.iter().find(|i| i.id == id && !i.overridden) else { return Ok(None) };
    let cur = std::fs::read_to_string(&item.path)?;
    let (meta, _) = split_frontmatter(&cur);
    std::fs::write(&item.path, join_frontmatter(&meta, body.trim()))?;
    // Forget the old hash so the next sync rewrites the file from the Library.
    let mut m = load_managed(ws);
    m.files.remove(rel);
    std::fs::write(super::sync::managed_path(ws), serde_json::to_vec_pretty(&m)?)?;
    let _ = std::fs::remove_file(ws.join(rel));
    Ok(Some(item.display_name.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{list, workspace_root};

    #[test]
    fn imports_cursor_rules_claude_assets_and_shared_files() {
        let ws = tempfile::tempdir().unwrap();
        let w = ws.path();
        std::fs::create_dir_all(w.join(".cursor/rules")).unwrap();
        std::fs::write(w.join(".cursor/rules/style.mdc"), "---\ndescription: Style\nglobs: src/**/*.ts\nalwaysApply: false\n---\nUse tabs.\n").unwrap();
        std::fs::create_dir_all(w.join(".claude/skills/deploy")).unwrap();
        std::fs::write(w.join(".claude/skills/deploy/SKILL.md"), "---\nname: deploy\ndescription: Deploy it\n---\nSteps\n").unwrap();
        std::fs::create_dir_all(w.join(".claude/agents")).unwrap();
        std::fs::write(w.join(".claude/agents/reviewer.md"), "---\nname: reviewer\ndescription: Reviews\ntools: Read, Grep\n---\nReview code.\n").unwrap();
        std::fs::write(w.join("AGENTS.md"), "Always run tests.\n").unwrap();
        let c = candidates(w);
        assert_eq!(c.len(), 4, "{c:?}");
        let root = workspace_root(w);
        let n = import_all(w, &root).unwrap();
        assert_eq!(n, 4);
        let data = tempfile::tempdir().unwrap();
        let items = list(data.path(), Some(w));
        let style = items.iter().find(|i| i.id == "style").unwrap();
        assert_eq!(style.applies_to, vec!["src/**/*.ts"]);
        assert!(!style.always_on);
        let rev = items.iter().find(|i| i.id == "reviewer").unwrap();
        assert_eq!(rev.tools, vec!["Read", "Grep"]);
        assert!(items.iter().any(|i| i.id == "imported-agents-md"));
        // Importing again doesn't duplicate.
        assert_eq!(import_all(w, &root).unwrap(), 0);
    }
}
