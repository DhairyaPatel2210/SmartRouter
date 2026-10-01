//! Context handoff through files in `<workspace>/.orchestrator/`: the plan,
//! one short task file per step, progress and failure notes. Agents never
//! share sessions; these files are the shared context every CLI can read.

pub mod plan;

use crate::brand::HANDOFF_DIR_NAME;
use anyhow::Result;
use std::path::{Path, PathBuf};

pub use plan::{parse_plan, PlanTask};

/// Task files target < 2k tokens (~4 chars/token) so small models keep the prompt.
pub const TASK_FILE_BUDGET_CHARS: usize = 7_500;

pub fn dir(ws: &Path) -> PathBuf {
    ws.join(HANDOFF_DIR_NAME)
}

pub fn plan_path(ws: &Path) -> PathBuf {
    dir(ws).join("plan.md")
}

pub fn progress_path(ws: &Path) -> PathBuf {
    dir(ws).join("progress.md")
}

pub fn task_path(ws: &Path, n: i64) -> PathBuf {
    dir(ws).join("tasks").join(format!("{n}.md"))
}

pub fn rel(ws: &Path, p: &Path) -> String {
    p.strip_prefix(ws).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

/// Creates the handoff dir with its own `.gitignore` that ignores everything
/// except the shareable `library/` and `workflows/` folders.
pub fn ensure(ws: &Path) -> Result<()> {
    let d = dir(ws);
    std::fs::create_dir_all(d.join("tasks"))?;
    std::fs::create_dir_all(d.join("failures"))?;
    let gi = d.join(".gitignore");
    if !gi.exists() {
        std::fs::write(
            gi,
            format!(
                "# Managed by {}: run files stay local; the Library and workflows can be committed.\n*\n!.gitignore\n!library/\n!library/**\n!workflows/\n!workflows/**\n",
                crate::brand::PRODUCT_NAME
            ),
        )?;
    }
    Ok(())
}

/// Clears per-run files (plan, tasks, progress, failures) before a new run.
pub fn reset_run_files(ws: &Path) -> Result<()> {
    let d = dir(ws);
    for f in ["plan.md", "progress.md", "review.md"] {
        let _ = std::fs::remove_file(d.join(f));
    }
    for sub in ["tasks", "failures", ".fake"] {
        let _ = std::fs::remove_dir_all(d.join(sub));
    }
    ensure(ws)
}

pub struct TaskInput<'a> {
    pub n: i64,
    pub total: usize,
    pub goal: &'a str,
    pub task: &'a PlanTask,
    pub plan_excerpt: String,
    pub progress_tail: String,
    pub checks: &'a [String],
    /// Library role (agent system prompt), rules and skills for CLIs that
    /// can't receive them natively (fallback injection).
    pub role: Option<String>,
    pub rules: Vec<String>,
    pub skills: Vec<(String, String, String)>,
    pub failure_notes: Option<String>,
}

