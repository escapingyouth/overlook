import { useLayoutEffect, useRef, useState } from "react";
import { Bucket, bucketTotal, Granularity, Report } from "../lib/analytics";
import { axisUnit, bytes, fmtDate, niceTicks, number } from "../lib/format";

export type SeriesMode = "both" | "download" | "upload";

export function bucketTitle(b: Bucket, g: Granularity): string {
  const partial = b.end.getTime() > Date.now() - 60_000 ? " (so far)" : "";
  switch (g) {
    case "hour": {
      const end = new Date(b.start.getTime() + 3_600_000);
      return `${fmtDate(b.start, { weekday: "short", month: "short", day: "numeric" })}, ${fmtDate(b.start, { hour: "numeric" })} – ${fmtDate(end, { hour: "numeric" })}${partial}`;
    }
    case "day":
      return fmtDate(b.start, { weekday: "long", month: "short", day: "numeric" }) + partial;
    case "week":
      return `${fmtDate(b.start, { month: "short", day: "numeric" })} – ${fmtDate(new Date(b.end.getTime() - 1), { month: "short", day: "numeric" })}${partial}`;
    case "month":
      return fmtDate(b.start, { month: "long", year: "numeric" }) + partial;
  }
}

function axisLabel(d: Date, report: Report): string {
  switch (report.granularity) {
    case "hour":
      return report.range === "day" ? fmtDate(d, { hour: "numeric" }) : fmtDate(d, { weekday: "short", hour: "numeric" });
    case "day":
      return report.range === "week" ? fmtDate(d, { weekday: "short" }) : fmtDate(d, { month: "short", day: "numeric" });
    case "week":
      return fmtDate(d, { month: "short", day: "numeric" });
    case "month":
      return fmtDate(d, { month: "short" });
  }
}

function topRounded(x: number, y: number, w: number, h: number, r: number) {
  r = Math.max(0, Math.min(r, w / 2, h));
  return `M${x},${y + h}V${y + r}Q${x},${y} ${x + r},${y}H${x + w - r}Q${x + w},${y} ${x + w},${y + r}V${y + h}Z`;
}

const useWidth = () => {
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    const el = ref.current!;
    const ro = new ResizeObserver(([e]) => setWidth(e.contentRect.width));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  return [ref, width] as const;
};

