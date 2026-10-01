//! Shared Library of rules, skills and agents. Markdown + YAML frontmatter
//! on disk, in two scopes (global `<data>/library`, workspace
//! `.orchestrator/library`); a workspace item with the same id overrides the
//! global one. `library::sync` writes the resolved set into each CLI's
//! native format.

pub mod import;
pub mod sync;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Rule,
    Skill,
    Agent,
}

impl Kind {
    pub fn dir(&self) -> &'static str {
        match self {
            Kind::Rule => "rules",
            Kind::Skill => "skills",
            Kind::Agent => "agents",
        }
    }
    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            "rule" | "rules" => Some(Kind::Rule),
            "skill" | "skills" => Some(Kind::Skill),
            "agent" | "agents" => Some(Kind::Agent),
            _ => None,
        }
    }
    pub fn all() -> [Kind; 3] {
        [Kind::Rule, Kind::Skill, Kind::Agent]
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Global,
    Workspace,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Item {
    /// Stable slug; never shown as the name, never changed by a rename.
    pub id: String,
    pub kind: Kind,
    pub scope: Scope,
    pub display_name: String,
    pub description: String,
    pub enabled: bool,
    /// The canonical file (`SKILL.md` for skills).
    pub path: String,
    pub body: String,
    /// Full frontmatter (unknown keys are preserved on save).
    #[serde(skip)]
    pub meta: Mapping,
    pub applies_to: Vec<String>,
    pub always_on: bool,
    pub tier: Option<String>,
    pub model: Option<String>,
    pub tools: Vec<String>,
    pub skills: Vec<String>,
    pub rules: Vec<String>,
    pub updated_at: i64,
    /// A workspace item with the same id shadows this global one.
    pub overridden: bool,
    /// Raw file text (frontmatter + body), for the editor.
    pub raw: String,
}

impl Item {
    pub fn key(&self) -> String {
        format!("{}:{}", self.kind.dir(), self.id)
    }
    /// Skill directory (for supporting files).
    pub fn skill_dir(&self) -> Option<PathBuf> {
        (self.kind == Kind::Skill).then(|| Path::new(&self.path).parent().map(Path::to_path_buf)).flatten()
    }
}

pub fn global_root(data_dir: &Path) -> PathBuf {
    data_dir.join("library")
}

pub fn workspace_root(ws: &Path) -> PathBuf {
    crate::handoff::dir(ws).join("library")
}

pub fn root_for(scope: Scope, data_dir: &Path, ws: Option<&Path>) -> Result<PathBuf> {
    match (scope, ws) {
        (Scope::Global, _) => Ok(global_root(data_dir)),
        (Scope::Workspace, Some(w)) => Ok(workspace_root(w)),
        (Scope::Workspace, None) => bail!("no workspace selected"),
    }
}

/// Splits `---\nyaml\n---\nbody` into (frontmatter, body).
pub fn split_frontmatter(s: &str) -> (Mapping, String) {
    let t = s.strip_prefix('\u{feff}').unwrap_or(s);
    if let Some(rest) = t.strip_prefix("---\n").or_else(|| t.strip_prefix("---\r\n")) {
        if let Some(end) = rest.find("\n---") {
            let yaml = &rest[..end];
            let after = &rest[end + 4..];
            let body = after.split_once('\n').map(|(_, b)| b).unwrap_or("");
            let map = serde_yaml::from_str::<Mapping>(yaml).unwrap_or_default();
            return (map, body.trim_start_matches(['\r', '\n']).to_string());
        }
    }
    (Mapping::new(), t.to_string())
}

pub fn join_frontmatter(meta: &Mapping, body: &str) -> String {
    if meta.is_empty() {
        return body.to_string();
    }
    let yaml = serde_yaml::to_string(meta).unwrap_or_default();
    format!("---\n{}---\n\n{}", yaml, body.trim_start())
}

fn get_str(m: &Mapping, k: &str) -> Option<String> {
    match m.get(k)? {
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn get_list(m: &Mapping, k: &str) -> Vec<String> {
    match m.get(k) {
        Some(Value::Sequence(s)) => s.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
        Some(Value::String(s)) => s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
        _ => vec![],
    }
}

fn get_bool(m: &Mapping, k: &str) -> Option<bool> {
    match m.get(k)? {
        Value::Bool(b) => Some(*b),
        Value::String(s) => Some(s == "true"),
        _ => None,
    }
}

pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "item".into()
    } else {
        out.chars().take(64).collect()
    }
}

pub fn parse_item(path: &Path, kind: Kind, scope: Scope) -> Result<Item> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let (meta, body) = split_frontmatter(&raw);
    let id = match kind {
        Kind::Skill => path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        _ => path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
    };
    let display_name =
        get_str(&meta, "displayName").or_else(|| get_str(&meta, "name").filter(|_| kind != Kind::Skill)).unwrap_or_else(|| humanize(&id));
    let updated_at = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Ok(Item {
        description: get_str(&meta, "description").unwrap_or_default(),
        enabled: get_bool(&meta, "enabled").unwrap_or(true),
        applies_to: get_list(&meta, "appliesTo"),
        always_on: get_bool(&meta, "alwaysOn").unwrap_or(kind == Kind::Rule && get_list(&meta, "appliesTo").is_empty()),
        tier: get_str(&meta, "tier"),
        model: get_str(&meta, "model"),
        tools: get_list(&meta, "tools"),
        skills: get_list(&meta, "skills"),
        rules: get_list(&meta, "rules"),
        path: path.to_string_lossy().into_owned(),
        id,
        kind,
        scope,
        display_name,
        body,
        meta,
        updated_at,
        overridden: false,
        raw,
    })
}

