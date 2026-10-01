// Small, dependency-free UI kit in the shadcn style (Tailwind classes, no runtime).
import React, { useEffect, useRef, useState } from "react";
import { cn } from "../../lib/format";

type BtnVariant = "primary" | "secondary" | "ghost" | "danger" | "outline";
type BtnSize = "sm" | "md" | "lg" | "icon";

export const Button = React.forwardRef<
  HTMLButtonElement,
  React.ButtonHTMLAttributes<HTMLButtonElement> & { variant?: BtnVariant; size?: BtnSize; loading?: boolean }
>(function Button({ variant = "secondary", size = "md", loading, className, children, disabled, ...p }, ref) {
  const v: Record<BtnVariant, string> = {
    primary: "bg-accent text-accent-fg hover:brightness-110 shadow-sm shadow-accent/20",
    secondary: "bg-panel-2 text-fg hover:bg-line border border-line",
    outline: "border border-line-strong text-fg hover:bg-panel-2",
    ghost: "text-muted hover:text-fg hover:bg-panel-2",
    danger: "bg-bad text-white hover:brightness-110",
  };
  const s: Record<BtnSize, string> = {
    sm: "h-7 px-2.5 text-xs gap-1.5 rounded-md",
    md: "h-8 px-3 text-[13px] gap-2 rounded-lg",
    lg: "h-10 px-4 text-sm gap-2 rounded-lg font-medium",
    icon: "h-7 w-7 rounded-md justify-center",
  };
  return (
    <button
      ref={ref}
      disabled={disabled || loading}
      className={cn(
        "inline-flex items-center whitespace-nowrap font-medium transition-[background,filter,color] duration-100 disabled:opacity-50 disabled:pointer-events-none cursor-default",
        v[variant],
        s[size],
        className,
      )}
      {...p}
    >
      {loading && <Spinner className="h-3.5 w-3.5" />}
      {children}
    </button>
  );
});

export function Spinner({ className }: { className?: string }) {
  return (
    <svg className={cn("animate-spin h-4 w-4", className)} viewBox="0 0 24 24" fill="none" aria-hidden>
      <circle cx="12" cy="12" r="9" stroke="currentColor" strokeOpacity="0.25" strokeWidth="3" />
      <path d="M21 12a9 9 0 0 0-9-9" stroke="currentColor" strokeWidth="3" strokeLinecap="round" />
    </svg>
  );
}

type Tone = "neutral" | "accent" | "ok" | "warn" | "bad" | "info" | "local" | "cloud" | "premium";

export function Badge({ tone = "neutral", className, children, title }: { tone?: Tone; className?: string; children: React.ReactNode; title?: string }) {
  const t: Record<Tone, string> = {
    neutral: "bg-panel-2 text-muted border-line",
    accent: "bg-accent-soft text-accent border-transparent",
    ok: "bg-ok-soft text-ok border-transparent",
    warn: "bg-warn-soft text-warn border-transparent",
    bad: "bg-bad-soft text-bad border-transparent",
    info: "bg-info-soft text-info border-transparent",
    local: "bg-local-soft text-local border-transparent",
    cloud: "bg-cloud-soft text-cloud border-transparent",
    premium: "bg-premium-soft text-premium border-transparent",
  };
  return (
    <span title={title} className={cn("inline-flex items-center gap-1 h-5 px-1.5 rounded-md border text-[11px] font-medium whitespace-nowrap", t[tone], className)}>
      {children}
    </span>
  );
}

export function Card({ className, children, ...p }: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div className={cn("bg-panel border border-line rounded-xl", className)} {...p}>
      {children}
    </div>
  );
}

export function SectionTitle({ children, right, className }: { children: React.ReactNode; right?: React.ReactNode; className?: string }) {
  return (
    <div className={cn("flex items-center justify-between mb-2", className)}>
      <h3 className="text-[11px] font-semibold uppercase tracking-wider text-faint">{children}</h3>
      {right}
    </div>
  );
}

