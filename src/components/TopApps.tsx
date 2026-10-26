import { useState } from "react";
import { queryStartHour, RangeId, rangeInfo } from "../lib/analytics";
import { api, Status, useApps } from "../lib/api";
import { AppRow, topApps } from "../lib/apps";
import { bytes, fmtDate, percent } from "../lib/format";
import { Icon } from "./Icon";

function AppIcon({ app }: { app: AppRow }) {
  if (app.icon) return <img className="app-icon" src={app.icon} alt="" />;
  return <span className="icon-tile"><Icon name={app.system ? "computer" : "app"} size={14} /></span>;
}

export function TopApps({ network, range, status, onEnable }: { network: number | null; range: RangeId; status: Status; onEnable: () => void }) {
  const { enabled, error } = status.live.perApp;
  const report = useApps(network, range);
  const [all, setAll] = useState(false);
  const info = rangeInfo(range);

  let sub = info.title;
  let body: React.ReactNode;
  let note: string | null = null;

  if (!enabled) {
    body = (
      <div className="panel-empty">
        <span className="muted">See which apps use your data. Overlook only counts bytes per app, never what they send.</span>
        <button className="btn" onClick={onEnable}>Turn on per-app tracking</button>
      </div>
    );
  } else if (error) {
    body = (
      <div className="panel-empty">
        <span className="pill warn"><Icon name="alert" size={13} />{error}</span>
        <button className="btn" onClick={() => api.setPerApp(true).catch(() => {})}>Retry</button>
      </div>
    );
  } else if (!report) {
    return null;
  } else {
    const { rows, hidden, total, count } = topApps(report);
    if (count === 0) {
      body = <div className="panel-empty muted">No app traffic recorded in this range yet.</div>;
    } else {
      sub = `${bytes(total)} across ${count} apps · ${info.title}`;
      const shown = all ? [...rows, ...hidden] : rows;
      if (report.sinceHour !== null && report.sinceHour > queryStartHour(range, new Date()))
        note = `Per-app data since ${fmtDate(new Date(report.sinceHour * 3_600_000), { month: "short", day: "numeric" })}`;
      body = (
        <div style={{ overflowX: "auto" }}>
          <table className="table">
            <thead>
              <tr><th>#</th><th>App</th><th className="r">Downloaded</th><th className="r">Uploaded</th><th className="r">Total</th><th className="r">Share</th></tr>
            </thead>
            <tbody>
              {shown.map((a, i) => (
                <tr key={a.key}>
                  <td className="faint num">{i + 1}</td>
                  <td><span className="name-cell"><AppIcon app={a} />{a.name}</span></td>
                  <td className="r num">{bytes(a.rx)}</td>
                  <td className="r num">{bytes(a.tx)}</td>
                  <td className="r num" style={{ fontWeight: 500 }}>{bytes(a.total)}</td>
                  <td className="r num">
                    <span className="share">
                      {percent(a.share)}
                      <span className="bar"><i style={{ width: `${Math.max(a.share * 100, a.share > 0 ? 2 : 0)}%` }} /></span>
                    </span>
                  </td>
                </tr>
              ))}
              {!all && hidden.length > 0 && (
                <tr>
                  <td />
                  <td colSpan={5}>
                    <button className="more" aria-expanded={all} aria-label={`Show ${hidden.length} more apps`} onClick={() => setAll(true)}>
                      <span className="stack">{hidden.slice(0, 3).map((a) => <AppIcon key={a.key} app={a} />)}</span>
                      + {hidden.length} more
                    </button>
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      );
    }
  }

  return (
    <section className="panel">
      <div className="panel-head"><h2>Top apps</h2><span className="faint" style={{ fontSize: 12 }}>{sub}</span></div>
      {body}
      {note && <div className="panel-note">{note}</div>}
    </section>
  );
}
