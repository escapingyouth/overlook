import { createContext, useContext, useState } from "react";
import { GRANULARITY_LABEL, Granularity, hourOf, RangeId, rangeInfo, Report, weekStart } from "../lib/analytics";
import { api, displayName, Network, Status, useLive, useRate, useReport } from "../lib/api";
import { ago, bytes, fmtDate, number, percent, rate, scaled } from "../lib/format";
import { Icon, IconName, kindIcon } from "./Icon";
import { Bytes, Card, Segmented } from "./ui";
import { bucketTitle, PatternChart, SeriesVisibility, TrendChart } from "./UsageChart";
import { Donut } from "./Spark";
import { CapsCard } from "./Caps";

const ReportContext = createContext<Report | null>(null);
const useReportContext = () => useContext(ReportContext)!;

export function Dashboard({ status, network, range, narrow, onSelect, onRename, onAccuracy, onCaps }: {
  status: Status;
  network: Network | null;
  range: RangeId;
  narrow: boolean;
  onSelect: (id: number) => void;
  onRename: (n: Network) => void;
  onAccuracy: () => void;
  onCaps: () => void;
}) {
  const [granularity, setGranularity] = useState<Partial<Record<RangeId, Granularity>>>({});
  const [show, setShow] = useState<SeriesVisibility>({ download: true, upload: true });
  const [pattern, setPattern] = useState<"hour" | "weekday">("hour");
  const report = useReport(network?.id ?? null, range, granularity[range], status.firstCoveredHour);
  if (!report) return null;
  const info = rangeInfo(range);

  return (
    <ReportContext.Provider value={report}>
    <div className="page">
      <PageHead status={status} network={network} />

      <div className="kpi-grid">
        <KpiCard icon="arrows" title="Total used" value={report.total} previous={report.previousTotal} />
        <KpiCard icon="cloudDown" title="Downloaded" value={report.rx} previous={report.previousRx} />
        <KpiCard icon="cloudUp" title="Uploaded" value={report.tx} previous={report.previousTx} />
        <KpiCard icon="average" title="Daily average" value={report.dailyAverage} previous={null}
          note={report.trackedDays ? `${report.trackedDays} day${report.trackedDays === 1 ? "" : "s"}` : undefined} />
      </div>

      <CapsCard status={status} network={network} onManage={onCaps} />

      <div className="main-grid">
        <SplitDonut report={report} />
        <section className="card">
          <div className="card-head">
            <div style={{ minWidth: 0 }}>
              <h3 className="card-title lg">Usage trend</h3>
              <div className="card-sub" style={{ whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                {GRANULARITY_LABEL[report.granularity]} · <Peak report={report} />
              </div>
            </div>
            {info.granularities.length > 1 && (
              <Segmented size="sm" label="Granularity" value={report.granularity} onChange={(g) => setGranularity({ ...granularity, [range]: g })}
                options={info.granularities.map((g) => ({ value: g, label: GRANULARITY_LABEL[g] }))} />
            )}
          </div>
          <div style={{ display: "flex", justifyContent: "flex-end", gap: 12, margin: "-4px 0 6px" }}>
            {(["download", "upload"] as const).map((k) => (
              <button key={k} className="legend-toggle" aria-pressed={show[k]} style={{ "--c": `var(--${k})` } as React.CSSProperties}
                title={`${show[k] ? "Hide" : "Show"} ${k}`} onClick={() => setShow({ ...show, [k]: !show[k] })}>
                <span className="ldot" />{k === "download" ? "Download" : "Upload"}
              </button>
            ))}
          </div>
          <TrendChart report={report} show={show} height={narrow ? 240 : 290} />
        </section>
      </div>

      {network ? <DaysTable report={report} /> : <NetworksTable status={status} report={report} onSelect={onSelect} />}

      <div className={`row ${network ? "three" : ""}`}>
        {range !== "day" && (
          <Card title="Typical usage" subtitle={pattern === "hour" ? "Average per tracked day" : "Average per tracked weekday"}
            action={<Segmented size="sm" label="Pattern" value={pattern} onChange={setPattern} options={[{ value: "hour", label: "Hour" }, { value: "weekday", label: "Weekday" }]} />}>
            {pattern === "hour" ? (
              <PatternChart values={report.hourOfDay} labels={report.hourOfDay.map((_, h) => (h % 6 === 0 ? fmtDate(new Date(2026, 0, 1, h), { hour: "numeric" }) : null))} />
            ) : (
              <PatternChart values={weekOrder().map((d) => report.weekday[d])} labels={weekOrder().map((d) => fmtDate(new Date(2026, 0, 4 + d), { weekday: "short" }))} />
            )}
          </Card>
        )}
        <HighlightsCard report={report} />
        {network && <NetworkCard network={network} status={status} onRename={onRename} />}
      </div>

      <Footnote report={report} network={network} onAccuracy={onAccuracy} />
    </div>
    </ReportContext.Provider>
  );
}

function weekOrder() {
  const first = weekStart();
  return Array.from({ length: 7 }, (_, i) => (first + i) % 7);
}

function Peak({ report }: { report: Report }) {
  const peak = report.buckets.reduce<Report["buckets"][number] | null>(
    (m, b) => ((b.rx ?? 0) + (b.tx ?? 0) > (m ? (m.rx ?? 0) + (m.tx ?? 0) : 0) ? b : m), null);
  return <>{peak ? `Peak ${bytes((peak.rx ?? 0) + (peak.tx ?? 0))} · ${bucketTitle(peak, report.granularity)}` : "No usage yet"}</>;
}

function PageHead({ status, network }: { status: Status; network: Network | null }) {
  const active = network ? status.live.active.find((a) => a.networkId === network.id) : status.live.active.find((a) => a.networkId === status.live.primary);
  const current = status.networks.find((n) => n.id === status.live.primary);
  const live = network ? !!active : !!current;
  const now = useRate(status.live.rate);
  const showRate = now && (network ? status.live.primary === network.id : true);
  return (
    <div className="page-head">
      <div style={{ minWidth: 0 }}>
        <h2>{network ? displayName(network) : "Overview"}</h2>
        <div className="muted" style={{ display: "flex", alignItems: "center", gap: 7, fontSize: 12.5 }}>
          <span className={`dot ${live ? "" : "off"}`} />
          {network
            ? active ? `Connected · ${active.kind === "wifi" ? "Wi-Fi" : "Ethernet"} · ${active.interface}` : `Not connected · last seen ${ago(network.lastSeen)}`
            : current ? `${status.networks.length} networks · on ${displayName(current)}` : `${status.networks.length} networks · offline`}
        </div>
      </div>
      {showRate && (
        <span className="rate num" title="Updated every second">
          <span><Icon name="down" size={13} style={{ color: "var(--download)" }} />{rate(now![0])}</span>
          <span><Icon name="up" size={13} style={{ color: "var(--upload)" }} />{rate(now![1])}</span>
        </span>
      )}
    </div>
  );
}

function DeltaPill({ value, previous, report }: { value: number | null; previous: number | null; report: Report }) {
  if (value === null || previous === null || previous === 0) return null;
  const change = (value - previous) / previous;
  const text = Math.abs(change) < 0.0005 ? "±0%" : `${change > 0 ? "+" : "−"}${percent(Math.abs(change))}`;
  return <span className="delta-pill num" title={`vs ${bytes(previous)} in the ${rangeInfo(report.range).previous}`}>{text}</span>;
}

function KpiCard({ icon, title, value, previous, note }: { icon: IconName; title: string; value: number | null; previous: number | null; note?: string }) {
  const report = useReportContext();
  return (
    <section className="kpi-card">
      <div className="kpi-top">
        <span className="icon-tile lg"><Icon name={icon} size={18} /></span>
      </div>
      <div className="kpi-label">{title}</div>
      <div className="kpi-line">
        <Bytes value={value} />
        <DeltaPill value={value} previous={previous} report={report} />
        {note && <span className="delta-pill">{note}</span>}
      </div>
    </section>
  );
}

function SplitDonut({ report }: { report: Report }) {
  const [v, unit] = scaled(report.total);
  const item = (icon: IconName, label: string, value: number, color: string) => (
    <div className="result">
      <span className="icon-tile lg" style={{ color }}><Icon name={icon} size={18} /></span>
      <div style={{ minWidth: 0 }}>
        <div style={{ display: "flex", flexWrap: "wrap", alignItems: "baseline", columnGap: 6 }}>
          <span className="num" style={{ fontWeight: 600, fontSize: 14, whiteSpace: "nowrap" }}>{bytes(value)}</span>
          <span className="faint num" style={{ fontSize: 12 }}>{report.total ? percent(value / report.total) : "—"}</span>
        </div>
        <div className="faint" style={{ fontSize: 12 }}>{label}</div>
      </div>
    </div>
  );
  return (
    <section className="card" style={{ display: "flex", flexDirection: "column" }}>
      <div className="card-head">
        <div>
          <h3 className="card-title lg">Download vs upload</h3>
          <div className="card-sub">{report.estimated > 0 ? `Includes ${bytes(report.estimated)} estimated` : "All measured"}</div>
        </div>
      </div>
      <div style={{ flex: 1, display: "grid", placeItems: "center", padding: "8px 0 22px" }}>
        <Donut parts={[{ value: report.rx, color: "var(--download)", label: "Download" }, { value: report.tx, color: "var(--upload)", label: "Upload" }]}>
          <div>
            <div className="num" style={{ fontSize: 26, fontWeight: 600, letterSpacing: "-0.02em" }}>
              {number(v)} <span className="muted" style={{ fontSize: 15, fontWeight: 500 }}>{unit}</span>
            </div>
            <div className="faint" style={{ fontSize: 12 }}>Total used</div>
          </div>
        </Donut>
      </div>
      <div className="results">
        {item("cloudDown", "Downloaded", report.rx, "var(--download)")}
        {item("cloudUp", "Uploaded", report.tx, "var(--upload)")}
      </div>
    </section>
  );
}

function NetworksTable({ status, report, onSelect }: { status: Status; report: Report; onSelect: (id: number) => void }) {
  const totals = useLive(() => api.totals(hourOf(report.start.getTime()), hourOf(report.end.getTime()) + 1), [report.start.getTime()]) ?? [];
  const rows = status.networks
    .map((n) => {
      const t = totals.find((t) => t.networkId === n.id);
      return { n, rx: t?.rx ?? 0, tx: t?.tx ?? 0 };
    })
    .sort((a, b) => b.rx + b.tx - (a.rx + a.tx));
  return (
    <section className="panel">
      <div className="panel-head"><h2>Networks</h2><span className="faint" style={{ fontSize: 12 }}>{rangeInfo(report.range).title}</span></div>
      <div style={{ overflowX: "auto" }}>
        <table className="table">
          <thead>
            <tr><th>Network</th><th>Type</th><th>Identified by</th><th className="r">Downloaded</th><th className="r">Uploaded</th><th className="r">Total</th><th>Status</th></tr>
          </thead>
          <tbody>
            {rows.map(({ n, rx, tx }) => {
              const live = status.live.active.some((a) => a.networkId === n.id);
              return (
                <tr key={n.id} className="clickable" onClick={() => onSelect(n.id)} title={`Show ${displayName(n)}`}>
                  <td><span className="name-cell"><span className="icon-tile"><Icon name={kindIcon(n.kind)} size={14} /></span>{displayName(n)}</span></td>
                  <td className="muted">{n.kind === "wifi" ? "Wi-Fi" : "Ethernet"}</td>
                  <td className="muted">{n.ssid ? "Network name" : "Router address"}</td>
                  <td className="r num">{bytes(rx)}</td>
                  <td className="r num">{bytes(tx)}</td>
                  <td className="r num" style={{ fontWeight: 500 }}>{bytes(rx + tx)}</td>
                  <td>{live ? <span className="pill ok"><Icon name="ok" size={13} />Connected</span> : <span className="pill muted"><Icon name="clock" size={13} />{ago(n.lastSeen)}</span>}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function DaysTable({ report }: { report: Report }) {
  const days = [...report.days].reverse().slice(0, 14);
  const today = new Date().toDateString();
  return (
    <section className="panel">
      <div className="panel-head">
        <h2>Daily usage</h2>
        <span className="faint" style={{ fontSize: 12 }}>{report.days.length > 14 ? "Most recent 14 days" : rangeInfo(report.range).title}</span>
      </div>
      <div style={{ overflowX: "auto" }}>
        <table className="table">
          <thead>
            <tr><th>Day</th><th className="r">Downloaded</th><th className="r">Uploaded</th><th className="r">Total</th><th>Status</th></tr>
          </thead>
          <tbody>
            {days.map((d) => (
              <tr key={d.day.getTime()}>
                <td>{fmtDate(d.day, { weekday: "short", month: "short", day: "numeric" })}{d.day.toDateString() === today && <span className="faint"> · today</span>}</td>
                <td className="r num">{d.coveredSeconds ? bytes(d.rx) : "—"}</td>
                <td className="r num">{d.coveredSeconds ? bytes(d.tx) : "—"}</td>
                <td className="r num" style={{ fontWeight: 500 }}>{d.coveredSeconds ? bytes(d.rx + d.tx) : "—"}</td>
                <td>
                  {!d.coveredSeconds ? (
                    <span className="pill muted" title="Overlook wasn't running"><Icon name="clock" size={13} />Not tracked</span>
                  ) : d.estimated > 0 ? (
                    <span className="pill warn" title="Attributed across a network switch or a sleep/restart gap"><Icon name="alert" size={13} />{bytes(d.estimated)} estimated</span>
                  ) : (
                    <span className="pill ok"><Icon name="ok" size={13} />Measured</span>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function HighlightsCard({ report }: { report: Report }) {
  const day = (d: Date) => fmtDate(d, { weekday: "short", month: "short", day: "numeric" });
  const item = (label: string, value: number | null, detail?: string) => (
    <div className="list-row">
      <span className="label">{label}</span>
      {detail && <span className="faint" style={{ fontSize: 12 }}>{detail}</span>}
      <span className={`value num ${value === null ? "faint" : ""}`} style={{ minWidth: 64, textAlign: "right" }}>{value === null ? "—" : bytes(value)}</span>
    </div>
  );
  return (
    <Card title="Highlights">
      {item("Highest day", report.peakDay?.total ?? null, report.peakDay ? day(report.peakDay.day) : undefined)}
      {item("Lowest day", report.lowestDay?.total ?? null, report.lowestDay ? day(report.lowestDay.day) : "Needs 3 complete days")}
      {report.range !== "day" && report.range !== "week" && item("Weekly average", report.weeklyAverage, report.weeklyAverage === null ? "Needs 7 tracked days" : undefined)}
      {item("Previous period", report.previousTotal, report.previousTotal === null ? "No history yet" : undefined)}
    </Card>
  );
}

function NetworkCard({ network, status, onRename }: { network: Network; status: Status; onRename: (n: Network) => void }) {
  const live = status.live.active.some((a) => a.networkId === network.id);
  const when = (s: number) => fmtDate(new Date(s * 1000), { dateStyle: "medium", timeStyle: "short" });
  const item = (label: string, value: string, help?: string) => (
    <div className="list-row" title={help}>
      <span className="label">{label}</span>
      <span className="value num" style={{ userSelect: "text" }}>{value}</span>
    </div>
  );
  return (
    <Card title="About this network" action={<button className="btn ghost" onClick={() => onRename(network)}><Icon name="edit" size={13} />Rename</button>}>
      {item("Identified by", network.ssid ? "Network name" : "Router address",
        network.ssid ? undefined : "Without access to Wi-Fi names, this network is recognised by its router's MAC address: reliable, but anonymous.")}
      {network.routerMac && item("Router", network.routerMac.toUpperCase())}
      {item("First seen", when(network.firstSeen))}
      {item("Last seen", live ? "Now" : when(network.lastSeen))}
    </Card>
  );
}

function Footnote({ report, network, onAccuracy }: { report: Report; network: Network | null; onAccuracy: () => void }) {
  const dropped = useLive(() => api.droppedBytes(report.start.getTime() / 1000), [report.start.getTime()]) ?? 0;
  const parts = ["Measured from interface byte counters, including local-network traffic."];
  if (report.estimated > 0) parts.push(`${bytes(report.estimated)} is estimated (attributed across a network switch or a sleep/restart gap).`);
  if (!network && dropped > 0) parts.push(`${bytes(dropped)} couldn't be attributed to any network and isn't counted.`);
  return (
    <div className="footnote">
      <Icon name="info" size={13} style={{ marginTop: 1, flex: "none" }} />
      <span>{parts.join(" ")} <button className="link" onClick={onAccuracy}>How Overlook measures</button></span>
    </div>
  );
}
