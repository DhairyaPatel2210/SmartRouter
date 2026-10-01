export function cn(...xs: (string | false | null | undefined)[]): string {
  return xs.filter(Boolean).join(" ");
}

export function usd(x: number | null | undefined, digits?: number): string {
  const v = x ?? 0;
  if (v === 0) return "$0";
  const d = digits ?? (v < 0.01 ? 4 : v < 1 ? 3 : 2);
  return `$${v.toFixed(d)}`;
}

export function gb(x: number | null | undefined): string {
  const v = x ?? 0;
  return v >= 10 ? `${v.toFixed(0)} GB` : `${v.toFixed(1)} GB`;
}

export function mb(x: number | null | undefined): string {
  const v = x ?? 0;
  if (v >= 1024) return `${(v / 1024).toFixed(1)} GB`;
  return `${v.toFixed(0)} MB`;
}

export function bytes(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(2)} GB`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)} MB`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(0)} KB`;
  return `${n} B`;
}

export function tokens(n: number): string {
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)}M`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(1)}k`;
  return `${n}`;
}

export function duration(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}

export function ago(ts: number): string {
  const s = Math.round((Date.now() - ts) / 1000);
  if (s < 45) return "just now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} h ago`;
  const d = Math.round(h / 24);
  if (d < 7) return `${d} d ago`;
  return new Date(ts).toLocaleDateString();
}

export function shortPath(p: string, home?: string): string {
  if (home && p.startsWith(home)) return "~" + p.slice(home.length);
  const m = p.match(/^\/Users\/[^/]+(.*)$/);
  return m ? "~" + m[1] : p;
}

export function pct(a: number, b: number): number {
  return b > 0 ? Math.min(100, Math.max(0, (a / b) * 100)) : 0;
}