pub fn humanize(id: &str) -> String {
    let s = id.replace(['-', '_'], " ");
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn scan_root(root: &Path, scope: Scope) -> Vec<Item> {
    let mut out = vec![];
    for kind in Kind::all() {
        let dir = root.join(kind.dir());
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let item = match kind {
                Kind::Skill if p.is_dir() => parse_item(&p.join("SKILL.md"), kind, scope),
                Kind::Rule | Kind::Agent if p.extension().is_some_and(|x| x == "md") => parse_item(&p, kind, scope),
                _ => continue,
            };
            if let Ok(i) = item {
                out.push(i);
            }
        }
    }
    out
}

/// Every item in both scopes; global items shadowed by a workspace item are marked `overridden`.
pub fn list(data_dir: &Path, ws: Option<&Path>) -> Vec<Item> {
    let mut items = scan_root(&global_root(data_dir), Scope::Global);
    if let Some(w) = ws {
        let wsi = scan_root(&workspace_root(w), Scope::Workspace);
        for g in items.iter_mut() {
            if wsi.iter().any(|x| x.kind == g.kind && x.id == g.id) {
                g.overridden = true;
            }
        }
        items.extend(wsi);
    }
    items.sort_by(|a, b| (a.kind, a.display_name.to_lowercase()).cmp(&(b.kind, b.display_name.to_lowercase())));
    items
}

/// The effective set for a run: enabled, not overridden, and in the profile (if any).
pub fn resolve(items: &[Item], profile: Option<&[String]>) -> Vec<Item> {
    items
        .iter()
        .filter(|i| i.enabled && !i.overridden)
        .filter(|i| profile.is_none_or(|p| p.iter().any(|k| *k == i.key())))
        .cloned()
        .collect()
}

/// Hash of exactly what a run used, so it can be reproduced.
pub fn snapshot_hash(items: &[Item]) -> String {
    let mut sorted: Vec<&Item> = items.iter().collect();
    sorted.sort_by_key(|i| i.key());
    let mut h = Sha256::new();
    for i in sorted {
        h.update(i.key().as_bytes());
        h.update([0]);
        h.update(i.raw.as_bytes());
        h.update([0]);
    }
    hex::encode(h.finalize())[..16].to_string()
}

