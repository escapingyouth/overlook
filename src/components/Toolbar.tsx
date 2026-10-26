import { useEffect, useRef, useState } from "react";
import { hourOf, queryStartHour, RANGES, RangeId, rangeInfo, rangeInterval } from "../lib/analytics";
import { api, displayName, Network } from "../lib/api";
import { Icon } from "./Icon";

export function RangeMenu({ value, onChange }: { value: RangeId; onChange: (r: RangeId) => void }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
      const n = Number(e.key);
      if ((e.metaKey || e.ctrlKey) && n >= 1 && n <= RANGES.length) {
        e.preventDefault();
        onChange(RANGES[n - 1].id);
      }
    };
    const onDown = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false);
    window.addEventListener("keydown", onKey);
    window.addEventListener("mousedown", onDown);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("mousedown", onDown);
    };
  }, [onChange]);
  const mod = navigator.platform.startsWith("Mac") ? "⌘" : "Ctrl+";
  return (
    <div className="dropdown" ref={ref}>
      <button className="btn" aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen(!open)}>
        <Icon name="calendar" size={14} />
        {rangeInfo(value).title}
        <Icon name="chevron" size={13} className="muted" />
      </button>
      {open && (
        <div className="menu" role="menu">
          {RANGES.map((r, i) => (
            <button key={r.id} role="menuitemradio" aria-checked={r.id === value} onClick={() => { onChange(r.id); setOpen(false); }}>
              <span style={{ width: 14, display: "inline-flex" }}>{r.id === value && <Icon name="check" size={14} />}</span>
              {r.title}
              <span className="k">{mod}{i + 1}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

const csvField = (v: string | number) => (typeof v === "string" && /[",\n]/.test(v) ? `"${v.replace(/"/g, '""')}"` : String(v));

export function ExportButton({ network, range }: { network: Network | null; range: RangeId }) {
  const [busy, setBusy] = useState(false);
  const run = async () => {
    setBusy(true);
    try {
      const now = new Date();
      const { start } = rangeInterval(range, now);
      const startHour = hourOf(start.getTime());
      const { rows } = await api.usage(network?.id ?? null, Math.max(startHour, queryStartHour(range, now)), hourOf(now.getTime()) + 1);
      const name = network ? displayName(network) : "All networks";
      const lines = [["hour_start_local", "hour_start_utc", "network", "download_bytes", "upload_bytes", "total_bytes", "estimated_bytes"].join(",")];
      for (const r of rows.filter((r) => r.hour >= startHour)) {
        const d = new Date(r.hour * 3_600_000);
        const local = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")} ${String(d.getHours()).padStart(2, "0")}:00`;
        lines.push([local, d.toISOString(), name, r.rx, r.tx, r.rx + r.tx, r.estimatedRx + r.estimatedTx].map(csvField).join(","));
      }
      const safe = name.replace(/[^\p{L}\p{N} ._-]/gu, "").trim() || "Network";
      await api.saveExport(`Overlook ${safe} ${rangeInfo(range).short} ${now.toISOString().slice(0, 10)}.csv`, lines.join("\n") + "\n");
    } catch (e) {
      alert(`Export failed: ${e}`);
    } finally {
      setBusy(false);
    }
  };
  return (
    <button className="btn accent" onClick={run} disabled={busy} title="Hourly download/upload for the selected network and range, saved to Downloads">
      <Icon name="export" size={14} />
      {busy ? "Exporting…" : "Export CSV"}
    </button>
  );
}