export function UsageChart({ report, mode, height = 260, compact = false }: { report: Report; mode: SeriesMode; height?: number; compact?: boolean }) {
  const [ref, width] = useWidth();
  const [hover, setHover] = useState<number | null>(null);

  const rx = (b: Bucket) => (mode === "upload" ? 0 : b.rx ?? 0);
  const tx = (b: Bucket) => (mode === "download" ? 0 : b.tx ?? 0);
  const buckets = report.buckets;
  const max = Math.max(0, ...buckets.map((b) => rx(b) + tx(b)));
  const { divisor, unit } = axisUnit(max);
  const ticks = niceTicks(max / divisor, compact ? 2 : 4);
  const top = ticks[ticks.length - 1] * divisor || 1;

  const left = compact ? 0 : 52;
  const bottom = compact ? 0 : 24;
  const plotW = Math.max(0, width - left);
  const plotH = height - bottom;
  const band = buckets.length ? plotW / buckets.length : 0;
  const barW = Math.max(1, Math.min(band * (buckets.length > 40 ? 0.72 : 0.58), 36));
  const y = (v: number) => plotH - (v / top) * plotH;
  const labelStep = Math.max(1, Math.ceil(buckets.length / Math.max(2, Math.floor(plotW / 84))));
  const tracked = buckets.some((b) => b.rx !== null);

  const onMove = (e: React.MouseEvent) => {
    const x = e.clientX - e.currentTarget.getBoundingClientRect().left - left;
    const i = Math.floor(x / band);
    setHover(i >= 0 && i < buckets.length ? i : null);
  };

  const hovered = hover !== null ? buckets[hover] : null;
  const tipLeft = hover !== null ? left + band * (hover + 0.5) : 0;
  const flip = tipLeft > width - 200;

  return (
    <div className="chart" ref={ref} style={{ height }} onMouseLeave={() => setHover(null)}>
      {width > 0 && (
        <svg width={width} height={height} onMouseMove={compact ? undefined : onMove} role="img" aria-label={`Data usage chart, ${report.granularity} totals`}>
          {!compact &&
            ticks.map((t) => (
              <g key={t}>
                <line className="grid" x1={left} x2={width} y1={y(t * divisor)} y2={y(t * divisor)} strokeDasharray={t === 0 ? undefined : "2 4"} />
                <text x={left - 10} y={y(t * divisor)} dy="0.32em" textAnchor="end">{t === 0 ? "0" : `${number(t)} ${unit}`}</text>
              </g>
            ))}
          {hovered && <rect className="hover-band" x={left + band * hover!} y={0} width={band} height={plotH} rx={6} />}
          {buckets.map((b, i) => {
            if (b.rx === null) return null;
            const x = left + band * i + (band - barW) / 2;
            const d = rx(b);
            const u = tx(b);
            const total = bucketTotal(b) ?? 0;
            const faded = hover !== null && hover !== i;
            const estimated = total > 0 && b.estimated / total > 0.5;
            const opacity = (faded ? 0.4 : 1) * (estimated ? 0.5 : 1);
            const gap = d > 0 && u > 0 ? 2 : 0;
            const r = compact ? 1.5 : 3;
            return (
              <g key={i} opacity={opacity}>
                {!compact && <path className="track" d={topRounded(x, 0, barW, plotH, r)} />}
                {d > 0 && <path d={topRounded(x, y(d), barW, plotH - y(d), u > 0 ? 0 : r)} fill="var(--download)" />}
                {u > 0 && <path d={topRounded(x, y(d + u), barW, Math.max(1, y(d) - y(d + u) - gap), r)} fill="var(--upload)" />}
              </g>
            );
          })}
          {!compact &&
            buckets.map((b, i) =>
              i % labelStep === 0 ? (
                <text key={i} x={left + band * (i + 0.5)} y={height - 6} textAnchor="middle">{axisLabel(b.start, report)}</text>
              ) : null,
            )}
        </svg>
      )}
      {!compact && max === 0 && (
        <div className="chart-empty">
          <div>
            <strong>{tracked ? "No data used in this period" : "Not tracked during this period"}</strong>
            {tracked ? "Usage appears here as it's recorded." : "Tally can only count traffic while it's running."}
          </div>
        </div>
      )}
      {hovered && (
        <div className="tooltip" style={{ top: 4, left: flip ? undefined : tipLeft + 14, right: flip ? width - tipLeft + 14 : undefined }}>
          <div className="t-title">{bucketTitle(hovered, report.granularity)}</div>
          {hovered.rx === null ? (
            <div className="t-row">Not tracked: Tally wasn't running</div>
          ) : (
            <>
              {mode !== "upload" && <div className="t-row"><span className="swatch" style={{ background: "var(--download)" }} />Download<span className="v num">{bytes(hovered.rx)}</span></div>}
              {mode !== "download" && <div className="t-row"><span className="swatch" style={{ background: "var(--upload)" }} />Upload<span className="v num">{bytes(hovered.tx ?? 0)}</span></div>}
              {mode === "both" && <div className="t-row t-total">Total<span className="v num">{bytes(bucketTotal(hovered) ?? 0)}</span></div>}
              {hovered.estimated > 0 && <div className="t-note">Includes {bytes(hovered.estimated)} estimated</div>}
            </>
          )}
        </div>
      )}
    </div>
  );
}

