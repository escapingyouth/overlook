import { ReactNode, useEffect, useRef } from "react";
import { scaled, number } from "../lib/format";

export function Segmented<T extends string>({ value, options, onChange, size, label }: Readonly<{
  value: T;
  options: { value: T; label: string; title?: string }[];
  onChange: (v: T) => void;
  size?: "sm";
  label: string;
}>) {
  return (
    <div className={`seg ${size ?? ""}`} role="group" aria-label={label}>
      {options.map((o) => (
        <button type="button" key={o.value} aria-pressed={o.value === value} title={o.title} onClick={() => onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Setting({ title, desc, children }: Readonly<{ title: string; desc?: string; children: ReactNode }>) {
  return (
    <div className="setting">
      <div>
        <div>{title}</div>
        {desc && <div className="desc">{desc}</div>}
      </div>
      {children}
    </div>
  );
}

export function Toggle({ checked, onChange, label }: Readonly<{ checked: boolean; onChange: (v: boolean) => void; label: string }>) {
  return <button type="button" className="toggle" role="switch" aria-checked={checked} aria-label={label} onClick={() => onChange(!checked)} />;
}

export function Card({ title, subtitle, action, children, className }: Readonly<{ title?: string; subtitle?: ReactNode; action?: ReactNode; children: ReactNode; className?: string }>) {
  return (
    <section className={`card ${className ?? ""}`}>
      {(title || action) && (
        <div className="card-head">
          <div>
            {title && <h3 className="card-title">{title}</h3>}
            {subtitle && <div className="card-sub">{subtitle}</div>}
          </div>
          {action}
        </div>
      )}
      {children}
    </section>
  );
}

export function Bytes({ value, className = "stat-value" }: Readonly<{ value: number | null | undefined; className?: string }>) {
  if (value === null || value === undefined) return <div className={className}><span className="big faint">—</span></div>;
  const [v, unit] = scaled(value);
  return (
    <div className={className}>
      <span className="big num">{number(v)}</span>
      <span className="unit">{unit}</span>
    </div>
  );
}

export function Dialog({ children, onClose, wide }: Readonly<{ children: ReactNode; onClose: () => void; wide?: boolean }>) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const key = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [onClose]);
  // Move focus into the dialog (the [data-autofocus] element, else the first control) and give it back on close.
  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    ref.current?.querySelector<HTMLElement>("[data-autofocus], button, input, select, textarea")?.focus();
    return () => opener?.focus();
  }, []);
  return (
    <div className="backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <dialog open ref={ref} className={`dialog ${wide ? "wide" : ""}`} aria-modal>
        {children}
      </dialog>
    </div>
  );
}

export function SplitBar({ rx, tx, height = 8 }: Readonly<{ rx: number; tx: number; height?: number }>) {
  const total = rx + tx;
  return (
    <div className="split" style={{ height }} aria-hidden>
      {total > 0 && rx > 0 && <div style={{ width: `${(rx / total) * 100}%`, background: "var(--download)", minWidth: height }} />}
      {total > 0 && tx > 0 && <div style={{ flex: 1, background: "var(--upload)", minWidth: height }} />}
    </div>
  );
}
