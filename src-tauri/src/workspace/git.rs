//! Thin wrappers over the `git` CLI (the user's own git, config and hooks).

use crate::proc;
use anyhow::{bail, Result};
use std::path::Path;
use std::time::Duration;

const T: Duration = Duration::from_secs(60);

async fn git(ws: &Path, args: &[&str]) -> Result<String> {
    let (code, out, err) = proc::output("git", args, Some(ws), T).await?;
    if code != 0 {
        bail!("git {}: {}", args.join(" "), if err.is_empty() { out } else { err });
    }
    Ok(out)
}

pub async fn is_repo(ws: &Path) -> bool {
    matches!(proc::output("git", &["rev-parse", "--is-inside-work-tree"], Some(ws), T).await, Ok((0, ref o, _)) if o == "true")
}

pub async fn init(ws: &Path) -> Result<()> {
    git(ws, &["init", "-q"]).await?;
    Ok(())
}

pub async fn current_branch(ws: &Path) -> Option<String> {
    git(ws, &["rev-parse", "--abbrev-ref", "HEAD"]).await.ok().filter(|b| b != "HEAD")
}

pub async fn head(ws: &Path) -> Option<String> {
    git(ws, &["rev-parse", "HEAD"]).await.ok()
}

/// Uncommitted changes (tracked and untracked), excluding the handoff dir.
pub async fn dirty_files(ws: &Path) -> Result<Vec<String>> {
    let out = git(ws, &["status", "--porcelain", "--untracked-files=all"]).await?;
    let handoff = format!("{}/", crate::brand::HANDOFF_DIR_NAME);
    Ok(out
        .lines()
        .filter_map(|l| l.get(3..))
        .map(|p| p.rsplit(" -> ").next().unwrap_or(p).trim_matches('"').to_string())
        .filter(|p| !p.starts_with(&handoff))
        .collect())
}

pub async fn stash(ws: &Path, message: &str) -> Result<()> {
    git(ws, &["stash", "push", "--include-untracked", "-m", message]).await?;
    Ok(())
}

/// Commits everything. Falls back to an app identity only if the user has
/// no git identity configured (so the commit can't fail on a fresh machine).
pub async fn commit_all(ws: &Path, message: &str) -> Result<Option<String>> {
    git(ws, &["add", "-A"]).await?;
    let staged = proc::output("git", &["diff", "--cached", "--quiet"], Some(ws), T).await?;
    if staged.0 == 0 {
        return Ok(None); // nothing to commit
    }
    let has_identity = git(ws, &["config", "user.email"]).await.is_ok();
    let res = if has_identity {
        git(ws, &["commit", "-q", "--no-verify", "-m", message]).await
    } else {
        let name = format!("user.name={}", crate::brand::PRODUCT_NAME);
        let email = format!("user.email={}@localhost", crate::brand::DATA_DIR_NAME);
        git(ws, &["-c", &name, "-c", &email, "commit", "-q", "--no-verify", "-m", message]).await
    };
    res?;
    Ok(head(ws).await)
}

/// Creates the run's working branch; an unborn repo gets an initial commit first.
pub async fn start_branch(ws: &Path, branch: &str) -> Result<String> {
    if head(ws).await.is_none() {
        commit_empty(ws, "Initial commit").await?;
    }
    git(ws, &["switch", "-q", "-c", branch]).await?;
    head(ws).await.ok_or_else(|| anyhow::anyhow!("no HEAD after branching"))
}

async fn commit_empty(ws: &Path, message: &str) -> Result<()> {
    let has_identity = git(ws, &["config", "user.email"]).await.is_ok();
    if has_identity {
        git(ws, &["commit", "-q", "--allow-empty", "--no-verify", "-m", message]).await?;
    } else {
        let name = format!("user.name={}", crate::brand::PRODUCT_NAME);
        let email = format!("user.email={}@localhost", crate::brand::DATA_DIR_NAME);
        git(ws, &["-c", &name, "-c", &email, "commit", "-q", "--allow-empty", "--no-verify", "-m", message]).await?;
    }
    Ok(())
}

/// Hard-resets the working branch to `rev` (used for per-step rollback,
/// only on the run's own branch).
pub async fn reset_hard(ws: &Path, rev: &str) -> Result<()> {
    git(ws, &["reset", "-q", "--hard", rev]).await?;
    git(ws, &["clean", "-fdq", "-e", crate::brand::HANDOFF_DIR_NAME]).await?;
    Ok(())
}

/// Throws away uncommitted edits (a failed attempt) before retrying.
pub async fn discard_changes(ws: &Path) -> Result<()> {
    reset_hard(ws, "HEAD").await
}

pub async fn changed_since(ws: &Path, rev: &str) -> Result<Vec<String>> {
    let out = git(ws, &["diff", "--name-only", rev]).await?;
    let mut files: Vec<String> = out.lines().map(String::from).collect();
    let untracked = git(ws, &["ls-files", "--others", "--exclude-standard"]).await?;
    files.extend(untracked.lines().map(String::from));
    let handoff = format!("{}/", crate::brand::HANDOFF_DIR_NAME);
    files.retain(|f| !f.starts_with(&handoff));
    files.sort();
    files.dedup();
    Ok(files)
}

pub async fn diff_stat(ws: &Path, rev: &str) -> String {
    git(ws, &["diff", "--stat", rev]).await.unwrap_or_default()
}

pub async fn switch(ws: &Path, branch: &str) -> Result<()> {
    git(ws, &["switch", "-q", branch]).await?;
    Ok(())
}

/// Adds patterns to `.git/info/exclude` inside a managed block (local only,
/// never touches the user's .gitignore).
pub fn set_excludes(ws: &Path, patterns: &[String]) -> Result<()> {
    let p = ws.join(".git/info/exclude");
    if !ws.join(".git").is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(p.parent().unwrap())?;
    let cur = std::fs::read_to_string(&p).unwrap_or_default();
    let block = patterns.join("\n");
    let next = crate::library::sync::replace_block(&cur, &block, "#");
    if next != cur {
        std::fs::write(&p, next)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        init(d.path()).await.unwrap();
        d
    }

    #[tokio::test]
    async fn branch_commit_and_rollback() {
        let d = repo().await;
        let ws = d.path();
        assert!(is_repo(ws).await);
        std::fs::write(ws.join("a.txt"), "1").unwrap();
        commit_all(ws, "base").await.unwrap();
        let base = start_branch(ws, "orchestrator/test").await.unwrap();
        assert_eq!(current_branch(ws).await.as_deref(), Some("orchestrator/test"));
        std::fs::write(ws.join("b.txt"), "2").unwrap();
        assert_eq!(dirty_files(ws).await.unwrap(), vec!["b.txt"]);
        let c1 = commit_all(ws, "step 1").await.unwrap().unwrap();
        assert_ne!(c1, base);
        assert_eq!(changed_since(ws, &base).await.unwrap(), vec!["b.txt"]);
        reset_hard(ws, &base).await.unwrap();
        assert!(!ws.join("b.txt").exists());
        assert_eq!(commit_all(ws, "nothing").await.unwrap(), None);
    }

    #[tokio::test]
    async fn unborn_repo_gets_initial_commit() {
        let d = repo().await;
        let base = start_branch(d.path(), "orchestrator/x").await.unwrap();
        assert!(!base.is_empty());
    }
}
