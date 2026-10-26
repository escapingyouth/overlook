import { useEffect, useRef, useState } from "react";
import { api, Cap, CapUsage, displayName, Network, Status, useCaps, useLive } from "../lib/api";
import { bytes, fmtDate, percent } from "../lib/format";
import { capForecast, CapForecast, hourOf, weekStart } from "../lib/analytics";
import { Icon, IconName, kindIcon } from "./Icon";
import { Card, Segmented, Setting, Toggle } from "./ui";

const PERIOD = { day: "Daily", week: "Weekly", month: "Monthly" } as const;
const PER = { day: "a day", week: "a week", month: "a month" } as const;
const PERIOD_DAYS = { day: 1, week: 7, month: 30.44 } as const;
const PRESETS = [50, 80, 90, 100];
const SPEEDS = [256, 1000, 5000];
const kbps = (k: number) => (k >= 1000 ? `${k / 1000} Mbps` : `${k} kbps`);
const day = (sec: number) => fmtDate(new Date(sec * 1000), { month: "short", day: "numeric" });

export const capScope = (cap: Cap, networks: Network[]) => {
  if (cap.networkId === null) return "All networks";
  const n = networks.find((n) => n.id === cap.networkId);
  return n ? displayName(n) : "Removed network";
};

/** Caps that apply to a network: its own plus the all-networks ones. Null network = every cap. */
export const capsFor = (caps: CapUsage[], networkId: number | null) =>
  networkId === null ? caps : caps.filter((c) => c.networkId === null || c.networkId === networkId);

function capStatus(cap: Cap, f: CapForecast): { tone: "ok" | "warn" | "danger"; label: string } {
  if (f.share >= 1) return { tone: "danger", label: "Used up" };
  if (f.runsOutAt) return { tone: "warn", label: `Runs out ${day(f.runsOutAt)}` };
  const crossed = cap.alerts.filter((a) => f.share * 100 >= a);
  if (crossed.length) return { tone: "warn", label: `Past ${Math.max(...crossed)}%` };
  return { tone: "ok", label: "On track" };
}

const TONE = { ok: "var(--download)", warn: "var(--warn)", danger: "var(--danger)" };

/** Used so far, the pace projection (hatched), alert notches and a marker for today. */
export function CapMeter({ cap, f, compact }: Readonly<{ cap: CapUsage; f: CapForecast; compact?: boolean }>) {
  const tone = capStatus(cap, f).tone;
  const ghost = f.projected === null ? 0 : Math.min(f.projected / cap.limitBytes, 1);
  // The drawn meter is decorative; a visually hidden native <meter> carries the value for assistive tech.
  return (
    <>
      <meter className="sr-only" min={0} max={cap.limitBytes} value={Math.min(cap.used, cap.limitBytes)}
        aria-label="Cap used" aria-valuetext={`${bytes(cap.used)} of ${bytes(cap.limitBytes)}`} />
      <div className={`cap-meter ${compact ? "sm" : ""}`} style={{ "--c": TONE[tone] } as React.CSSProperties} aria-hidden>
        <div className="ghost" style={{ width: `${ghost * 100}%` }} />
        <div className="fill" style={{ width: `${Math.min(f.share, 1) * 100}%` }} />
        {cap.alerts.filter((a) => a < 100).map((a) => <span key={a} className="notch" style={{ left: `${a}%` }} />)}
        {f.elapsed > 0 && f.elapsed < 1 && <span className="today" style={{ left: `${f.elapsed * 100}%` }} title={`Today · ${percent(f.elapsed)} of the period has passed`} />}
      </div>
    </>
  );
}

function actionNote(cap: Cap) {
  if (cap.action === "block") return "Blocks Wi-Fi when used up";
  if (cap.action === "throttle") return `Slows to ${kbps(cap.throttleKbps)} when used up`;
  if (!cap.alerts.length) return "No alerts";
  const list = cap.alerts.map((a) => `${a}%`).join(", ");
  return `Alerts at ${list}`;
}

function forecastNote(cap: CapUsage, f: CapForecast) {
  if (f.share >= 1) return `${percent(f.share)} of the cap used`;
  if (f.projected !== null) return `At this pace, about ${bytes(f.projected)} by ${day(cap.end)}`;
  return "Too early in the period to forecast";
}

/** What happens now that the cap is used up. */
function enforcedNote(cap: CapUsage, scope: string) {
  const until = day(cap.end);
  if (cap.action === "block") {
    const from = cap.networkId === null ? "" : ` from ${scope}`;
    return `Overlook disconnects Wi-Fi${from} until ${until}.`;
  }
  const subject = cap.networkId === null ? "Your connection" : scope;
  return `${subject} is limited to ${kbps(cap.throttleKbps)} until ${until}.`;
}