export function PatternChart({ values, labels, height = 150 }: { values: { rx: number; tx: number }[]; labels: (string | null)[]; height?: number }) {
  const [ref, width] = useWidth();
  const max = Math.max(0, ...values.map((v) => v.rx + v.tx));
  const { divisor, unit } = axisUnit(max);
  const ticks = niceTicks(max / divisor, 2);
  const top = ticks[ticks.length - 1] * divisor || 1;
  const left = 52;
  const plotH = height - 22;
  const band = (width - left) / values.length;
  const barW = Math.max(2, band * 0.56);
  const y = (v: number) => plotH - (v / top) * plotH;
  return (
    <div className="chart" ref={ref} style={{ height }}>
      {width > 0 && (
        <svg width={width} height={height} role="img" aria-label="Typical usage pattern">
          {ticks.map((t) => (
            <g key={t}>
              <line className="grid" x1={left} x2={width} y1={y(t * divisor)} y2={y(t * divisor)} strokeDasharray={t === 0 ? undefined : "2 4"} />
              <text x={left - 10} y={y(t * divisor)} dy="0.32em" textAnchor="end">{t === 0 ? "0" : `${number(t)} ${unit}`}</text>
            </g>
          ))}
          {values.map((v, i) => {
            const x = left + band * i + (band - barW) / 2;
            const gap = v.rx > 0 && v.tx > 0 ? 1.5 : 0;
            return (
              <g key={i}>
                <title>{`${labels[i] ?? ""} · ${bytes(v.rx + v.tx)} on average`}</title>
                {v.rx > 0 && <path d={topRounded(x, y(v.rx), barW, plotH - y(v.rx), v.tx > 0 ? 0 : 2)} fill="var(--download)" />}
                {v.tx > 0 && <path d={topRounded(x, y(v.rx + v.tx), barW, Math.max(1, y(v.rx) - y(v.rx + v.tx) - gap), 2)} fill="var(--upload)" />}
                {labels[i] && <text x={left + band * (i + 0.5)} y={height - 5} textAnchor="middle">{labels[i]}</text>}
              </g>
            );
          })}
        </svg>
      )}
      {max === 0 && <div className="chart-empty">Not enough data yet</div>}
    </div>
  );
}

export type SeriesVisibility = { download: boolean; upload: boolean };

