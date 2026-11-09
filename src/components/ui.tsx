import { ReactNode, useLayoutEffect, useRef } from "react";
import { scaled, number } from "../lib/format";

export function Segmented<T extends string>({ value, options, onChange, label }: Readonly<{
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
  label: string;
}>) {
  return (
    <div className="seg sm" role="group" aria-label={label}>
      {options.map((o) => (
        <button type="button" key={o.value} aria-pressed={o.value === value} onClick={() => onChange(o.value)}>
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

export function Card({ title, subtitle, action, children }: Readonly<{ title: string; subtitle?: ReactNode; action?: ReactNode; children: ReactNode }>) {
  return (
    <section className="card">
      <div className="card-head">
        <div>
          <h3 className="card-title">{title}</h3>
          {subtitle && <div className="card-sub">{subtitle}</div>}
        </div>
        {action}
      </div>
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
  // A layout effect, so close() runs while the dialog is still attached and can hand focus back.
  useLayoutEffect(() => {
    const dialog = ref.current!;
    dialog.showModal();
    dialog.querySelector<HTMLElement>("[data-autofocus]")?.focus();
    return () => dialog.close();
  }, []);
  // React bubbles these through nested dialogs; the target check keeps them to the topmost one.
  const dismiss = (e: React.SyntheticEvent) => e.target === e.currentTarget && onClose();
  return (
    <dialog ref={ref} className={`dialog ${wide ? "wide" : ""}`} onCancel={dismiss} onMouseDown={dismiss}>
      <div>{children}</div>
    </dialog>
  );
}
