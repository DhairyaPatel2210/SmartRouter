// Per-step log lines kept outside React state: appends are O(1) and only
// the step being viewed re-renders. Bounded like the core's ring buffer.
import { useSyncExternalStore } from "react";
import type { LogLine } from "./types";

const CAP = 2000;

class LogStore {
  private lines = new Map<string, LogLine[]>();
  private versions = new Map<string, number>();
  private listeners = new Map<string, Set<() => void>>();

  append(batch: [string, LogLine][]) {
    const touched = new Set<string>();
    for (const [step, line] of batch) {
      let arr = this.lines.get(step);
      if (!arr) {
        arr = [];
        this.lines.set(step, arr);
      }
      const last = arr[arr.length - 1];
      if (last && line.seq <= last.seq) continue; // already have it (backfill race)
      arr.push(line);
      touched.add(step);
    }
    for (const step of touched) {
      const arr = this.lines.get(step)!;
      if (arr.length > CAP * 1.25) arr.splice(0, arr.length - CAP);
      this.bump(step);
    }
  }

  /** Replaces a step's lines with a backfill from the core (keeps newer streamed lines). */
  backfill(step: string, lines: LogLine[]) {
    const cur = this.lines.get(step) ?? [];
    const maxSeq = lines.length ? lines[lines.length - 1].seq : 0;
    const merged = [...lines, ...cur.filter((l) => l.seq > maxSeq)];
    this.lines.set(step, merged.slice(-CAP));
    this.bump(step);
  }

  has(step: string) {
    return this.lines.has(step);
  }

  get(step: string): LogLine[] {
    return this.lines.get(step) ?? EMPTY;
  }

  lastSeq(step: string): number {
    const a = this.lines.get(step);
    return a && a.length ? a[a.length - 1].seq : 0;
  }

  version(step: string) {
    return this.versions.get(step) ?? 0;
  }

  subscribe(step: string, fn: () => void) {
    let set = this.listeners.get(step);
    if (!set) {
      set = new Set();
      this.listeners.set(step, set);
    }
    set.add(fn);
    return () => set!.delete(fn);
  }

  private bump(step: string) {
    this.versions.set(step, this.version(step) + 1);
    this.listeners.get(step)?.forEach((f) => f());
  }
}

const EMPTY: LogLine[] = [];

export const logStore = new LogStore();

/** Re-renders when the step's log changes; returns [lines, version]. */
export function useStepLogs(step: string | null): [LogLine[], number] {
  const version = useSyncExternalStore(
    (cb) => (step ? logStore.subscribe(step, cb) : () => {}),
    () => (step ? logStore.version(step) : 0),
  );
  return [step ? logStore.get(step) : EMPTY, version];
}