export function TrendChart({ report, show, height = 280 }: { report: Report; show: SeriesVisibility; height?: number }) {
  const [ref, width] = useWidth();
  const [hover, setHover] = useState<number | null>(null);
  const buckets = report.buckets;
  const series = ([
    { key: "download", color: "var(--download)", value: (b: Bucket) => b.rx },
    { key: "upload", color: "var(--upload)", value: (b: Bucket) => (b.rx === null ? null : b.tx ?? 0) },
  ] as const).filter((s) => show[s.key]);
  const max = Math.max(0, ...buckets.flatMap((b) => series.map((s) => s.value(b) ?? 0)));
  const { divisor, unit } = axisUnit(max);
  const ticks = niceTicks(max / divisor, 5);
  const top = ticks[ticks.length - 1] * divisor || 1;
  const left = 62;
  const right = 8;
  const plotTop = 8;
  const plotH = height - 30;
  const band = buckets.length ? (width - left - right) / buckets.length : 0;
  const x = (i: number) => left + band * (i + 0.5);
  const y = (v: number) => plotTop + (plotH - plotTop) - (v / top) * (plotH - plotTop);
  const labelStep = Math.max(1, Math.ceil(buckets.length / Math.max(2, Math.floor((width - left) / 84))));

  const segments: number[][] = [];
  buckets.forEach((b, i) => {
    if (b.rx === null) return;
    const last = segments[segments.length - 1];
    if (last && last[last.length - 1] === i - 1) last.push(i);
    else segments.push([i]);
  });
  const line = (seg: number[], v: (b: Bucket) => number | null) => seg.map((i, k) => `${k ? "L" : "M"}${x(i)},${y(v(buckets[i]) ?? 0)}`).join("");
  const area = (seg: number[], v: (b: Bucket) => number | null) => `${line(seg, v)}L${x(seg[seg.length - 1])},${y(0)}L${x(seg[0])},${y(0)}Z`;

  const onMove = (e: React.MouseEvent) => {
    const i = Math.floor((e.clientX - e.currentTarget.getBoundingClientRect().left - left) / band);
    setHover(i >= 0 && i < buckets.length ? i : null);
  };
  const hovered = hover !== null ? buckets[hover] : null;
  const flip = hover !== null && x(hover) > width - 210;

  return (
    <div className="chart" ref={ref} style={{ height }} onMouseLeave={() => setHover(null)}>
      {width > 0 && (
        <svg width={width} height={height} onMouseMove={onMove} role="img" aria-label={`Download and upload trend, ${report.granularity} totals in ${unit}`}>
          <defs>
            {series.map((s) => (
              <linearGradient key={s.key} id={`fill-${s.key}`} x1="0" x2="0" y1="0" y2="1">
                <stop offset="0%" stopColor={s.color} stopOpacity={0.22} />
                <stop offset="100%" stopColor={s.color} stopOpacity={0} />
              </linearGradient>
            ))}
          </defs>
          <text className="axis-title" transform={`translate(12 ${(plotTop + plotH) / 2}) rotate(-90)`} textAnchor="middle">{`Data (${unit})`}</text>
          {ticks.map((t) => (
            <g key={t}>
              <line className="grid" x1={left} x2={width - right} y1={y(t * divisor)} y2={y(t * divisor)} />
              <text x={left - 12} y={y(t * divisor)} dy="0.32em" textAnchor="end">{number(t)}</text>
            </g>
          ))}
          {series.slice(0, 1).map((s) => segments.map((seg, k) => <path key={`a${k}`} d={area(seg, s.value)} fill={`url(#fill-${s.key})`} />))}
          {series.map((s) =>
            segments.map((seg, k) =>
              seg.length > 1 ? (
                <path key={`${s.key}${k}`} d={line(seg, s.value)} fill="none" stroke={s.color} strokeWidth={1.75} strokeLinejoin="round" strokeLinecap="round" />
              ) : (
                <circle key={`${s.key}${k}`} cx={x(seg[0])} cy={y(s.value(buckets[seg[0]]) ?? 0)} r={2.5} fill={s.color} />
              ),
            ),
          )}
          {hover !== null && (
            <g>
              <line x1={x(hover)} x2={x(hover)} y1={plotTop} y2={y(0)} stroke="var(--border-strong)" strokeDasharray="3 4" />
              {hovered!.rx !== null &&
                series.map((s) => <circle key={s.key} cx={x(hover)} cy={y(s.value(hovered!) ?? 0)} r={4.5} fill="var(--surface)" stroke={s.color} strokeWidth={2} />)}
            </g>
          )}
          {buckets.map((b, i) =>
            i % labelStep === 0 ? <text key={i} x={x(i)} y={height - 8} textAnchor="middle" style={{ fill: "var(--text-2)" }}>{axisLabel(b.start, report)}</text> : null,
          )}
        </svg>
      )}
      {(max === 0 || series.length === 0) && (
        <div className="chart-empty">
          <div>
            <strong>{series.length === 0 ? "No series selected" : segments.length ? "No data used in this period" : "Not tracked during this period"}</strong>
            {series.length === 0 ? "Turn on Download or Upload in the legend." : segments.length ? "Usage appears here as it's recorded." : "Tally can only count traffic while it's running."}
          </div>
        </div>
      )}
      {hovered && series.length > 0 && (
        <div className="tooltip" style={{ top: 0, left: flip ? undefined : x(hover!) + 14, right: flip ? width - x(hover!) + 14 : undefined }}>
          <div className="t-title">{bucketTitle(hovered, report.granularity)}</div>
          {hovered.rx === null ? (
            <div className="t-row">Not tracked: Tally wasn't running</div>
          ) : (
            <>
              <div className="t-row"><span className="swatch" style={{ background: "var(--download)", borderRadius: "50%" }} />Download<span className="v num">{bytes(hovered.rx)}</span></div>
              <div className="t-row"><span className="swatch" style={{ background: "var(--upload)", borderRadius: "50%" }} />Upload<span className="v num">{bytes(hovered.tx ?? 0)}</span></div>
              <div className="t-row t-total">Total<span className="v num">{bytes(bucketTotal(hovered) ?? 0)}</span></div>
              {hovered.estimated > 0 && <div className="t-note">Includes {bytes(hovered.estimated)} estimated</div>}
            </>
          )}
        </div>
      )}
    </div>
  );
}
