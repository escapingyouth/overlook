import { describe, expect, it } from "vitest";
import type { AppsReport, AppTotal } from "./api";
import { topApps } from "./apps";

const app = (key: string, rx: number, tx: number, id = 1): AppTotal => ({ id, key, name: key, icon: null, rx, tx });
const rep = (apps: AppTotal[], coveredRx: number, coveredTx: number): AppsReport => ({ apps, coveredRx, coveredTx, sinceHour: null });

describe("topApps", () => {
  it("computes System as the unattributed remainder", () => {
    const r = topApps(rep([app("A", 600, 50)], 1000, 100));
    const sys = r.rows.find((x) => x.system)!;
    expect([sys.rx, sys.tx]).toEqual([400, 50]);
    expect(r.rows.find((x) => x.key === "A")!.share).toBeCloseTo(650 / 1100);
    expect(r.total).toBe(1100);
    expect(r.count).toBe(2);
  });

  it("merges the system app into the remainder", () => {
    const r = topApps(rep([app("system", 100, 10), app("A", 600, 50)], 1000, 100));
    const sys = r.rows.filter((x) => x.system);
    expect(sys).toHaveLength(1);
    expect([sys[0].rx, sys[0].tx]).toEqual([500, 60]);
  });

  it("clamps system and rescales shares when apps exceed total", () => {
    const r = topApps(rep([app("A", 150, 0)], 100, 0));
    expect(r.rows.some((x) => x.system)).toBe(false);
    expect(r.rows[0].share).toBe(1);
  });

  it("splits top 10 from hidden", () => {
    const apps = Array.from({ length: 13 }, (_, i) => app(`a${i}`, 100 - i, 0, i + 1));
    const r = topApps(rep(apps, apps.reduce((n, a) => n + a.rx, 0), 0), 10);
    expect(r.rows).toHaveLength(10);
    expect(r.hidden).toHaveLength(3);
  });
});
