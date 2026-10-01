import { useEffect } from "react";
import { Battery, Cpu, Flame, MemoryStick, OctagonX, Zap } from "lucide-react";
import { useApp } from "../lib/store";
import { api } from "../lib/api";
import { cn, gb, mb, usd } from "../lib/format";

/** Always-visible resource strip: what's running, memory, CPU, cost. Click → Telemetry. */
export function StatusBar() {
  const snap = useApp((s) => s.snapshot);
  const go = useApp((s) => s.go);
  const toast = useApp((s) => s.toast);
  const activeRuns = useApp((s) => Object.values(s.runs).filter((r) => ["pending", "planning", "running", "reviewing"].includes(r.status)).length);

  // Idle: no timers. Refresh once when the window regains focus.
  useEffect(() => {
    const f = () => api.resourceSnapshot().then((snapshot) => useApp.setState({ snapshot }), () => {});
    window.addEventListener("focus", f);
    return () => window.removeEventListener("focus", f);
  }, []);

  const running = snap?.running ?? [];
  const cpu = (snap?.procs ?? []).reduce((a, p) => a + p.cpu_pct, 0);
  const used = (snap?.controlled_mb ?? 0) / 1024;
  const total = snap?.sys.total_gb ?? 0;
  const free = snap?.sys.free_gb ?? 0;
  const usedPct = total ? (used / total) * 100 : 0;
  const otherPct = total ? Math.max(0, ((total - free - used) / total) * 100) : 0;
  const pressure = snap?.sys.pressure;
  const hot = snap?.sys.thermal === "serious" || snap?.sys.thermal === "critical";
  const battery = snap?.sys.power === "battery";
  const busy = running.length > 0 || activeRuns > 0;

  return (
    <div className="h-7 shrink-0 border-t border-line bg-panel flex items-center gap-4 px-3 text-[11.5px] text-muted">
      <button onClick={() => go("telemetry")} className="flex items-center gap-4 min-w-0 flex-1 h-full hover:text-fg text-left" title="Open the Resource view">
        <span className="flex items-center gap-1.5 min-w-0">
          <span className={cn("h-2 w-2 rounded-full shrink-0", busy ? "bg-info animate-pulse-soft" : "bg-ok")} />
          <span className="truncate">{running.length ? running.join(" · ") : activeRuns ? "Working…" : "Idle"}</span>
        </span>
        <span className="flex items-center gap-1.5 shrink-0" title={`Agents + models: ${gb(used)} · other apps: ${gb(total - free - used)} · free: ${gb(free)}`}>
          <MemoryStick className="h-3.5 w-3.5" />
          <span className="relative w-20 h-1.5 rounded-full bg-panel-2 overflow-hidden">
            <span className="absolute inset-y-0 left-0 bg-accent" style={{ width: `${usedPct}%` }} />
            <span className="absolute inset-y-0 bg-line-strong" style={{ left: `${usedPct}%`, width: `${otherPct}%` }} />
          </span>
          <span>{used > 0.05 ? `${gb(used)} used · ` : ""}{gb(free)} free</span>
        </span>
        {cpu > 0.5 && (
          <span className="flex items-center gap-1 shrink-0">
            <Cpu className="h-3.5 w-3.5" />
            {cpu.toFixed(0)}%
          </span>
        )}
        {snap && snap.cost_usd > 0 && <span className="shrink-0">~{usd(snap.cost_usd)} this run</span>}
        {snap?.app && <span className="shrink-0 text-faint" title="This app's own footprint">App {mb(snap.app.rss_mb)}</span>}
      </button>
      <span className="flex items-center gap-2 shrink-0">
        {pressure && pressure !== "normal" && (
          <span className={cn("flex items-center gap-1", pressure === "critical" ? "text-bad" : "text-warn")} title="macOS memory pressure">
            <Zap className="h-3.5 w-3.5" />
            {pressure === "critical" ? "Memory critical" : "Memory tight"}
          </span>
        )}
        {hot && (
          <span className="flex items-center gap-1 text-warn" title="Thermal state">
            <Flame className="h-3.5 w-3.5" />
            Hot
          </span>
        )}
        {battery && (
          <span className="flex items-center gap-1" title={snap?.sys.low_power ? "On battery · Low Power Mode" : "On battery"}>
            <Battery className="h-3.5 w-3.5" />
            {snap?.sys.low_power ? "Low power" : "Battery"}
          </span>
        )}
        {busy && (
          <button
            onClick={async () => toast("warn", await api.stopEverything())}
            className="flex items-center gap-1 h-5 px-1.5 rounded text-bad hover:bg-bad-soft"
            title="Cancel all runs, kill every agent process and unload models the app loaded"
          >
            <OctagonX className="h-3.5 w-3.5" />
            Stop everything
          </button>
        )}
      </span>
    </div>
  );
}
