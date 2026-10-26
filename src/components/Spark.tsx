
export function Donut({ parts, size = 184, thickness = 18, children }: { parts: { value: number; color: string; label: string }[]; size?: number; thickness?: number; children?: React.ReactNode }) {
  const r = (size - thickness) / 2;
  const c = 2 * Math.PI * r;
  const total = parts.reduce((s, p) => s + p.value, 0);
  const visible = parts.filter((p) => p.value > 0);
  const gap = visible.length > 1 ? thickness + 8 : 0;
  let start = 0;
  const arcs = visible.map((p) => {
    const span = (p.value / total) * c;
    const arc = { ...p, offset: start + gap / 2, length: Math.max(0.01, span - gap) };
    start += span;
    return arc;
  });
  return (
    <div style={{ position: "relative", width: size, height: size }}>
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} role="img"
        aria-label={parts.map((p) => `${p.label} ${total ? Math.round((p.value / total) * 100) : 0}%`).join(", ")} style={{ transform: "rotate(-90deg)" }}>
        <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="var(--track)" strokeWidth={thickness} />
        {total > 0 && arcs.map((a) => (
          <circle key={a.label} cx={size / 2} cy={size / 2} r={r} fill="none" stroke={a.color} strokeWidth={thickness} strokeLinecap="round"
            strokeDasharray={`${a.length} ${c}`} strokeDashoffset={-a.offset} />
        ))}
      </svg>
      <div style={{ position: "absolute", inset: 0, display: "grid", placeItems: "center", textAlign: "center" }}>{children}</div>
    </div>
  );
}