export const Input = React.forwardRef<HTMLInputElement, React.InputHTMLAttributes<HTMLInputElement>>(function Input({ className, ...p }, ref) {
  return (
    <input
      ref={ref}
      className={cn(
        "h-8 w-full rounded-lg bg-panel border border-line px-2.5 text-[13px] text-fg placeholder:text-faint outline-none focus:border-accent focus:ring-2 focus:ring-accent/20 transition-shadow",
        className,
      )}
      {...p}
    />
  );
});

export const Textarea = React.forwardRef<HTMLTextAreaElement, React.TextareaHTMLAttributes<HTMLTextAreaElement>>(function Textarea({ className, ...p }, ref) {
  return (
    <textarea
      ref={ref}
      className={cn(
        "w-full rounded-lg bg-panel border border-line px-3 py-2 text-[13px] text-fg placeholder:text-faint outline-none focus:border-accent focus:ring-2 focus:ring-accent/20 resize-none transition-shadow",
        className,
      )}
      {...p}
    />
  );
});

export function Select({ className, children, ...p }: React.SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select
      className={cn(
        "h-8 rounded-lg bg-panel border border-line px-2 pr-7 text-[13px] text-fg outline-none focus:border-accent appearance-none bg-no-repeat bg-[right_0.5rem_center] bg-[length:12px]",
        className,
      )}
      style={{
        backgroundImage:
          "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 20 20' fill='%238e8e9c'%3E%3Cpath d='M5.5 7.5 10 12l4.5-4.5'/%3E%3C/svg%3E\")",
      }}
      {...p}
    >
      {children}
    </select>
  );
}

export function Switch({ checked, onChange, disabled, label }: { checked: boolean; onChange: (v: boolean) => void; disabled?: boolean; label?: string }) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        "relative inline-flex h-[18px] w-8 shrink-0 rounded-full transition-colors disabled:opacity-50",
        checked ? "bg-accent" : "bg-line-strong",
      )}
    >
      <span className={cn("absolute top-[2px] h-[14px] w-[14px] rounded-full bg-white shadow transition-transform", checked ? "translate-x-[16px]" : "translate-x-[2px]")} />
    </button>
  );
}

