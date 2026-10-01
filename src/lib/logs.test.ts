import { logStore } from "./logs";
import type { LogLine } from "./types";

const line = (seq: number): LogLine => ({ seq, ts: 0, kind: "out", text: `line ${seq}` });

describe("logStore", () => {
  it("appends, ignores duplicates and stays bounded", () => {
    const step = "s-bounded";
    logStore.append(Array.from({ length: 3000 }, (_, i) => [step, line(i + 1)] as [string, LogLine]));
    expect(logStore.get(step).length).toBeLessThanOrEqual(2500);
    expect(logStore.lastSeq(step)).toBe(3000);
    logStore.append([[step, line(10)]]);
    expect(logStore.lastSeq(step)).toBe(3000);
  });

  it("backfill keeps newer streamed lines", () => {
    const step = "s-backfill";
    logStore.append([[step, line(5)], [step, line(6)]]);
    logStore.backfill(step, [line(1), line(2), line(3), line(4), line(5)]);
    expect(logStore.get(step).map((l) => l.seq)).toEqual([1, 2, 3, 4, 5, 6]);
  });

  it("notifies only the subscribed step", () => {
    let a = 0;
    let b = 0;
    const ua = logStore.subscribe("A", () => a++);
    const ub = logStore.subscribe("B", () => b++);
    logStore.append([["A", line(1)], ["A", line(2)]]);
    expect(a).toBe(1);
    expect(b).toBe(0);
    ua();
    ub();
  });
});
