process.env.TZ = "America/New_York";

import { describe, expect, it } from "vitest";
import { buildReport, bucketTotal, capForecast, HourRow, hourOf, rangeInterval, RangeId, startOf } from "./analytics";
import { bytes, niceTicks } from "./format";

const d = (s: string) => new Date(s);
const row = (date: Date, rx: number, tx = 0, estimatedRx = 0): HourRow => ({ hour: hourOf(date.getTime()), rx, tx, estimatedRx, estimatedTx: 0 });
const report = (range: RangeId, now: Date, rows: HourRow[] = [], coverage: [number, number][] = [], historyStartHour: number | null = null, granularity?: any) =>
  buildReport({ range, now, rows, coverage, historyStartHour, granularity, firstDay: 0 });

describe("time windows", () => {
  const now = d("2026-10-25T14:37:00");
  it("starts each range where documented", () => {
    const start = (r: RangeId) => rangeInterval(r, now).start.getTime();
    expect(start("day")).toBe(d("2026-10-24T15:00:00").getTime());
    expect(start("week")).toBe(d("2026-10-19T00:00:00").getTime());
    expect(start("twoWeeks")).toBe(d("2026-10-12T00:00:00").getTime());
    expect(start("month")).toBe(d("2026-09-26T00:00:00").getTime());
    expect(start("threeMonths")).toBe(d("2026-07-26T00:00:00").getTime());
    expect(start("sixMonths")).toBe(d("2026-04-26T00:00:00").getTime());
    expect(start("year")).toBe(d("2025-10-26T00:00:00").getTime());
  });
  it("clamps month ends", () => {
    expect(rangeInterval("month", d("2026-03-31T10:00:00")).start.getTime()).toBe(d("2026-03-01T00:00:00").getTime());
  });
  it("produces the expected bucket counts", () => {
    const n = (r: RangeId, g?: string) => report(r, now, [], [], null, g).buckets.length;
    expect(n("day")).toBe(24);
    expect(n("week")).toBe(7);
    expect(n("twoWeeks")).toBe(14);
    expect(n("month")).toBe(30);
    expect(n("year")).toBe(13);
    expect(n("year", "week")).toBe(53);
    expect(n("threeMonths", "day")).toBe(92);
  });
  it("keeps 24 real hours across a DST change", () => {
    const now = d("2026-03-08T12:30:00");
    const r = report("day", now);
    expect(r.buckets.length).toBe(24);
    expect(now.getTime() - r.start.getTime()).toBe(23.5 * 3_600_000);
  });
  it("aligns weeks to the locale's first day", () => {
    expect(startOf(d("2026-10-22T09:00:00"), "week", 1).getDate()).toBe(19);
    expect(startOf(d("2026-10-22T09:00:00"), "week", 0).getDate()).toBe(18);
  });
});

describe("report", () => {
  const now = d("2026-10-25T12:30:00");
  const at = (daysAgo: number, h: number) => d(`2026-10-${String(25 - daysAgo).padStart(2, "0")}T${String(h).padStart(2, "0")}:00:00`);

  it("totals, untracked buckets as unknown, tracked zero as zero", () => {
    const rows = [row(at(0, 9), 1000, 200, 100), row(at(2, 9), 3000)];
    const coverage: [number, number][] = [[hourOf(at(0, 9).getTime()), 3600], [hourOf(at(1, 9).getTime()), 3600], [hourOf(at(2, 9).getTime()), 3600]];
    const r = report("week", now, rows, coverage);
    expect([r.rx, r.tx, r.estimated, r.trackedDays, r.dailyAverage]).toEqual([4000, 200, 100, 3, 1400]);
    expect(bucketTotal(r.buckets[0])).toBeNull();
    expect(bucketTotal(r.buckets[5])).toBe(0);
    expect(bucketTotal(r.buckets[6])).toBe(1200);
    expect(r.peakDay?.total).toBe(3000);
    expect(r.lowestDay).toBeNull();
    expect(r.change).toBeNull();
  });
  it("doesn't leak the previous window into the current one", () => {
    const r = report("week", now, [row(at(7, 23), 500), row(at(6, 0), 7)]);
    expect(r.total).toBe(7);
  });
  it("compares with the previous period only when history covers it", () => {
    const rows = [row(at(8, 10), 1000), row(at(1, 10), 1500)];
    const early = hourOf(at(20, 0).getTime());
    expect(report("week", now, rows, [], early).change).toBeCloseTo(0.5);
    expect(report("week", now, rows, [], early).previousTotal).toBe(1000);
    expect(report("week", now, rows, [], hourOf(at(8, 0).getTime())).change).toBeNull();
    expect(report("week", now, rows, [], early).previousRx).toBe(1000);
  });
  it("lists every calendar day with coverage", () => {
    const r = report("week", now, [row(at(1, 9), 500, 50, 100)], [[hourOf(at(1, 9).getTime()), 3600]]);
    expect(r.days.length).toBe(7);
    expect(r.days[5]).toMatchObject({ rx: 500, tx: 50, estimated: 100, coveredSeconds: 3600 });
    expect(r.days[0].coveredSeconds).toBe(0);
  });
  it("lowest day excludes today and zero days", () => {
    const rows = [0, 1, 2, 3, 4].map((n) => row(at(n, 1), 100 * (n + 1)));
    const r = report("week", now, rows, [[hourOf(at(5, 1).getTime()), 3600]]);
    expect(r.lowestDay?.total).toBe(200);
    expect(r.peakDay?.total).toBe(500);
  });
  it("weekday pattern averages per tracked occurrence", () => {
    const r = report("twoWeeks", now, [row(at(7, 1), 100), row(at(0, 1), 300)]);
    expect(r.weekday[now.getDay()].rx).toBe(200);
  });
});

describe("format", () => {
  it("formats bytes with three significant figures", () => {
    expect(bytes(999)).toBe("999 B");
    expect(bytes(1_234_000)).toBe("1.23 MB");
    expect(bytes(45_600_000_000)).toBe("45.6 GB");
  });
  it("makes nice ticks", () => {
    expect(niceTicks(3.7)).toEqual([0, 1, 2, 3, 4]);
    expect(niceTicks(860, 4)).toEqual([0, 250, 500, 750, 1000]);
  });
});

describe("cap forecast", () => {
  const cap = { limitBytes: 100, start: 0, end: 1000 };
  it("projects the current pace to the reset", () => {
    const f = capForecast({ ...cap, used: 30 }, 500);
    expect(f).toEqual({ share: 0.3, elapsed: 0.5, projected: 60, runsOutAt: null });
  });
  it("predicts when a fast pace runs out", () => {
    expect(capForecast({ ...cap, used: 80 }, 400).runsOutAt).toBe(500);
  });
  it("stays quiet early in the period and once the cap is used up", () => {
    expect(capForecast({ ...cap, used: 5 }, 50).projected).toBeNull();
    expect(capForecast({ ...cap, used: 120 }, 900).runsOutAt).toBeNull();
  });
});