function CapRow({ cap, networks, onEdit }: Readonly<{ cap: CapUsage; networks: Network[]; onEdit: () => void }>) {
  const f = capForecast(cap);
  const status = capStatus(cap, f);
  const scope = capScope(cap, networks);
  const network = networks.find((n) => n.id === cap.networkId);
  const left = cap.limitBytes - cap.used;
  return (
    <div className="cap-row">
      <div className="cap-head">
        <span className="icon-tile"><Icon name={network ? kindIcon(network.kind) : "grid"} size={16} /></span>
        <div style={{ minWidth: 0, flex: 1 }}>
          <div className="cap-name">{scope}</div>
          <div className="faint" style={{ fontSize: 12 }}>{PERIOD[cap.period]} · resets {day(cap.end)}</div>
        </div>
        <span className={`pill ${status.tone}`}>{status.label}</span>
      </div>
      <div className="cap-figures">
        <span className="num cap-used">{bytes(cap.used)}</span>
        <span className="muted num">of {bytes(cap.limitBytes)}</span>
        <span className="muted num" style={{ marginLeft: "auto" }}>{left >= 0 ? `${bytes(left)} left` : `${bytes(-left)} over`}</span>
      </div>
      <CapMeter cap={cap} f={f} />
      <div className="cap-foot">
        {forecastNote(cap, f)}
        {!(f.share >= 1 && cap.action !== "none") && ` · ${actionNote(cap)}`}
      </div>
      {f.share >= 1 && cap.action !== "none" && (
        <div className="cap-callout">
          <Icon name={cap.action === "block" ? "block" : "gauge"} size={16} style={{ flex: "none" }} />
          <span style={{ flex: 1 }}>{enforcedNote(cap, scope)}</span>
          <button className="btn" onClick={onEdit}>Edit cap</button>
        </div>
      )}
    </div>
  );
}

export function CapsCard({ status, network, onManage }: Readonly<{ status: Status; network: Network | null; onManage: () => void }>) {
  const caps = capsFor(useCaps() ?? [], network?.id ?? null);
  if (!caps.length) return null;
  return (
    <Card title="Data caps" subtitle={network ? "Caps that count usage on this network" : "Hatched: where you'll be at reset at this pace"}
      action={<button className="btn ghost" onClick={onManage}>Manage</button>}>
      <div className="cap-grid">
        {caps.map((c) => <CapRow key={c.id} cap={c} networks={status.networks} onEdit={onManage} />)}
      </div>
    </Card>
  );
}

