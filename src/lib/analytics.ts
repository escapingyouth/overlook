
export type RangeId = "day" | "week" | "twoWeeks" | "month" | "threeMonths" | "sixMonths" | "year";
export type Granularity = "hour" | "day" | "week" | "month";

export interface HourRow {
  hour: number;
  rx: number;
  tx: number;
  estimatedRx: number;
  estimatedTx: number;
}

export interface RangeInfo {
  id: RangeId;
  short: string;
  title: string;
  previous: string;
  granularities: Granularity[];
}

export const RANGES: RangeInfo[] = [
  { id: "day", short: "24H", title: "Last 24 hours", previous: "previous 24 hours", granularities: ["hour"] },
  { id: "week", short: "1W", title: "Last 1 week", previous: "previous week", granularities: ["day", "hour"] },
  { id: "twoWeeks", short: "2W", title: "Last 2 weeks", previous: "previous 2 weeks", granularities: ["day", "hour"] },
  { id: "month", short: "1M", title: "Last 1 month", previous: "previous month", granularities: ["day", "week"] },
  { id: "threeMonths", short: "3M", title: "Last 3 months", previous: "previous 3 months", granularities: ["week", "day"] },
  { id: "sixMonths", short: "6M", title: "Last 6 months", previous: "previous 6 months", granularities: ["week", "month"] },
  { id: "year", short: "1Y", title: "Last 1 year", previous: "previous year", granularities: ["month", "week"] },
];

export const rangeInfo = (id: RangeId) => RANGES.find((r) => r.id === id)!;

export const GRANULARITY_LABEL: Record<Granularity, string> = { hour: "Hourly", day: "Daily", week: "Weekly", month: "Monthly" };

export const HOUR = 3_600_000;
export const hourOf = (ms: number) => Math.floor(ms / HOUR);
export const dateOfHour = (hour: number) => new Date(hour * HOUR);

export function weekStart(): number {
  try {
    const info = (new Intl.Locale(navigator.language) as any).getWeekInfo?.() ?? (new Intl.Locale(navigator.language) as any).weekInfo;
    if (info?.firstDay) return info.firstDay % 7;
  } catch {
  }
  return navigator.language === "en-US" ? 0 : 1;
}

export function startOf(date: Date, unit: Granularity, firstDay = weekStart()): Date {
  const d = new Date(date);
  if (unit === "hour") {
    d.setMinutes(0, 0, 0);
    return d;
  }
  d.setHours(0, 0, 0, 0);
  if (unit === "week") d.setDate(d.getDate() - ((d.getDay() - firstDay + 7) % 7));
  if (unit === "month") d.setDate(1);
  return d;
}

export function add(date: Date, unit: Granularity, n: number): Date {
  const d = new Date(date);
  if (unit === "hour") d.setTime(d.getTime() + n * HOUR);
  else if (unit === "day") d.setDate(d.getDate() + n);
  else if (unit === "week") d.setDate(d.getDate() + 7 * n);
  else d.setMonth(d.getMonth() + n);
  return d;
}

function monthsBack(date: Date, n: number): Date {
  const d = new Date(date.getFullYear(), date.getMonth() - n, 1);
  const lastDay = new Date(d.getFullYear(), d.getMonth() + 1, 0).getDate();
  d.setDate(Math.min(date.getDate(), lastDay));
  return d;
}

export function rangeInterval(range: RangeId, now: Date): { start: Date; end: Date } {
  const today = startOf(now, "day");
  let start: Date;
  switch (range) {
    case "day":
      start = new Date(startOf(now, "hour").getTime() - 23 * HOUR);
      break;
    case "week":
      start = add(today, "day", -6);
      break;
    case "twoWeeks":
      start = add(today, "day", -13);
      break;
    default: {
      const months = { month: 1, threeMonths: 3, sixMonths: 6, year: 12 }[range];
      start = add(monthsBack(today, months), "day", 1);
    }
  }
  return { start, end: now };
}

export interface Bucket {
  start: Date;
  end: Date;
  rx: number | null;
  tx: number | null;
  estimated: number;
}