export function Segmented<T extends string>({ value, options, onChange, size = "md" }: { value: T; options: { value: T; label: React.ReactNode; title?: string }[]; onChange: (v: T) => void; size?: "sm" | "md" }) {
  return (
    <div className="inline-flex p-0.5 rounded-lg bg-panel-2 border border-line">
      {options.map((o) => (
        <button
          key={o.value}
          title={o.title}
          onClick={() => onChange(o.value)}
          className={cn(
            "rounded-md font-medium transition-colors",
            size === "sm" ? "h-6 px-2 text-xs" : "h-7 px-3 text-[13px]",
            value === o.value ? "bg-panel text-fg shadow-sm" : "text-muted hover:text-fg",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Progress({ value, tone = "accent", className }: { value: number; tone?: "accent" | "ok" | "warn" | "bad" | "local" | "cloud"; className?: string }) {
  const c = { accent: "bg-accent", ok: "bg-ok", warn: "bg-warn", bad: "bg-bad", local: "bg-local", cloud: "bg-cloud" }[tone];
  return (
    <div className={cn("h-1.5 w-full rounded-full bg-panel-2 overflow-hidden", className)}>
      <div className={cn("h-full rounded-full transition-[width] duration-300", c)} style={{ width: `${Math.max(0, Math.min(100, value))}%` }} />
    </div>
  );
}

export function Kbd({ children }: { children: React.ReactNode }) {
  return <kbd className="inline-flex items-center h-5 px-1.5 rounded border border-line bg-panel-2 text-[11px] font-sans text-muted">{children}</kbd>;
}

export function Empty({ icon, title, children, action }: { icon?: React.ReactNode; title: string; children?: React.ReactNode; action?: React.ReactNode }) {
  return (
    <div className="flex flex-col items-center justify-center text-center py-12 px-6">
      {icon && <div className="mb-3 text-faint">{icon}</div>}
      <div className="text-sm font-medium">{title}</div>
      {children && <div className="mt-1 text-muted max-w-sm leading-relaxed">{children}</div>}
      {action && <div className="mt-4">{action}</div>}
    </div>
  );
}

/** Inline rename: double-click or F2 to edit; Enter saves, Esc cancels. */
export function InlineEdit({ value, onSave, className, placeholder }: { value: string; onSave: (v: string) => void | Promise<void>; className?: string; placeholder?: string }) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(value);
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => setDraft(value), [value]);
  useEffect(() => {
    if (editing) {
      ref.current?.focus();
      ref.current?.select();
    }
  }, [editing]);
  const commit = () => {
    setEditing(false);
    const v = draft.trim();
    if (v && v !== value) void onSave(v);
    else setDraft(value);
  };
  if (editing) {
    return (
      <input
        ref={ref}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
          if (e.key === "Escape") {
            setDraft(value);
            setEditing(false);
          }
          e.stopPropagation();
        }}
        className={cn("bg-panel border border-accent rounded px-1 -mx-1 outline-none min-w-0", className)}
      />
    );
  }
  return (
    <span
      tabIndex={0}
      title="Double-click or press F2 to rename"
      onDoubleClick={(e) => {
        e.stopPropagation();
        setEditing(true);
      }}
      onKeyDown={(e) => {
        if (e.key === "F2") setEditing(true);
      }}
      className={cn("truncate cursor-text", className)}
    >
      {value || <span className="text-faint">{placeholder}</span>}
    </span>
  );
}

export function Dialog({ open, onClose, title, children, footer, width = 480, dismissable = true }: { open: boolean; onClose: () => void; title?: React.ReactNode; children: React.ReactNode; footer?: React.ReactNode; width?: number; dismissable?: boolean }) {
  useEffect(() => {
    if (!open || !dismissable) return;
    const h = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [open, onClose, dismissable]);
  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-[2px] animate-fade-in" onMouseDown={() => dismissable && onClose()}>
      <div className="bg-elev border border-line rounded-2xl shadow-2xl max-h-[85vh] flex flex-col animate-pop" style={{ width }} onMouseDown={(e) => e.stopPropagation()}>
        {title && <div className="px-5 pt-4 pb-2 text-[15px] font-semibold">{title}</div>}
        <div className="px-5 py-2 overflow-auto">{children}</div>
        {footer && <div className="px-5 py-3 flex justify-end gap-2 border-t border-line mt-2">{footer}</div>}
      </div>
    </div>
  );
}

export function Field({ label, hint, children, className }: { label: React.ReactNode; hint?: React.ReactNode; children: React.ReactNode; className?: string }) {
  return (
    <div className={cn("flex items-start justify-between gap-6 py-3 border-b border-line last:border-0", className)}>
      <div className="min-w-0">
        <div className="text-[13px] font-medium">{label}</div>
        {hint && <div className="text-xs text-muted mt-0.5 leading-relaxed max-w-md">{hint}</div>}
      </div>
      <div className="shrink-0 flex items-center gap-2">{children}</div>
    </div>
  );
}

export function Tabs<T extends string>({ value, onChange, tabs }: { value: T; onChange: (v: T) => void; tabs: { value: T; label: React.ReactNode; count?: number }[] }) {
  return (
    <div className="flex gap-1 border-b border-line">
      {tabs.map((t) => (
        <button
          key={t.value}
          onClick={() => onChange(t.value)}
          className={cn(
            "relative h-9 px-3 text-[13px] font-medium transition-colors",
            value === t.value ? "text-fg" : "text-muted hover:text-fg",
          )}
        >
          {t.label}
          {t.count != null && <span className="ml-1.5 text-faint">{t.count}</span>}
          {value === t.value && <span className="absolute inset-x-2 -bottom-px h-0.5 rounded bg-accent" />}
        </button>
      ))}
    </div>
  );
}
