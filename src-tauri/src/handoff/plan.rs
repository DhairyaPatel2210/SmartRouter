//! Parser for `plan.md`. Tolerant of the ways models format markdown
//! (headings, bold numbers, checkbox lists), strict about nothing.

use crate::types::StepClass;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlanTask {
    pub n: i64,
    pub title: String,
    /// Planner's tag (`[high]` / `[low]` / `[trivial]`), if any.
    pub tag: Option<StepClass>,
    pub reason: Option<String>,
    /// Library agent the planner assigned (`[agent: id]`).
    pub agent: Option<String>,
    pub files: Vec<String>,
    /// Acceptance command, when the plan names one.
    pub check: Option<String>,
    pub details: String,
}

fn task_start(line: &str) -> Option<(i64, &str)> {
    let mut t = line.trim_start();
    t = t.trim_start_matches('#').trim_start();
    t = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).unwrap_or(t);
    t = t.strip_prefix("[ ] ").or_else(|| t.strip_prefix("[x] ")).unwrap_or(t);
    let t = t.trim_start_matches("**");
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 3 {
        return None;
    }
    let rest = &t[digits.len()..];
    let rest = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')')).or_else(|| rest.strip_prefix(':'))?;
    let rest = rest.trim_start_matches("**");
    if !rest.starts_with(' ') {
        return None;
    }
    Some((digits.parse().ok()?, rest.trim()))
}

/// Extracts `[tag]`s from a title, returning (clean title, tag, agent).
fn take_tags(title: &str) -> (String, Option<StepClass>, Option<String>) {
    let mut tag = None;
    let mut agent = None;
    let mut out = String::with_capacity(title.len());
    let mut rest = title;
    while let Some(i) = rest.find('[') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let Some(j) = after.find(']') else {
            out.push_str(&rest[i..]);
            rest = "";
            break;
        };
        let inner = after[..j].trim();
        let low = inner.to_ascii_lowercase();
        match low.as_str() {
            "high" => tag = Some(StepClass::High),
            "low" => tag = Some(StepClass::Low),
            "trivial" => tag = Some(StepClass::Trivial),
            _ if low.starts_with("agent:") => agent = Some(inner[6..].trim().to_string()),
            _ => {
                out.push('[');
                out.push_str(&after[..j]);
                out.push(']');
            }
        }
        rest = &after[j + 1..];
    }
    out.push_str(rest);
    (out.split_whitespace().collect::<Vec<_>>().join(" "), tag, agent)
}

fn split_reason(title: &str) -> (String, Option<String>) {
    let low = title.to_ascii_lowercase();
    for sep in ["— reason:", "– reason:", "- reason:", "(reason:", "reason:"] {
        if let Some(i) = low.find(sep) {
            let reason = title[i + sep.len()..].trim().trim_end_matches(')').trim().to_string();
            let t = title[..i].trim().trim_end_matches(['—', '–', '-', '(']).trim().to_string();
            return (t, (!reason.is_empty()).then_some(reason));
        }
    }
    (title.to_string(), None)
}

fn backticked(s: &str) -> Option<String> {
    let i = s.find('`')?;
    let j = s[i + 1..].find('`')?;
    let c = s[i + 1..i + 1 + j].trim();
    (!c.is_empty()).then(|| c.to_string())
}

fn field<'a>(line: &'a str, names: &[&str]) -> Option<&'a str> {
    let t = line.trim().trim_start_matches(['-', '*']).trim().trim_start_matches("**");
    let low = t.to_ascii_lowercase();
    for n in names {
        if low.starts_with(n) {
            let rest = t[n.len()..].trim_start_matches("**").trim_start_matches(':').trim_start_matches("**").trim();
            return Some(rest);
        }
    }
    None
}

pub fn parse_plan(md: &str) -> Vec<PlanTask> {
    let mut tasks: Vec<PlanTask> = Vec::new();
    let mut in_fence = false;
    let mut stopped = false;
    for line in md.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            // Plans sometimes wrap the whole list in a fence; treat its contents as plain lines.
            continue;
        }
        if let Some((_, raw)) = task_start(line) {
            if stopped {
                continue;
            }
            let (no_tags, tag, agent) = take_tags(raw);
            let (title, reason) = split_reason(&no_tags);
            let title = title.trim_matches('*').trim().to_string();
            if title.is_empty() {
                continue;
            }
            let n = tasks.len() as i64 + 1;
            tasks.push(PlanTask { n, title, tag, reason, agent, ..Default::default() });
            continue;
        }
        let Some(cur) = tasks.last_mut() else { continue };
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with('#') && !in_fence {
            // A new section (e.g. "## Notes") ends the task list.
            stopped = true;
            continue;
        }
        if stopped {
            continue;
        }
        if let Some(f) = field(t, &["files:", "files", "file:"]) {
            cur.files = f
                .split(',')
                .map(|x| x.trim().trim_matches('`').to_string())
                .filter(|x| !x.is_empty() && x != "none" && x != "-")
                .collect();
        } else if let Some(c) = field(t, &["check:", "acceptance check:", "acceptance:", "test:", "verify:"]) {
            // Only a backticked command counts; prose like "tests pass" isn't runnable.
            cur.check = backticked(c);
        } else if let Some(a) = field(t, &["agent:"]) {
            cur.agent = Some(a.trim_matches('`').to_string());
        } else {
            if !cur.details.is_empty() {
                cur.details.push('\n');
            }
            cur.details.push_str(t);
        }
    }
    // Renumber in order (plans sometimes restart numbering per section).
    for (i, t) in tasks.iter_mut().enumerate() {
        t.n = i as i64 + 1;
    }
    tasks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_documented_format() {
        let md = "# Plan\n\nGoal: add tests\n\n\
1. [low] Add a unit test for parse() — reason: single file [agent: test-writer]\n   Files: src/parse.ts, src/parse.test.ts\n   Check: `npm test`\n   Cover the empty-input case.\n\
2. [high] Redesign the config loader — reason: touches 5 modules\n   Files: a, b, c, d, e\n";
        let t = parse_plan(md);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].title, "Add a unit test for parse()");
        assert_eq!(t[0].tag, Some(StepClass::Low));
        assert_eq!(t[0].agent.as_deref(), Some("test-writer"));
        assert_eq!(t[0].reason.as_deref(), Some("single file"));
        assert_eq!(t[0].files, vec!["src/parse.ts", "src/parse.test.ts"]);
        assert_eq!(t[0].check.as_deref(), Some("npm test"));
        assert_eq!(t[0].details, "Cover the empty-input case.");
        assert_eq!(t[1].tag, Some(StepClass::High));
        assert_eq!(t[1].files.len(), 5);
    }

    #[test]
    fn tolerates_markdown_variants() {
        let md = "## Tasks\n### 1. **Set up CI** [low]\n- **2.** Write docs\n3) [trivial] Format code\n## Notes\n4. not a task\n";
        let t = parse_plan(md);
        assert_eq!(t.iter().map(|x| x.title.as_str()).collect::<Vec<_>>(), vec!["Set up CI", "Write docs", "Format code"]);
        assert_eq!(t[0].tag, Some(StepClass::Low));
        assert_eq!(t[2].tag, Some(StepClass::Trivial));
    }

    #[test]
    fn empty_or_prose_plan_yields_nothing() {
        assert!(parse_plan("I think we should refactor things.").is_empty());
    }

    #[test]
    fn check_without_backticks_is_ignored() {
        let t = parse_plan("1. Do it\n   Check: none\n");
        assert_eq!(t[0].check, None);
    }
}