export const bucketTotal = (b: Bucket) => (b.rx === null ? null : b.rx + (b.tx ?? 0));

export interface DayUsage {
  day: Date;
  total: number;
}

export interface Report {
  range: RangeId;
  granularity: Granularity;
  start: Date;
  end: Date;
  buckets: Bucket[];
  rx: number;
  tx: number;
  total: number;
  estimated: number;
  trackedDays: number;
  dailyAverage: number | null;
  weeklyAverage: number | null;
  peakDay: DayUsage | null;
  lowestDay: DayUsage | null;
  previousTotal: number | null;
  previousRx: number | null;
  previousTx: number | null;
  days: { day: Date; rx: number; tx: number; estimated: number; coveredSeconds: number }[];
  change: number | null;
  hourOfDay: { rx: number; tx: number }[];
  weekday: { rx: number; tx: number }[];
}

export function queryStartHour(range: RangeId, now: Date): number {
  const { start, end } = rangeInterval(range, now);
  return hourOf(start.getTime() - (end.getTime() - start.getTime()));
}

export function buildReport(opts: {
  range: RangeId;
  granularity?: Granularity;
  rows: HourRow[];
  coverage: [number, number][];
  historyStartHour: number | null;
  now: Date;
  firstDay?: number;
}): Report {
  const { range, rows, now } = opts;
  const firstDay = opts.firstDay ?? weekStart();
  const info = rangeInfo(range);
  const granularity = opts.granularity && info.granularities.includes(opts.granularity) ? opts.granularity : info.granularities[0];
  const { start, end } = rangeInterval(range, now);
  const startHour = hourOf(start.getTime());
  const current = rows.filter((r) => r.hour >= startHour);
  const covered = opts.coverage.filter(([h, s]) => h >= startHour && s > 0).map(([h]) => h);

  const buckets: Bucket[] = [];
  for (let cursor = start; cursor < end; ) {
    const next = add(startOf(cursor, granularity, firstDay), granularity, 1);
    const bEnd = next < end ? next : end;
    buckets.push({ start: cursor, end: bEnd, rx: null, tx: null, estimated: 0 });
    cursor = bEnd;
  }
  const index = (ms: number) => {
    let lo = 0;
    let hi = buckets.length - 1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if (ms < buckets[mid].start.getTime()) hi = mid - 1;
      else if (ms >= buckets[mid].end.getTime()) lo = mid + 1;
      else return mid;
    }
    return -1;
  };
  for (const h of covered) {
    const b = buckets[index(h * HOUR)];
    if (b && b.rx === null) {
      b.rx = 0;
      b.tx = 0;
    }
  }
  for (const r of current) {
    const b = buckets[index(r.hour * HOUR)];
    if (!b) continue;
    b.rx = (b.rx ?? 0) + r.rx;
    b.tx = (b.tx ?? 0) + r.tx;
    b.estimated += r.estimatedRx + r.estimatedTx;
  }

  const days = new Map<number, number>();
  const dayKey = (hour: number) => startOf(dateOfHour(hour), "day").getTime();
  for (const h of covered) if (!days.has(dayKey(h))) days.set(dayKey(h), 0);
  const hourOfDay = Array.from({ length: 24 }, () => ({ rx: 0, tx: 0 }));
  const weekday = Array.from({ length: 7 }, () => ({ rx: 0, tx: 0 }));
  for (const r of current) {
    const date = dateOfHour(r.hour);
    days.set(dayKey(r.hour), (days.get(dayKey(r.hour)) ?? 0) + r.rx + r.tx);
    hourOfDay[date.getHours()].rx += r.rx;
    hourOfDay[date.getHours()].tx += r.tx;
    weekday[date.getDay()].rx += r.rx;
    weekday[date.getDay()].tx += r.tx;
  }
  const dayDetail = new Map<number, { rx: number; tx: number; estimated: number; coveredSeconds: number }>();
  for (let d = startOf(start, "day"); d < end; d = add(d, "day", 1)) dayDetail.set(d.getTime(), { rx: 0, tx: 0, estimated: 0, coveredSeconds: 0 });
  for (const [h, sec] of opts.coverage) {
    const e = h >= startHour ? dayDetail.get(dayKey(h)) : undefined;
    if (e) e.coveredSeconds += sec;
  }
  for (const r of current) {
    const e = dayDetail.get(dayKey(r.hour));
    if (!e) continue;
    e.rx += r.rx;
    e.tx += r.tx;
    e.estimated += r.estimatedRx + r.estimatedTx;
    e.coveredSeconds = Math.max(e.coveredSeconds, 1);
  }
  const trackedDays = days.size;
  const weekdayCounts = Array(7).fill(0);
  for (const d of days.keys()) weekdayCounts[new Date(d).getDay()]++;
  const perDay = Math.max(trackedDays, 1);
  hourOfDay.forEach((v) => {
    v.rx /= perDay;
    v.tx /= perDay;
  });
  weekday.forEach((v, i) => {
    const n = Math.max(weekdayCounts[i], 1);
    v.rx /= n;
    v.tx /= n;
  });

  const rx = current.reduce((s, r) => s + r.rx, 0);
  const tx = current.reduce((s, r) => s + r.tx, 0);
  const total = rx + tx;
  const estimated = current.reduce((s, r) => s + r.estimatedRx + r.estimatedTx, 0);
  const dayList = [...days].map(([d, t]) => ({ day: new Date(d), total: t }));
  const today = startOf(now, "day").getTime();
  const peakDay = dayList.filter((d) => d.total > 0).reduce<DayUsage | null>((m, d) => (!m || d.total > m.total ? d : m), null);
  const complete = dayList.filter((d) => d.day.getTime() < today && d.total > 0);
  const lowestDay = complete.length >= 3 ? complete.reduce((m, d) => (d.total < m.total ? d : m)) : null;

  const duration = end.getTime() - start.getTime();
  const prevStartHour = hourOf(start.getTime() - duration);
  let previousTotal: number | null = null;
  let previousRx: number | null = null;
  let previousTx: number | null = null;
  let change: number | null = null;
  if (opts.historyStartHour !== null && opts.historyStartHour <= prevStartHour + 1) {
    const prev = rows.filter((r) => r.hour >= prevStartHour && r.hour < startHour);
    previousRx = prev.reduce((s, r) => s + r.rx, 0);
    previousTx = prev.reduce((s, r) => s + r.tx, 0);
    previousTotal = previousRx + previousTx;
    if (previousTotal > 0) change = (total - previousTotal) / previousTotal;
  }

  return {
    range,
    granularity,
    start,
    end,
    buckets,
    rx,
    tx,
    total,
    estimated,
    trackedDays,
    dailyAverage: trackedDays > 0 ? total / trackedDays : null,
    weeklyAverage: range !== "day" && range !== "week" && trackedDays >= 7 ? (total * 7) / trackedDays : null,
    peakDay,
    lowestDay,
    previousTotal,
    previousRx,
    previousTx,
    days: [...dayDetail].map(([d, v]) => ({ day: new Date(d), ...v })),
    change,
    hourOfDay,
    weekday,
  };
}

export interface CapForecast {
  /** Share of the cap used so far; can exceed 1. */
  share: number;
  /** Share of the period that has passed. */
  elapsed: number;
  /** Usage by the reset at the current pace, or null when it's too early to say. */
  projected: number | null;
  /** When the cap runs out at the current pace (epoch seconds), if before the reset. */
  runsOutAt: number | null;
}

export function capForecast(c: { used: number; limitBytes: number; start: number; end: number }, now = Date.now() / 1000): CapForecast {
  const elapsed = Math.min(Math.max((now - c.start) / Math.max(c.end - c.start, 1), 0), 1);
  const share = c.used / Math.max(c.limitBytes, 1);
  // Extrapolating from the first few hours of a month is noise.
  const projected = elapsed >= 0.1 && c.used > 0 ? c.used / elapsed : null;
  const runsOutAt = projected !== null && projected > c.limitBytes && share < 1 ? c.start + (c.limitBytes / c.used) * (now - c.start) : null;
  return { share, elapsed, projected, runsOutAt };
}