/** One line for the tray popover. */
export function CapCompact({ cap, networks }: Readonly<{ cap: CapUsage; networks: Network[] }>) {
  const f = capForecast(cap);
  const status = capStatus(cap, f);
  return (
    <div>
      <div className="stat-label" style={{ justifyContent: "space-between", marginBottom: 6 }}>
        <span style={{ whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{PERIOD[cap.period]} cap · {capScope(cap, networks)}</span>
        <span className="num" style={{ color: "var(--text)", whiteSpace: "nowrap" }}>{bytes(cap.used)} <span className="muted">/ {bytes(cap.limitBytes)}</span></span>
      </div>
      <CapMeter cap={cap} f={f} compact />
      <div className="faint" style={{ fontSize: 11.5, marginTop: 6, color: status.tone === "ok" ? undefined : TONE[status.tone] }}>
        {status.tone === "ok" ? `On track · resets ${day(cap.end)}` : `${status.label} · resets ${day(cap.end)}`}
      </div>
    </div>
  );
}

export function CapsPane({ status }: Readonly<{ status: Status }>) {
  const caps = useCaps() ?? [];
  const [editing, setEditing] = useState<Cap | null>(null);
  if (editing) return <CapForm status={status} cap={editing} onDone={() => setEditing(null)} />;
  const add = () => setEditing({ id: 0, networkId: status.live.primary, period: "month", reset: 1, limitBytes: 50e9, alerts: [80, 100], action: "none", throttleKbps: 1000 });
  return (
    <>
      <h3>Caps & alerts</h3>
      <div className="cap-list">
        {caps.map((c) => {
          const f = capForecast(c);
          return (
            <button key={c.id} className="cap-item" onClick={() => setEditing(c)}>
              <span style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
                <span className="cap-name" style={{ flex: 1 }}>{capScope(c, status.networks)}</span>
                <span className="num muted" style={{ fontSize: 12 }}>{bytes(c.used)} / {bytes(c.limitBytes)} {PERIOD[c.period].toLowerCase()}</span>
              </span>
              <CapMeter cap={c} f={f} compact />
              <span className="faint" style={{ fontSize: 11.5, display: "flex", justifyContent: "space-between" }}>
                {actionNote(c)}<span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}><Icon name="edit" size={12} />Edit</span>
              </span>
            </button>
          );
        })}
        <button className="cap-item add" onClick={add}>
          <Icon name="plus" size={15} />
          <span>
            <span style={{ display: "block", fontWeight: 550 }}>Add a cap</span>
            <span className="faint" style={{ fontSize: 11.5 }}>{caps.length ? "Daily, weekly or monthly; for one network or all of them." : "Get alerts before a plan or hotspot runs out of data."}</span>
          </span>
        </button>
      </div>
      <Setting title="Unusual upload alerts" desc="Notifies you when an hour's upload is far above normal for that network, such as an unexpected backup or sync.">
        <Toggle label="Unusual upload alerts" checked={status.uploadAlerts} onChange={(v) => api.setUploadAlerts(v)} />
      </Setting>
    </>
  );
}

/** Typical usage per period on a network, from up to 90 days of tracked history. */
function useTypical(networkId: number | null, period: Cap["period"]) {
  return useLive(async () => {
    const now = Date.now();
    const { rows, coverage } = await api.usage(networkId, hourOf(now) - 90 * 24, hourOf(now));
    const days = coverage.reduce((s, [, sec]) => s + sec, 0) / 86400;
    if (days < 3) return null;
    return (rows.reduce((s, r) => s + r.rx + r.tx, 0) / days) * PERIOD_DAYS[period];
  }, [networkId, period]);
}

function CapForm({ status, cap, onDone }: Readonly<{ status: Status; cap: Cap; onDone: () => void }>) {
  const [draft, setDraft] = useState(cap);
  const [unit, setUnit] = useState(cap.limitBytes < 1e9 ? 1e6 : 1e9);
  const [amount, setAmount] = useState(String(cap.limitBytes / unit));
  const [custom, setCustom] = useState("");
  const [confirmDelete, setConfirmDelete] = useState(false);
  // The form replaces the list (and the button that opened it), so focus would otherwise drop to the page.
  const limitRef = useRef<HTMLInputElement>(null);
  useEffect(() => limitRef.current?.focus(), []);
  const typical = useTypical(draft.networkId, draft.period);
  const set = (patch: Partial<Cap>) => setDraft({ ...draft, ...patch });
  const limit = Math.round(Number(amount) * unit);
  const valid = limit > 0;
  const toggleAlert = (a: number) => set({ alerts: draft.alerts.includes(a) ? draft.alerts.filter((x) => x !== a) : [...draft.alerts, a].sort((x, y) => x - y) });
  const addCustom = () => {
    const n = Math.round(Number(custom.replace("%", "")));
    if (n > 0 && n <= 1000 && !draft.alerts.includes(n)) toggleAlert(n);
    setCustom("");
  };
  const save = () => {
    if (!valid) return;
    api.saveCap({ ...draft, limitBytes: limit });
    onDone();
  };
  const applyTypical = (bytesValue: number) => {
    const u = bytesValue >= 1e9 ? 1e9 : 1e6;
    setUnit(u);
    setAmount(String(Math.ceil(bytesValue / u / (u === 1e6 ? 100 : 1)) * (u === 1e6 ? 100 : 1)));
  };
  const weekday = (d: number) => fmtDate(new Date(2026, 0, 4 + d), { weekday: "long" });
  const scope = draft.networkId === null ? "all networks" : displayName(status.networks.find((n) => n.id === draft.networkId)!);
  const choices: { id: Cap["action"]; icon: IconName; title: string; desc: string; disabled?: string }[] = [
    { id: "none", icon: "bell", title: "Alert only", desc: "Just notify me" },
    { id: "block", icon: "block", title: "Block", desc: status.platform === "linux" ? "Disconnect the network" : "Disconnect Wi-Fi" },
    { id: "throttle", icon: "gauge", title: "Throttle", desc: "Slow the connection", disabled: status.canThrottle ? undefined : "Not available on Windows" },
  ];
  const actionDetail = {
    none: "You'll get the alerts above and nothing else changes.",
    block: status.platform === "linux"
      ? "Disconnects until the cap resets. Reconnecting while it's used up disconnects again."
      : "Disconnects Wi-Fi until the cap resets; reconnecting while it's used up disconnects again. Ethernet can't be disconnected on this OS.",
    throttle: "Limits speed until the cap resets. Your OS asks for an administrator password when the limit starts and ends.",
  }[draft.action];

  return (
    <form className="cap-form" onSubmit={(e) => { e.preventDefault(); save(); }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <button type="button" className="btn ghost icon-only" aria-label="Back to caps" onClick={onDone}><Icon name="back" size={15} /></button>
        <h3>{cap.id ? "Edit cap" : "New cap"}</h3>
      </div>

      <section className="field-group">
        <Setting title="Network">
          <select className="text" style={{ width: 210 }} value={draft.networkId ?? ""} onChange={(e) => set({ networkId: e.target.value ? Number(e.target.value) : null })}>
            <option value="">All networks</option>
            {status.networks.map((n) => <option key={n.id} value={n.id}>{displayName(n)}</option>)}
          </select>
        </Setting>
        <Setting title="Period">
          <Segmented size="sm" label="Period" value={draft.period} options={(["day", "week", "month"] as const).map((p) => ({ value: p, label: PERIOD[p] }))}
            onChange={(period) => set({ period, reset: period === "week" ? weekStart() : 1 })} />
        </Setting>
        {draft.period !== "day" && (
          <Setting title="Resets on" desc={draft.period === "month" ? "Match your plan's billing day." : undefined}>
            <select className="text" style={{ width: 210 }} value={draft.reset} onChange={(e) => set({ reset: Number(e.target.value) })}>
              {draft.period === "week"
                ? [0, 1, 2, 3, 4, 5, 6].map((d) => <option key={d} value={d}>Every {weekday(d)}</option>)
                : Array.from({ length: 28 }, (_, i) => <option key={i} value={i + 1}>Day {i + 1} of the month</option>)}
            </select>
          </Setting>
        )}
      </section>

      <section className="field-group">
        <div className="field-label">Limit</div>
        <div className="limit-input">
          <input className="num" type="number" min="0" step="any" required ref={limitRef} value={amount} aria-label="Limit" onChange={(e) => setAmount(e.target.value)} />
          <select value={unit} aria-label="Unit" onChange={(e) => setUnit(Number(e.target.value))}>
            <option value={1e6}>MB</option>
            <option value={1e9}>GB</option>
          </select>
          <span className="muted">{PER[draft.period]} on {scope}</span>
        </div>
        {typical != null && typical > 0 && (
          <div className="hint">
            You typically use about <strong className="num">{bytes(typical)}</strong> {PER[draft.period]} here.
            <button type="button" className="link" onClick={() => applyTypical(typical)}>Use that</button>
          </div>
        )}
      </section>

      <section className="field-group">
        <div className="field-label">Alert me at</div>
        <div className="chips">
          {[...new Set([...PRESETS, ...draft.alerts])].sort((a, b) => a - b).map((a) => (
            <button type="button" key={a} className="chip" aria-pressed={draft.alerts.includes(a)} onClick={() => toggleAlert(a)}>
              <span className="num">{a}%</span>
              {valid && <span className="num chip-sub">{bytes((limit * a) / 100)}</span>}
            </button>
          ))}
          <input className="chip-input num" value={custom} placeholder="Other %" aria-label="Add another alert percentage" onChange={(e) => setCustom(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); addCustom(); } }} onBlur={addCustom} />
        </div>
      </section>

      <section className="field-group">
        <div className="field-label">When it's used up</div>
        <div className="choices" role="radiogroup" aria-label="When it's used up">
          {choices.map((c) => (
            <button type="button" key={c.id} role="radio" className="choice" aria-checked={draft.action === c.id} disabled={!!c.disabled && draft.action !== c.id}
              title={c.disabled} onClick={() => set({ action: c.id })}>
              <Icon name={c.icon} size={18} />
              <span className="choice-title">{c.title}</span>
              <span className="choice-desc">{c.disabled ?? c.desc}</span>
            </button>
          ))}
        </div>
        <div className="hint" style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span style={{ flex: 1 }}>{actionDetail}</span>
          {draft.action === "throttle" && (
            <select className="text" style={{ width: 110 }} value={draft.throttleKbps} aria-label="Throttle speed" onChange={(e) => set({ throttleKbps: Number(e.target.value) })}>
              {[...new Set([...SPEEDS, draft.throttleKbps])].map((k) => <option key={k} value={k}>{kbps(k)}</option>)}
            </select>
          )}
        </div>
      </section>

      <div className="cap-form-actions">
        {cap.id > 0 && (
          <button type="button" className="btn danger" style={{ marginRight: "auto" }}
            onClick={() => (confirmDelete ? (api.deleteCap(cap.id), onDone()) : setConfirmDelete(true))}>
            <Icon name="trash" size={13} />{confirmDelete ? "Confirm delete" : "Delete cap"}
          </button>
        )}
        <button type="button" className="btn" onClick={onDone}>Cancel</button>
        <button type="submit" className="btn primary" disabled={!valid}>Save cap</button>
      </div>
    </form>
  );
}
