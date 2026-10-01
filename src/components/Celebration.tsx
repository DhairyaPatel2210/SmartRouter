import { useEffect } from "react";
import { PartyPopper } from "lucide-react";
import { useApp } from "../lib/store";

/** A small, one-time celebration for the first passing run (respects Reduce motion via CSS). */
export function Celebration() {
  const on = useApp((s) => s.celebrate);
  useEffect(() => {
    if (!on) return;
    const t = setTimeout(() => useApp.setState({ celebrate: false }), 3200);
    return () => clearTimeout(t);
  }, [on]);
  if (!on) return null;
  const dots = Array.from({ length: 28 }, (_, i) => i);
  return (
    <div className="fixed inset-0 z-50 pointer-events-none flex items-center justify-center">
      <div className="relative animate-pop rounded-2xl bg-elev border border-line shadow-2xl px-6 py-4 flex items-center gap-3">
        <PartyPopper className="h-6 w-6 text-accent" />
        <div>
          <div className="font-semibold">First run passed!</div>
          <div className="text-xs text-muted">The planner handed off, the executor delivered, and checks are green.</div>
        </div>
        {dots.map((i) => (
          <span
            key={i}
            className="absolute h-1.5 w-1.5 rounded-full"
            style={{
              left: "50%",
              top: "50%",
              background: ["#8b6cff", "#2dd4bf", "#60a5fa", "#fbbf24"][i % 4],
              transform: `rotate(${(i / dots.length) * 360}deg) translateY(-${70 + (i % 5) * 14}px)`,
              opacity: 0.9,
              transition: "transform 1s ease-out",
            }}
          />
        ))}
      </div>
    </div>
  );
}
