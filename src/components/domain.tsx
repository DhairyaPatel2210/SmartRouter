// Small domain components shared across screens.
import { Check, CircleDashed, CircleSlash, Clock, Loader2, Pause, RotateCcw, ShieldCheck, SkipForward, X } from "lucide-react";
import type { Fit, RunStatus, StepStatus, Tier } from "../lib/types";
import { Badge } from "./ui";
import { cn } from "../lib/format";

export function TierBadge({ tier, className }: { tier: Tier | null | undefined; className?: string }) {
  if (!tier) return null;
  const map = { local: ["local", "Local"], cheap_cloud: ["cloud", "Cheap cloud"], premium: ["premium", "Premium"] } as const;
  const [tone, label] = map[tier];
  return (
    <Badge tone={tone} className={className}>
      <span className={cn("h-1.5 w-1.5 rounded-full", tier === "local" ? "bg-local" : tier === "premium" ? "bg-premium" : "bg-cloud")} />
      {label}
    </Badge>
  );
}

export function FitBadge({ fit, className }: { fit: Fit | null | undefined; className?: string }) {
  if (!fit) return null;
  const m = { fits: ["ok", "Fits"], tight: ["warn", "Tight"], wont_fit: ["bad", "Won't fit"] } as const;
  const [tone, label] = m[fit];
  return (
    <Badge tone={tone} className={className}>
      {label}
    </Badge>
  );
}

export function ClassBadge({ cls }: { cls: string }) {
  const tone = cls === "high" ? "premium" : cls === "trivial" ? "neutral" : "local";
  return <Badge tone={tone as "premium" | "neutral" | "local"}>{cls}</Badge>;
}

export function StepIcon({ status, className }: { status: StepStatus; className?: string }) {
  const c = cn("h-4 w-4 shrink-0", className);
  switch (status) {
    case "running":
      return <Loader2 className={cn(c, "animate-spin text-info")} />;
    case "verifying":
      return <ShieldCheck className={cn(c, "text-info animate-pulse-soft")} />;
    case "awaiting_approval":
      return <Pause className={cn(c, "text-warn")} />;
    case "passed":
      return <Check className={cn(c, "text-ok")} />;
    case "accepted":
      return <Check className={cn(c, "text-warn")} />;
    case "failed":
      return <X className={cn(c, "text-bad")} />;
    case "skipped":
      return <SkipForward className={cn(c, "text-faint")} />;
    case "cancelled":
      return <CircleSlash className={cn(c, "text-faint")} />;
    case "rolled_back":
      return <RotateCcw className={cn(c, "text-faint")} />;
    default:
      return <CircleDashed className={cn(c, "text-faint")} />;
  }
}

const runTone: Record<RunStatus, "info" | "ok" | "bad" | "neutral" | "warn"> = {
  pending: "neutral",
  planning: "info",
  running: "info",
  paused: "warn",
  reviewing: "info",
  succeeded: "ok",
  failed: "bad",
  cancelled: "neutral",
};

const runLabel: Record<RunStatus, string> = {
  pending: "Starting",
  planning: "Planning",
  running: "Running",
  paused: "Paused",
  reviewing: "Reviewing",
  succeeded: "Succeeded",
  failed: "Failed",
  cancelled: "Cancelled",
};

export function RunStatusBadge({ status, paused }: { status: RunStatus; paused?: boolean }) {
  const s = paused && !["succeeded", "failed", "cancelled"].includes(status) ? "paused" : status;
  const live = ["planning", "running", "reviewing", "pending"].includes(s);
  return (
    <Badge tone={runTone[s]}>
      {live ? <Loader2 className="h-3 w-3 animate-spin" /> : s === "succeeded" ? <Check className="h-3 w-3" /> : s === "paused" ? <Clock className="h-3 w-3" /> : null}
      {runLabel[s]}
    </Badge>
  );
}

export const stepStatusLabel: Record<StepStatus, string> = {
  pending: "Pending",
  running: "Running",
  verifying: "Checking",
  awaiting_approval: "Needs you",
  passed: "Passed",
  failed: "Failed",
  accepted: "Accepted",
  skipped: "Skipped",
  cancelled: "Cancelled",
  rolled_back: "Rolled back",
};

/** A thin stacked bar showing share per tier. */
export function TierBar({ local, cloud, premium, className }: { local: number; cloud: number; premium: number; className?: string }) {
  const total = local + cloud + premium || 1;
  return (
    <div className={cn("flex h-1.5 w-full overflow-hidden rounded-full bg-panel-2", className)}>
      <div className="bg-local" style={{ width: `${(local / total) * 100}%` }} />
      <div className="bg-cloud" style={{ width: `${(cloud / total) * 100}%` }} />
      <div className="bg-premium" style={{ width: `${(premium / total) * 100}%` }} />
    </div>
  );
}
