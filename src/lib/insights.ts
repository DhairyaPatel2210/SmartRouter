import type { OutcomeStat } from "./types";

/** Learned routing suggestions from past outcomes (M5: suggests, never auto-applies). */
export function buildSuggestions(stats: OutcomeStat[]): string[] {
  const out: string[] = [];
  for (const s of stats) {
    if (s.total < 3) continue;
    const r = s.first_try_passes / s.total;
    if (s.tier !== "premium" && r < 0.4) {
      out.push(`${cap(s.class)} steps on ${s.model} pass first try only ${Math.round(r * 100)}% of the time (${s.total} steps). Consider a stronger model or routing ${s.class} steps to cloud/paid.`);
    }
    if (s.tier === "premium" && s.class === "low" && r > 0.9) {
      out.push(`Low steps on the paid agent pass ${Math.round(r * 100)}% of the time; Balanced or Cost mode could save money here.`);
    }
    if (s.tier === "local" && r >= 0.8) {
      out.push(`${s.model} handles ${s.class} steps well (${Math.round(r * 100)}% first try). Keep it first in the pool.`);
    }
  }
  return out.slice(0, 6);
}

function cap(s: string) {
  return s.charAt(0).toUpperCase() + s.slice(1);
}
