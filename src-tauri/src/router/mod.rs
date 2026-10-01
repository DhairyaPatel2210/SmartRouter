//! The router: rule-based step classifier, mode tables, Library tier hints,
//! executor-pool fallthrough and the escalation ladder
//! (local → cheap cloud → paid). Pure functions over a precomputed
//! [`RouteEnv`], so every decision is testable and carries its reason.

use crate::handoff::PlanTask;
use crate::types::*;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Classified {
    pub class: StepClass,
    pub reason: String,
    /// A hard rule decided (overrides the planner's tag).
    pub hard_rule: bool,
}

// Needles ending in `*` are stems ("concurren*" matches "concurrency");
// the rest must be whole words (an "s" plural is allowed), so "auth"
// doesn't match "author".
const HIGH_HARD: &[(&str, &str)] = &[
    ("security", "security code"),
    ("auth", "auth code"),
    ("authenticat*", "auth code"),
    ("authoriz*", "auth code"),
    ("oauth", "auth code"),
    ("login", "auth code"),
    ("password", "credentials"),
    ("credential", "credentials"),
    ("encrypt*", "cryptography"),
    ("permission", "permissions"),
    ("concurren*", "concurrency"),
    ("race", "concurrency"),
    ("deadlock", "concurrency"),
    ("thread", "concurrency"),
    ("mutex", "concurrency"),
    ("migration", "data migration"),
    ("migrate", "data migration"),
    ("schema change", "data migration"),
];
const HIGH_SOFT: &[&str] = &[
    "architect*", "design", "redesign", "debug*", "investigat*", "root cause", "diagnos*", "refactor*", "performance", "optimi*",
    "rewrite", "restructur*", "integrat*",
];
const TRIVIAL: &[&str] = &["format", "formatting", "prettier", "commit message", "changelog", "typo", "lint fix", "whitespace"];
const LOW_SOFT: &[&str] = &[
    "boilerplate", "test*", "rename", "docs", "documentation", "readme", "comment*", "config*", "scaffold*", "stub", "example",
    "docstring", "typing", "type hints", "bump", "dependency", "dependencies", "log message", "copy",
];

fn has_word(hay: &str, needle: &str) -> bool {
    let (n, stem) = match needle.strip_suffix('*') {
        Some(n) => (n, true),
        None => (needle, false),
    };
    let b = hay.as_bytes();
    let alnum = |i: usize| b.get(i).is_some_and(|c| c.is_ascii_alphanumeric());
    let mut start = 0;
    while let Some(i) = hay[start..].find(n) {
        let at = start + i;
        let end = at + n.len();
        let before_ok = at == 0 || !alnum(at - 1);
        let after_ok = stem || !alnum(end) || (b[end] == b's' && !alnum(end + 1));
        if before_ok && after_ok {
            return true;
        }
        start = at + n.len();
    }
    false
}

/// Classifies a plan task. Hard rules (security, concurrency, migrations,
/// more than 3 files, formatting chores) override the planner's tag.
pub fn classify(task: &PlanTask, mode: &ModeDef) -> Classified {
    let text = format!("{} {}", task.title, task.details).to_lowercase();
    if let Some((_, why)) = HIGH_HARD.iter().find(|(k, _)| has_word(&text, k)) {
        return Classified { class: StepClass::High, reason: format!("hard rule: {why}"), hard_rule: true };
    }
    if task.files.len() > 3 {
        return Classified { class: StepClass::High, reason: format!("hard rule: touches {} files", task.files.len()), hard_rule: true };
    }
    let title = task.title.to_lowercase();
    if TRIVIAL.iter().any(|k| has_word(&title, k)) {
        return Classified { class: StepClass::Trivial, reason: "hard rule: formatting/commit chore".into(), hard_rule: true };
    }
    if let Some(tag) = task.tag {
        let why = task.reason.clone().map(|r| format!("planner tagged [{tag}]: {r}")).unwrap_or_else(|| format!("planner tagged [{tag}]"));
        return Classified { class: tag, reason: why, hard_rule: false };
    }
    if let Some(k) = HIGH_SOFT.iter().find(|k| has_word(&text, k)) {
        return Classified { class: StepClass::High, reason: format!("looks like {} work", k.trim_end_matches('*')), hard_rule: false };
    }
    if let Some(k) = LOW_SOFT.iter().find(|k| has_word(&text, k)) {
        return Classified { class: StepClass::Low, reason: format!("routine {} work", k.trim_end_matches('*')), hard_rule: false };
    }
    Classified {
        class: mode.unclear_as,
        reason: format!("unclear; {} treats it as {}", mode.display_name, mode.unclear_as),
        hard_rule: false,
    }
}

