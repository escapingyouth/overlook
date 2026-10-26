import type { AppsReport } from "./api";

export interface AppRow {
  key: string;
  name: string;
  icon: string | null;
  system: boolean;
  rx: number;
  tx: number;
  total: number;
  share: number;
}

export function topApps(r: AppsReport, limit = 10): { rows: AppRow[]; hidden: AppRow[]; total: number; count: number } {
  const sysApp = r.apps.find((a) => a.key === "system");
  const others = r.apps.filter((a) => a.key !== "system");
  const sumRx = others.reduce((s, a) => s + a.rx, 0);
  const sumTx = others.reduce((s, a) => s + a.tx, 0);
  const sysRx = (sysApp?.rx ?? 0) + Math.max(0, r.coveredRx - sumRx);
  const sysTx = (sysApp?.tx ?? 0) + Math.max(0, r.coveredTx - sumTx);
  const all = others.map((a) => ({ key: a.key, name: a.name, icon: a.icon, system: false, rx: a.rx, tx: a.tx }));
  if (sysRx + sysTx > 0) all.push({ key: "system", name: sysApp?.name ?? "System", icon: sysApp?.icon ?? null, system: true, rx: sysRx, tx: sysTx });
  const sum = all.reduce((s, a) => s + a.rx + a.tx, 0);
  const total = Math.max(r.coveredRx + r.coveredTx, sum);
  const sorted = all
    .map((a) => ({ ...a, total: a.rx + a.tx, share: total > 0 ? (a.rx + a.tx) / total : 0 }))
    .sort((a, b) => b.total - a.total);
  return { rows: sorted.slice(0, limit), hidden: sorted.slice(limit), total, count: sorted.length };
}