pub fn write_task(ws: &Path, t: &TaskInput) -> Result<PathBuf> {
    ensure(ws)?;
    let mut s = String::with_capacity(4096);
    s.push_str(&format!("# Task {} of {}: {}\n\n", t.n, t.total, t.task.title));
    s.push_str(&format!("**Overall goal:** {}\n\n", one_line(t.goal, 400)));
    if !t.task.details.trim().is_empty() {
        s.push_str("## What to do\n");
        s.push_str(t.task.details.trim());
        s.push_str("\n\n");
    }
    if !t.task.files.is_empty() {
        s.push_str("## Files likely touched\n");
        for f in &t.task.files {
            s.push_str(&format!("- {f}\n"));
        }
        s.push('\n');
    }
    s.push_str("## Constraints\n");
    s.push_str("- Work only inside this directory. Do not touch files unrelated to this task.\n");
    s.push_str("- Do only this task; later tasks are handled separately.\n");
    s.push_str(&format!("- Don't edit files in `{HANDOFF_DIR_NAME}/` (they are managed for you).\n"));
    s.push_str("- Do not commit; the orchestrator commits after checks pass.\n\n");
    let mut checks: Vec<String> = t.checks.to_vec();
    if let Some(c) = &t.task.check {
        checks.insert(0, c.clone());
    }
    if !checks.is_empty() {
        s.push_str("## Must pass\n");
        for c in &checks {
            s.push_str(&format!("- `{c}`\n"));
        }
        s.push('\n');
    }
    if let Some(role) = &t.role {
        s.push_str("## Role\n");
        s.push_str(&clip(role, 1500));
        s.push_str("\n\n");
    }
    if !t.rules.is_empty() {
        s.push_str("## Rules\n");
        for r in &t.rules {
            s.push_str(&clip(r, 800));
            s.push('\n');
        }
        s.push('\n');
    }
    if !t.skills.is_empty() {
        s.push_str("## Skills available\n");
        for (name, desc, path) in &t.skills {
            s.push_str(&format!("- **{name}**: {} (read `{path}` when relevant)\n", one_line(desc, 200)));
        }
        s.push('\n');
    }
    if let Some(f) = &t.failure_notes {
        s.push_str("## Previous attempt failed\n");
        s.push_str(&clip(f, 1800));
        s.push_str("\n\n");
    }
    if !t.progress_tail.trim().is_empty() {
        s.push_str("## Done so far\n");
        s.push_str(&clip(&t.progress_tail, 1200));
        s.push_str("\n\n");
    }
    if !t.plan_excerpt.trim().is_empty() && s.len() < TASK_FILE_BUDGET_CHARS - 400 {
        s.push_str("## Plan context\n");
        let room = TASK_FILE_BUDGET_CHARS.saturating_sub(s.len() + 50);
        s.push_str(&clip(&t.plan_excerpt, room.max(200)));
        s.push('\n');
    }
    let p = task_path(ws, t.n);
    std::fs::create_dir_all(p.parent().unwrap())?;
    std::fs::write(&p, s)?;
    Ok(p)
}

pub fn append_progress(ws: &Path, line: &str) -> Result<()> {
    use std::io::Write;
    ensure(ws)?;
    let p = progress_path(ws);
    let new = !p.exists();
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p)?;
    if new {
        writeln!(f, "# Progress\n")?;
    }
    writeln!(f, "{line}")?;
    Ok(())
}

pub fn progress_tail(ws: &Path, lines: usize) -> String {
    let s = std::fs::read_to_string(progress_path(ws)).unwrap_or_default();
    let v: Vec<&str> = s.lines().filter(|l| l.starts_with("- ")).collect();
    v[v.len().saturating_sub(lines)..].join("\n")
}

pub fn write_failure(ws: &Path, n: i64, attempt: i64, body: &str) -> Result<PathBuf> {
    ensure(ws)?;
    let p = dir(ws).join("failures").join(format!("{n}-{attempt}.md"));
    std::fs::write(&p, body)?;
    Ok(p)
}

