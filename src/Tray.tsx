import { hourOf, startOf } from "./lib/analytics";
import { api, displayName, useLive, useReport, useStatus } from "./lib/api";
import { bytes, rate } from "./lib/format";
import { useAppearance } from "./lib/theme";
import { Icon, kindIcon } from "./components/Icon";
import { Bytes } from "./components/ui";
import { UsageChart } from "./components/UsageChart";

export default function Tray() {
  useAppearance();
  const status = useStatus();
  const id = status?.live.primary ?? null;
  const network = status?.networks.find((n) => n.id === id);
  const active = status?.live.active.find((a) => a.networkId === id);
  const report = useReport(id, "day", undefined, status?.firstCoveredHour ?? null);
  const today = useLive(async () => {
    if (id === null) return { rx: 0, tx: 0 };
    const { rows } = await api.usage(id, hourOf(startOf(new Date(), "day").getTime()), hourOf(Date.now()) + 1);
    return rows.reduce((s, r) => ({ rx: s.rx + r.rx, tx: s.tx + r.tx }), { rx: 0, tx: 0 });
  }, [id]);
  if (!status) return null;

  return (
    <div className="tray">
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <span className="brand-mark" style={{ width: 30, height: 30, borderRadius: 8 }}><Icon name={kindIcon(network?.kind)} size={15} /></span>
        <div style={{ minWidth: 0 }}>
          <div style={{ fontWeight: 600, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{network ? displayName(network) : "Not connected"}</div>
          <div className="muted" style={{ fontSize: 11.5, display: "flex", alignItems: "center", gap: 6 }}>
            <span className={`dot ${network ? "" : "off"}`} />
            {active ? `${active.kind === "wifi" ? "Wi-Fi" : "Ethernet"} · ${active.interface}` : "No usage is being recorded"}
          </div>
        </div>
      </div>

      {network && today && (
        <div>
          <div className="stat-label">Today on this network</div>
          <div style={{ display: "flex", alignItems: "flex-end", justifyContent: "space-between" }}>
            <Bytes value={today.rx + today.tx} className="hero-value" />
            <div className="num" style={{ fontSize: 11.5, textAlign: "right", lineHeight: 1.6 }}>
              <div><Icon name="down" size={11} style={{ color: "var(--download)", verticalAlign: -1 }} /> {bytes(today.rx)}</div>
              <div><Icon name="up" size={11} style={{ color: "var(--upload)", verticalAlign: -1 }} /> {bytes(today.tx)}</div>
            </div>
          </div>
        </div>
      )}

      {network && report && (
        <div>
          <div className="stat-label" style={{ justifyContent: "space-between", marginBottom: 6 }}>
            <span>Last 24 hours</span>
            <span className="num" style={{ color: "var(--text)" }}>{bytes(report.total)}</span>
          </div>
          <UsageChart report={report} mode="both" height={48} compact />
        </div>
      )}

      {status.live.rate && network && (
        <div className="stat-label" style={{ justifyContent: "space-between" }}>
          <span>Now</span>
          <span className="rate num">
            <span><Icon name="down" size={12} style={{ color: "var(--download)" }} />{rate(status.live.rate[0])}</span>
            <span><Icon name="up" size={12} style={{ color: "var(--upload)" }} />{rate(status.live.rate[1])}</span>
          </span>
        </div>
      )}

      <div style={{ display: "flex", gap: 6, borderTop: "1px solid var(--border)", paddingTop: 12 }}>
        <button className="btn primary" style={{ flex: 1, justifyContent: "center" }} onClick={() => api.showMain()}>Open Tally</button>
        <button className="btn icon-only" title="Quit Tally. Usage isn't recorded while it's closed." onClick={() => api.quit()}><Icon name="power" size={14} /></button>
      </div>
    </div>
  );
}