/// Availability of one executor-pool entry right now.
#[derive(Debug, Clone)]
pub struct PoolStatus {
    pub entry: ExecutorEntry,
    /// `None` = usable; `Some(reason)` = skipped.
    pub blocked: Option<String>,
}

impl PoolStatus {
    pub fn label(&self) -> String {
        format!("{} + {}", self.entry.agent_id, self.entry.model.label())
    }
}

pub struct RouteEnv {
    pub mode: ModeDef,
    /// Paid agent for planning/review/high steps (installed + enabled).
    pub paid_agent: Option<String>,
    /// Budget planning stand-in for the paid agent (already validated).
    pub budget_planner: Option<ExecutorEntry>,
    pub pool: Vec<PoolStatus>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RouteDecision {
    pub exec: Exec,
    pub agent_id: String,
    pub model: Option<ModelRef>,
    pub tier: Tier,
    pub reason: String,
    /// Pool entries skipped on the way, with why.
    pub fallthrough: Vec<String>,
    /// Escalating to this executor needs the user's OK (Cost mode → paid).
    pub needs_approval: bool,
}

impl RouteEnv {
    fn paid(&self, why: String) -> Result<RouteDecision, String> {
        if let Some(bp) = &self.budget_planner {
            return Ok(RouteDecision {
                exec: Exec::Paid,
                agent_id: bp.agent_id.clone(),
                model: Some(bp.model.clone()),
                tier: bp.model.tier,
                reason: format!("{why}; budget planning: {} stands in for the paid agent", bp.model.label()),
                fallthrough: vec![],
                needs_approval: false,
            });
        }
        match &self.paid_agent {
            Some(a) => Ok(RouteDecision {
                exec: Exec::Paid,
                agent_id: a.clone(),
                model: None,
                tier: Tier::Premium,
                reason: why,
                fallthrough: vec![],
                needs_approval: false,
            }),
            None => Err("No paid agent is available. Install one (Cursor, Claude Code, Codex, Copilot) or turn on budget planning.".into()),
        }
    }

    /// First usable pool entry, optionally restricted to a tier.
    fn cheap(&self, only: Option<Tier>, prefer_local: bool) -> (Option<RouteDecision>, Vec<String>) {
        let mut skipped = vec![];
        let mut order: Vec<&PoolStatus> = self.pool.iter().filter(|p| p.entry.enabled).collect();
        if prefer_local {
            order.sort_by_key(|p| p.entry.model.tier != Tier::Local);
        }
        for p in order {
            if let Some(t) = only {
                if p.entry.model.tier != t {
                    continue;
                }
            }
            match &p.blocked {
                Some(why) => skipped.push(format!("{}: {why}", p.label())),
                None => {
                    return (
                        Some(RouteDecision {
                            exec: Exec::Cheap,
                            agent_id: p.entry.agent_id.clone(),
                            model: Some(p.entry.model.clone()),
                            tier: p.entry.model.tier,
                            reason: String::new(),
                            fallthrough: skipped.clone(),
                            needs_approval: false,
                        }),
                        skipped,
                    )
                }
            }
        }
        (None, skipped)
    }

