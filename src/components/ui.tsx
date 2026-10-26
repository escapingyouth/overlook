import { ReactNode, useEffect } from "react";
import { scaled, number } from "../lib/format";

export function Segmented<T extends string>({ value, options, onChange, size, label }: {
  value: T;
  options: { value: T; label: string; title?: string }[];
  onChange: (v: T) => void;
  size?: "sm";
  label: string;
}) {
  return (
    <div className={`seg ${size ?? ""}`} role="group" aria-label={label}>
      {options.map((o) => (
        <button key={o.value} aria-pressed={o.value === value} title={o.title} onClick={() => onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Toggle({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return <button className="toggle" role="switch" aria-checked={checked} aria-label={label} onClick={() => onChange(!checked)} />;
}

export function Card({ title, subtitle, action, children, className }: { title?: string; subtitle?: ReactNode; action?: ReactNode; children: ReactNode; className?: string }) {
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

export function Bytes({ value, className = "stat-value" }: { value: number | null | undefined; className?: string }) {
  if (value === null || value === undefined) return <div className={className}><span className="big faint">—</span></div>;
  const [v, unit] = scaled(value);
  return (
    <div className={className}>
      <span className="big num">{number(v)}</span>
      <span className="unit">{unit}</span>
    </div>
  );
}

export function Dialog({ children, onClose, wide }: { children: ReactNode; onClose: () => void; wide?: boolean }) {
  useEffect(() => {
    const key = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [onClose]);
  return (
    <div className="backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className={`dialog ${wide ? "wide" : ""}`} role="dialog" aria-modal>
        {children}
      </div>
    </div>
  );
}

export function SplitBar({ rx, tx, height = 8 }: { rx: number; tx: number; height?: number }) {
  const total = rx + tx;
  return (
    <div className="split" style={{ height }} aria-hidden>
      {total > 0 && rx > 0 && <div style={{ width: `${(rx / total) * 100}%`, background: "var(--download)", minWidth: height }} />}
      {total > 0 && tx > 0 && <div style={{ flex: 1, background: "var(--upload)", minWidth: height }} />}
    </div>
  );
}
