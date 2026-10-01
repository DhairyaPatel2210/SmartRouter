// Workflow graph model: templates from modes and compilation into a mode.
// Pure (no React Flow import) so it can be unit-tested.
import type { Edge, Node } from "@xyflow/react";
import type { ModeDef, StepClass, Tier } from "../lib/types";

export const KIND_LABEL: Record<NodeKind, string> = {
  plan: "Plan",
  execute: "Execute",
  verify: "Verify",
  review: "Review",
  condition: "Condition",
  approval: "Human approval",
};

export type NodeKind = "plan" | "execute" | "verify" | "review" | "condition" | "approval";

export interface NodeData extends Record<string, unknown> {
  kind: NodeKind;
  label: string;
  agent: string; // "router" or an agent id
  tier: Tier | "any";
  libraryAgent?: string;
  classes?: StepClass[];
  shortPlan?: boolean;
}

export interface Workflow {
  id: string;
  display_name: string;
  builtin?: boolean;
  nodes: Node<NodeData>[];
  edges: Edge[];
}

const tierOf = (e: ModeDef["high"]): Tier => (e === "paid" ? "premium" : "cheap_cloud");

/** The three modes as editable templates. */
export function templateFromMode(m: ModeDef): Workflow {
  const n = (id: string, kind: NodeKind, x: number, y: number, data: Partial<NodeData> = {}): Node<NodeData> => ({
    id,
    type: "step",
    position: { x, y },
    data: { kind, label: KIND_LABEL[kind], agent: "router", tier: "any", ...data },
  });
  const nodes: Node<NodeData>[] = [
    n("plan", "plan", 0, 120, { tier: "premium", shortPlan: m.short_plan, label: m.short_plan ? "Plan (short)" : "Plan" }),
    n("route", "condition", 230, 120, { label: "Step class?" }),
    n("exec-high", "execute", 470, 0, { label: "High steps", tier: tierOf(m.high), classes: ["high"] }),
    n("exec-low", "execute", 470, 120, { label: "Low steps", tier: tierOf(m.low), classes: ["low"] }),
    n("exec-trivial", "execute", 470, 240, { label: "Trivial steps", tier: tierOf(m.trivial), classes: ["trivial"] }),
    n("verify", "verify", 720, 120, { label: "Checks" }),
  ];
  const edges: Edge[] = [
    { id: "e1", source: "plan", target: "route" },
    { id: "e2", source: "route", target: "exec-high", label: "high" },
    { id: "e3", source: "route", target: "exec-low", label: "low" },
    { id: "e4", source: "route", target: "exec-trivial", label: "trivial" },
    { id: "e5", source: "exec-high", target: "verify" },
    { id: "e6", source: "exec-low", target: "verify" },
    { id: "e7", source: "exec-trivial", target: "verify" },
  ];
  if (m.escalation !== "never") {
    if (m.escalation === "approval") {
      nodes.push(n("approve", "approval", 960, 250, { label: "Approve paid escalation" }));
      nodes.push(n("escalate", "execute", 1190, 250, { label: "Escalate to paid", tier: "premium", classes: [] }));
      edges.push({ id: "e8", source: "verify", target: "approve", label: "fail", data: { branch: "fail" } });
      edges.push({ id: "e9", source: "approve", target: "escalate" });
    } else {
      nodes.push(n("escalate", "execute", 960, 250, { label: "Escalate (cloud → paid)", tier: "premium", classes: [] }));
      edges.push({ id: "e8", source: "verify", target: "escalate", label: "fail", data: { branch: "fail" } });
    }
  }
  if (m.review) {
    nodes.push(n("review", "review", 960, 60, { label: "Final review", tier: "premium" }));
    edges.push({ id: "e10", source: "verify", target: "review", label: "pass", data: { branch: "pass" } });
  }
  return { id: `template-${m.id}`, display_name: m.display_name, builtin: true, nodes, edges };
}

/** Compiles a workflow graph into a routing mode. */
export function compileWorkflow(w: Workflow): ModeDef {
  const execs = w.nodes.filter((x) => x.data.kind === "execute");
  const execFor = (c: StepClass): ModeDef["high"] => {
    const node = execs.find((x) => x.data.classes?.includes(c));
    return node && node.data.tier === "premium" ? "paid" : "cheap";
  };
  const failTargets = w.edges.filter((e) => (e.data as { branch?: string } | undefined)?.branch === "fail" || e.label === "fail").map((e) => w.nodes.find((x) => x.id === e.target));
  const escalation: ModeDef["escalation"] = failTargets.some((t) => t?.data.kind === "approval") ? "approval" : failTargets.some((t) => t?.data.kind === "execute") ? "auto" : "never";
  const plan = w.nodes.find((x) => x.data.kind === "plan");
  return {
    id: `wf-${w.id}`,
    display_name: w.display_name,
    builtin: false,
    description: `From the "${w.display_name}" workflow.`,
    high: execFor("high"),
    low: execFor("low"),
    trivial: execFor("trivial"),
    unclear_as: execFor("high") === "paid" ? "high" : "low",
    review: w.nodes.some((x) => x.data.kind === "review"),
    escalation,
    attempts_per_executor: 2,
    short_plan: !!plan?.data.shortPlan,
  };
}

