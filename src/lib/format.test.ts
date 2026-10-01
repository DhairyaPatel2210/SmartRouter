import { ago, bytes, cn, duration, gb, pct, shortPath, tokens, usd } from "./format";

describe("format", () => {
  it("formats money as estimates", () => {
    expect(usd(0)).toBe("$0");
    expect(usd(0.004)).toBe("$0.0040");
    expect(usd(0.25)).toBe("$0.250");
    expect(usd(12.5)).toBe("$12.50");
  });
  it("formats sizes and tokens", () => {
    expect(gb(9.54)).toBe("9.5 GB");
    expect(gb(11.2)).toBe("11 GB");
    expect(bytes(4_700_000_000)).toBe("4.70 GB");
    expect(tokens(1500)).toBe("1.5k");
    expect(tokens(2_300_000)).toBe("2.3M");
  });
  it("formats durations and relative time", () => {
    expect(duration(42_000)).toBe("42s");
    expect(duration(125_000)).toBe("2m 5s");
    expect(ago(Date.now())).toBe("just now");
  });
  it("misc helpers", () => {
    expect(cn("a", false, null, "b")).toBe("a b");
    expect(pct(5, 10)).toBe(50);
    expect(pct(5, 0)).toBe(0);
    expect(shortPath("/Users/jane/code/app")).toBe("~/code/app");
  });
});