    pub fn has_cheap(&self) -> bool {
        self.pool.iter().any(|p| p.entry.enabled && p.blocked.is_none())
    }
}

/// Routes a step. `hint` is the Library agent's tier (`premium|cheap|local|any`).
pub fn route_step(c: &Classified, hint: Option<&str>, env: &RouteEnv) -> Result<RouteDecision, String> {
    let mode = &env.mode;
    let mut exec = mode.exec_for(c.class);
    let mut why = format!("{}: {} step → {}", mode.display_name, c.class, if exec == Exec::Paid { "paid" } else { "cheap" });
    let all_cheap = mode.high == Exec::Cheap && mode.low == Exec::Cheap;
    let mut prefer_local = false;
    match hint {
        Some("premium") if exec == Exec::Cheap => {
            if all_cheap {
                why.push_str(" (agent prefers premium, but this mode keeps execution cheap)");
            } else {
                exec = Exec::Paid;
                why = format!("{}: Library agent prefers premium", mode.display_name);
            }
        }
        Some(h @ ("cheap" | "local")) if exec == Exec::Paid => {
            if c.hard_rule && c.class == StepClass::High {
                why.push_str(&format!(" (agent prefers {h}, overridden by a hard rule)"));
            } else {
                exec = Exec::Cheap;
                prefer_local = h == "local";
                why = format!("{}: Library agent prefers {h}", mode.display_name);
            }
        }
        Some("local") => prefer_local = true,
        _ => {}
    }
    match exec {
        Exec::Paid => env.paid(why),
        Exec::Cheap => {
            let (d, skipped) = env.cheap(None, prefer_local);
            match d {
                Some(mut d) => {
                    d.reason = if skipped.is_empty() { why } else { format!("{why}; skipped {}", skipped.join("; ")) };
                    Ok(d)
                }
                None => {
                    let detail = if skipped.is_empty() { "the executor pool is empty".to_string() } else { skipped.join("; ") };
                    let mut d = env.paid(format!("{why}, but no cheap executor is available ({detail})"))?;
                    d.fallthrough = skipped;
                    d.needs_approval = mode.escalation == Escalation::Approval && d.tier == Tier::Premium;
                    Ok(d)
                }
            }
        }
    }
}

/// Next rung of the ladder after failures on `current`: local → cheap cloud → paid.
pub fn escalate(current: &RouteDecision, env: &RouteEnv) -> Option<RouteDecision> {
    let mode = &env.mode;
    if mode.escalation == Escalation::Never {
        return None;
    }
    let to_paid = |why: &str| -> Option<RouteDecision> {
        let mut d = env.paid(why.to_string()).ok()?;
        // Already on the stand-in (budget planning) or the paid agent: nowhere to go.
        if d.agent_id == current.agent_id && d.model == current.model {
            return None;
        }
        d.needs_approval = mode.escalation == Escalation::Approval && d.tier == Tier::Premium;
        Some(d)
    };
    match current.tier {
        Tier::Local => {
            let (d, _) = env.cheap(Some(Tier::CheapCloud), false);
            match d {
                Some(mut d) => {
                    d.reason = "escalated: local attempts failed, trying cheap cloud".into();
                    Some(d)
                }
                None => to_paid("escalated: local attempts failed and no cheap cloud model is available"),
            }
        }
        Tier::CheapCloud => to_paid("escalated: cheap attempts failed"),
        Tier::Premium => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(id: &str) -> ModeDef {
        ModeDef::builtins().into_iter().find(|m| m.id == id).unwrap()
    }

    fn task(title: &str) -> PlanTask {
        PlanTask { n: 1, title: title.into(), ..Default::default() }
    }

    fn model(tier: Tier, name: &str) -> ModelRef {
        ModelRef {
            provider_id: if tier == Tier::Local { "ollama".into() } else { "openrouter".into() },
            provider_type: if tier == Tier::Local { ProviderType::Ollama } else { ProviderType::Openrouter },
            name: name.into(),
            display_name: None,
            base_url: None,
            key_ref: None,
            tier,
            mem_needed_gb: None,
            ctx_len: None,
            price_in_per_m: None,
            price_out_per_m: None,
        }
    }

    fn env(m: &str, local_blocked: Option<&str>) -> RouteEnv {
        RouteEnv {
            mode: mode(m),
            paid_agent: Some("cursor".into()),
            budget_planner: None,
            pool: vec![
                PoolStatus {
                    entry: ExecutorEntry { agent_id: "opencode".into(), model: model(Tier::Local, "qwen"), enabled: true },
                    blocked: local_blocked.map(String::from),
                },
                PoolStatus {
                    entry: ExecutorEntry { agent_id: "opencode".into(), model: model(Tier::CheapCloud, "deepseek"), enabled: true },
                    blocked: None,
                },
            ],
        }
    }

    #[test]
    fn classifier_rules() {
        let m = mode("balanced");
        assert_eq!(classify(&task("Fix the auth token refresh race"), &m).class, StepClass::High);
        assert!(classify(&task("Fix the auth token refresh race"), &m).hard_rule);
        assert_eq!(classify(&task("Update author list in README"), &m).class, StepClass::Low, "author ≠ auth");
        assert_eq!(classify(&task("Format the code"), &m).class, StepClass::Trivial);
        assert_eq!(classify(&task("Write tests for the parser"), &m).class, StepClass::Low);
        assert_eq!(classify(&task("Design the plugin architecture"), &m).class, StepClass::High);
        let mut many = task("Add a feature");
        many.files = vec!["a".into(), "b".into(), "c".into(), "d".into()];
        assert_eq!(classify(&many, &m).class, StepClass::High);
        let mut tagged = task("Wire it up");
        tagged.tag = Some(StepClass::Low);
        assert_eq!(classify(&tagged, &m).class, StepClass::Low);
        let mut tagged_sec = task("Add password hashing");
        tagged_sec.tag = Some(StepClass::Low);
        assert_eq!(classify(&tagged_sec, &m).class, StepClass::High, "hard rule beats the tag");
        // Unclear: high in Balanced/Intelligent, low in Cost.
        assert_eq!(classify(&task("Wire it up"), &m).class, StepClass::High);
        assert_eq!(classify(&task("Wire it up"), &mode("cost")).class, StepClass::Low);
    }

    #[test]
    fn balanced_routes_low_cheap_high_paid() {
        let e = env("balanced", None);
        let low = route_step(&classify(&task("Write tests for x"), &e.mode), None, &e).unwrap();
        assert_eq!((low.exec, low.tier), (Exec::Cheap, Tier::Local));
        let high = route_step(&classify(&task("Design the cache"), &e.mode), None, &e).unwrap();
        assert_eq!((high.exec, high.agent_id.as_str()), (Exec::Paid, "cursor"));
    }

    #[test]
    fn pool_falls_through_with_reason() {
        let e = env("balanced", Some("on battery"));
        let d = route_step(&classify(&task("Write docs"), &e.mode), None, &e).unwrap();
        assert_eq!(d.tier, Tier::CheapCloud);
        assert!(d.reason.contains("on battery"), "{}", d.reason);
    }

    #[test]
    fn intelligent_only_trivial_is_cheap() {
        let e = env("intelligent", None);
        assert_eq!(route_step(&classify(&task("Write docs"), &e.mode), None, &e).unwrap().exec, Exec::Paid);
        assert_eq!(route_step(&classify(&task("Format the code"), &e.mode), None, &e).unwrap().exec, Exec::Cheap);
    }

    #[test]
    fn tier_hints() {
        let e = env("balanced", None);
        let c = classify(&task("Write docs"), &e.mode);
        assert_eq!(route_step(&c, Some("premium"), &e).unwrap().exec, Exec::Paid);
        let cost = env("cost", None);
        let c2 = classify(&task("Write docs"), &cost.mode);
        let d = route_step(&c2, Some("premium"), &cost).unwrap();
        assert_eq!(d.exec, Exec::Cheap, "Cost mode keeps execution cheap");
        assert!(d.reason.contains("keeps execution cheap"));
        let hard = classify(&task("Add password reset"), &e.mode);
        assert_eq!(route_step(&hard, Some("cheap"), &e).unwrap().exec, Exec::Paid, "hard rule beats a cheap hint");
    }

    #[test]
    fn escalation_ladder() {
        let e = env("balanced", None);
        let local = route_step(&classify(&task("Write docs"), &e.mode), None, &e).unwrap();
        let cloud = escalate(&local, &e).unwrap();
        assert_eq!(cloud.tier, Tier::CheapCloud);
        let paid = escalate(&cloud, &e).unwrap();
        assert_eq!(paid.tier, Tier::Premium);
        assert!(!paid.needs_approval);
        assert!(escalate(&paid, &e).is_none());
        // Cost mode: paid escalation needs approval.
        let c = env("cost", None);
        let p = escalate(&cloud, &c).unwrap();
        assert!(p.needs_approval);
    }

    #[test]
    fn budget_planning_stands_in_for_paid() {
        let mut e = env("balanced", None);
        e.paid_agent = None;
        let bp = ExecutorEntry { agent_id: "opencode".into(), model: model(Tier::CheapCloud, "strong-cheap"), enabled: true };
        e.budget_planner = Some(bp);
        let d = route_step(&classify(&task("Design the cache"), &e.mode), None, &e).unwrap();
        assert_eq!(d.model.as_ref().unwrap().name, "strong-cheap");
        assert!(d.reason.contains("budget planning"));
        // Escalating from the stand-in goes nowhere.
        assert!(escalate(&d, &e).is_none());
    }

    #[test]
    fn no_paid_agent_and_no_pool_is_an_error() {
        let e = RouteEnv { mode: mode("balanced"), paid_agent: None, budget_planner: None, pool: vec![] };
        assert!(route_step(&classify(&task("Design x"), &e.mode), None, &e).is_err());
    }
}