fn template(kind: Kind, id: &str, display: &str) -> String {
    match kind {
        Kind::Rule => format!(
            "---\ndisplayName: {display:?}\nalwaysOn: true\n# appliesTo: [\"src/**/*.ts\"]\n---\n\nDescribe the rule in plain language. For example: \"Use named exports; never default exports.\"\n"
        ),
        Kind::Skill => format!(
            "---\nname: {id}\ndescription: {:?}\ndisplayName: {display:?}\n---\n\n# {display}\n\n## When to use\nDescribe when an agent should use this skill.\n\n## Steps\n1. …\n",
            format!("Use when you need to {}.", display.to_lowercase())
        ),
        Kind::Agent => format!(
            "---\ndisplayName: {display:?}\ndescription: \"What this specialist does and when to pick it.\"\ntier: any\ntools: []\nskills: []\nrules: []\n---\n\nYou are a focused specialist. Describe the role, what good output looks like, and what to avoid.\n"
        ),
    }
}

fn item_path(root: &Path, kind: Kind, id: &str) -> PathBuf {
    match kind {
        Kind::Skill => root.join("skills").join(id).join("SKILL.md"),
        _ => root.join(kind.dir()).join(format!("{id}.md")),
    }
}

fn unique_id(root: &Path, kind: Kind, base: &str) -> String {
    let mut id = base.to_string();
    let mut n = 2;
    while item_path(root, kind, &id).exists() {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

pub fn create(root: &Path, kind: Kind, scope: Scope, display_name: &str) -> Result<Item> {
    let id = unique_id(root, kind, &slugify(display_name));
    let p = item_path(root, kind, &id);
    std::fs::create_dir_all(p.parent().unwrap())?;
    std::fs::write(&p, template(kind, &id, display_name))?;
    parse_item(&p, kind, scope)
}

/// Saves raw editor text for an item.
pub fn save_raw(item_path: &Path, raw: &str) -> Result<()> {
    let (_, _) = split_frontmatter(raw);
    std::fs::write(item_path, raw)?;
    Ok(())
}

fn update_meta(item: &Item, f: impl FnOnce(&mut Mapping)) -> Result<()> {
    let raw = std::fs::read_to_string(&item.path)?;
    let (mut meta, body) = split_frontmatter(&raw);
    f(&mut meta);
    std::fs::write(&item.path, join_frontmatter(&meta, &body))?;
    Ok(())
}

pub fn rename(item: &Item, display_name: &str) -> Result<()> {
    let name = display_name.trim();
    if name.is_empty() {
        bail!("name can't be empty");
    }
    update_meta(item, |m| {
        m.insert("displayName".into(), name.into());
    })
}

pub fn set_enabled(item: &Item, enabled: bool) -> Result<()> {
    update_meta(item, |m| {
        if enabled {
            m.remove("enabled");
        } else {
            m.insert("enabled".into(), false.into());
        }
    })
}

/// Agents that reference an item (shown before delete).
pub fn referenced_by(items: &[Item], target: &Item) -> Vec<String> {
    if target.kind == Kind::Agent {
        return vec![];
    }
    items
        .iter()
        .filter(|i| i.kind == Kind::Agent)
        .filter(|a| match target.kind {
            Kind::Skill => a.skills.contains(&target.id),
            Kind::Rule => a.rules.contains(&target.id),
            Kind::Agent => false,
        })
        .map(|a| a.display_name.clone())
        .collect()
}

pub fn delete(item: &Item) -> Result<()> {
    match item.skill_dir() {
        Some(d) => std::fs::remove_dir_all(d)?,
        None => std::fs::remove_file(&item.path)?,
    }
    Ok(())
}

/// Copies an item into `dest_root` (duplicate within a scope, or move/copy across scopes).
pub fn copy_to(item: &Item, dest_root: &Path, new_display: Option<&str>) -> Result<PathBuf> {
    let base = if new_display.is_some() { format!("{}-copy", item.id) } else { item.id.clone() };
    let id = unique_id(dest_root, item.kind, &base);
    let dst = item_path(dest_root, item.kind, &id);
    std::fs::create_dir_all(dst.parent().unwrap())?;
    if let Some(src_dir) = item.skill_dir() {
        copy_dir(&src_dir, dst.parent().unwrap())?;
    } else {
        std::fs::copy(&item.path, &dst)?;
    }
    let raw = std::fs::read_to_string(&dst)?;
    let (mut meta, body) = split_frontmatter(&raw);
    if let Some(n) = new_display {
        meta.insert("displayName".into(), n.into());
    }
    if item.kind == Kind::Skill {
        meta.insert("name".into(), id.clone().into());
    }
    std::fs::write(&dst, join_frontmatter(&meta, &body))?;
    Ok(dst)
}

pub fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)?.flatten() {
        let p = e.path();
        let d = dst.join(e.file_name());
        if p.is_dir() {
            copy_dir(&p, &d)?;
        } else {
            std::fs::copy(&p, &d)?;
        }
    }
    Ok(())
}