/// The excerpt of the plan around task `n` (previous, current and next).
pub fn plan_excerpt(tasks: &[PlanTask], n: i64) -> String {
    tasks
        .iter()
        .filter(|t| (t.n - n).abs() <= 1)
        .map(|t| {
            let mark = if t.n == n { " ← this task" } else { "" };
            format!("{}. {}{}", t.n, t.title, mark)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn one_line(s: &str, max: usize) -> String {
    crate::adapters::truncate(&s.split_whitespace().collect::<Vec<_>>().join(" "), max)
}

fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n…(truncated)", &s[..end])
}

// ---------- prompts ----------

pub struct AgentBrief {
    pub id: String,
    pub description: String,
    pub tier: String,
}

/// Instruction for the planner. It must write plan.md in a fixed format so the
/// core can parse it.
pub fn planning_prompt(goal: &str, short: bool, agents: &[AgentBrief], ws_summary: &str) -> String {
    let max = if short { "3-5" } else { "3-8" };
    let mut s = format!(
        "## Planning task\n\nYou are the planner in a multi-agent coding workflow. Do NOT implement anything yet.\n\nGoal: {}\n\n\
Read the project briefly (README, AGENTS.md, key files), then write a numbered plan of {max} small, independently verifiable tasks \
to `{HANDOFF_DIR_NAME}/plan.md` using exactly this format:\n\n\
```\n# Plan\n\nGoal: <goal>\n\n1. [low] <short imperative title> — reason: <one line why low/high>\n   Files: <comma-separated paths likely touched>\n   Check: `<optional shell command that proves the task works>`\n   <optional 1-3 lines of detail>\n2. [high] ...\n```\n\n\
Tag each task [high] for design/architecture, debugging unknown failures, security/auth, concurrency, data migrations or changes \
across more than 3 files; otherwise [low]. Keep each task small enough for a small model when tagged [low].\n",
        goal.trim()
    );
    if !agents.is_empty() {
        s.push_str("\nOptionally assign a specialist by adding `[agent: <id>]` after the title. Available specialists:\n");
        for a in agents {
            s.push_str(&format!("- `{}` ({}): {}\n", a.id, a.tier, one_line(&a.description, 160)));
        }
    }
    if !ws_summary.is_empty() {
        s.push_str(&format!("\nProject hints: {ws_summary}\n"));
    }
    s.push_str(&format!("\nWrite only `{HANDOFF_DIR_NAME}/plan.md`, then reply with one line saying how many tasks you planned.\n"));
    s
}

pub fn execute_prompt(ws: &Path, task_file: &Path) -> String {
    let rel_task = rel(ws, task_file);
    let body = std::fs::read_to_string(task_file).unwrap_or_default();
    format!(
        "You are executing one step of a larger plan in this repository.\n\
First skim AGENTS.md (if present), {HANDOFF_DIR_NAME}/plan.md and {HANDOFF_DIR_NAME}/progress.md for context. \
Your task is in {rel_task} (copied below). Complete exactly this task by editing files in this directory, \
run the checks it lists if you can, then reply with a one-line summary of what you changed.\n\n---\n{body}"
    )
}

pub fn review_prompt(goal: &str, base: Option<&str>) -> String {
    let diff = match base {
        Some(b) => format!("Inspect the changes with `git diff {b}...HEAD`."),
        None => "Inspect the files listed in progress.md.".to_string(),
    };
    format!(
        "## Review task\n\nYou are the final reviewer. Goal of this run: {}\n\nRead {HANDOFF_DIR_NAME}/plan.md and {HANDOFF_DIR_NAME}/progress.md. {diff} \
Fix small, clear problems directly. Write a short verdict to `{HANDOFF_DIR_NAME}/review.md` starting with `Verdict: pass` or `Verdict: needs work`, \
followed by up to 5 bullet points. Reply with the verdict line.",
        goal.trim()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_file_stays_within_budget() {
        let d = tempfile::tempdir().unwrap();
        let task = PlanTask { n: 2, title: "Add tests".into(), details: "x".repeat(20_000), ..Default::default() };
        let checks = vec!["npm test".to_string()];
        let p = write_task(
            d.path(),
            &TaskInput {
                n: 2,
                total: 3,
                goal: "Make it better",
                task: &task,
                plan_excerpt: "y".repeat(10_000),
                progress_tail: "- step 1 done".into(),
                checks: &checks,
                role: Some("You write tests.".into()),
                rules: vec!["Use conventional commits.".into()],
                skills: vec![],
                failure_notes: None,
            },
        )
        .unwrap();
        let s = std::fs::read_to_string(p).unwrap();
        assert!(s.contains("# Task 2 of 3: Add tests"));
        assert!(s.contains("`npm test`"));
        assert!(s.contains("## Role"));
        // Long details are kept (they're the task), but the plan excerpt is squeezed.
        assert!(s.len() < 20_000 + TASK_FILE_BUDGET_CHARS);
    }

    #[test]
    fn handoff_gitignore_keeps_library() {
        let d = tempfile::tempdir().unwrap();
        ensure(d.path()).unwrap();
        let gi = std::fs::read_to_string(dir(d.path()).join(".gitignore")).unwrap();
        assert!(gi.contains("!library/"));
        assert!(gi.contains("!workflows/"));
    }

    #[test]
    fn progress_tail_returns_last_lines() {
        let d = tempfile::tempdir().unwrap();
        for i in 1..=5 {
            append_progress(d.path(), &format!("- step {i}")).unwrap();
        }
        assert_eq!(progress_tail(d.path(), 2), "- step 4\n- step 5");
    }
}