/// Copies the starter pack into the global library the first time.
pub fn install_starter(data_dir: &Path, starter: &Path) -> Result<bool> {
    let root = global_root(data_dir);
    let marker = root.join(".starter-installed");
    if marker.exists() || !starter.is_dir() {
        return Ok(false);
    }
    copy_dir(starter, &root)?;
    std::fs::write(marker, "1")?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_roundtrip_preserves_unknown_keys() {
        let raw = "---\ndisplayName: Test writer\ncustom: 42\ntools: [Read, Edit]\n---\n\nBody here\n";
        let (meta, body) = split_frontmatter(raw);
        assert_eq!(get_str(&meta, "displayName").as_deref(), Some("Test writer"));
        assert_eq!(get_list(&meta, "tools"), vec!["Read", "Edit"]);
        let back = join_frontmatter(&meta, &body);
        assert!(back.contains("custom: 42"));
        assert!(back.ends_with("Body here\n"));
    }

    #[test]
    fn create_rename_toggle_delete() {
        let d = tempfile::tempdir().unwrap();
        let root = global_root(d.path());
        let r = create(&root, Kind::Rule, Scope::Global, "No default exports").unwrap();
        assert_eq!(r.id, "no-default-exports");
        assert!(r.always_on);
        rename(&r, "Named exports only").unwrap();
        let items = list(d.path(), None);
        assert_eq!(items[0].display_name, "Named exports only");
        assert_eq!(items[0].id, "no-default-exports", "rename keeps the id");
        set_enabled(&items[0], false).unwrap();
        assert!(resolve(&list(d.path(), None), None).is_empty());
        let s = create(&root, Kind::Skill, Scope::Global, "Write migration").unwrap();
        assert!(s.path.ends_with("skills/write-migration/SKILL.md"));
        let dup = create(&root, Kind::Skill, Scope::Global, "Write migration").unwrap();
        assert_eq!(dup.id, "write-migration-2");
        delete(&s).unwrap();
        assert!(!Path::new(&s.path).exists());
    }

    #[test]
    fn workspace_overrides_global() {
        let data = tempfile::tempdir().unwrap();
        let ws = tempfile::tempdir().unwrap();
        create(&global_root(data.path()), Kind::Rule, Scope::Global, "Style").unwrap();
        create(&workspace_root(ws.path()), Kind::Rule, Scope::Workspace, "Style").unwrap();
        let items = list(data.path(), Some(ws.path()));
        assert_eq!(items.len(), 2);
        let eff = resolve(&items, None);
        assert_eq!(eff.len(), 1);
        assert_eq!(eff[0].scope, Scope::Workspace);
        let h1 = snapshot_hash(&eff);
        assert_eq!(h1, snapshot_hash(&eff));
    }

    #[test]
    fn references_are_listed_before_delete() {
        let d = tempfile::tempdir().unwrap();
        let root = global_root(d.path());
        let s = create(&root, Kind::Skill, Scope::Global, "Write migration").unwrap();
        let a = create(&root, Kind::Agent, Scope::Global, "DB helper").unwrap();
        update_meta(&a, |m| {
            m.insert("skills".into(), Value::Sequence(vec!["write-migration".into()]));
        })
        .unwrap();
        let items = list(d.path(), None);
        assert_eq!(referenced_by(&items, &s), vec!["DB helper"]);
    }
}
