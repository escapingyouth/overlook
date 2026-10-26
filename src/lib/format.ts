const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

export function scaled(bytes: number): [number, string] {
  let value = bytes;
  let i = 0;
  while (Math.abs(value) >= 1000 && i < UNITS.length - 1) {
    value /= 1000;
    i++;
  }
  return [value, UNITS[i]];
}

export function number(value: number): string {
  const m = Math.abs(value);
  const digits = m >= 100 || Number.isInteger(value) ? 0 : m >= 10 ? 1 : 2;
  return value.toLocaleString(undefined, { maximumFractionDigits: digits });
}

export const bytes = (b: number) => {
  const [v, u] = scaled(b);
  return `${number(v)} ${u}`;
};

export const rate = (bps: number) => `${bytes(bps)}/s`;

export function axisUnit(max: number): { divisor: number; unit: string } {
  const [, unit] = scaled(Math.max(max, 1));
  return { divisor: 1000 ** UNITS.indexOf(unit), unit };
}

export const percent = (f: number) =>
  f.toLocaleString(undefined, { style: "percent", maximumFractionDigits: Math.abs(f) < 0.1 ? 1 : 0 });

export function niceTicks(max: number, count = 4): number[] {
  if (max <= 0) return [0];
  const raw = max / count;
  const pow = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * pow).find((s) => s >= raw)!;
  const ticks = [];
  for (let v = 0; v <= max + step * 0.001; v += step) ticks.push(v);
  if (ticks[ticks.length - 1] < max) ticks.push(ticks[ticks.length - 1] + step);
  return ticks;
}

const relative = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
export function ago(epochSeconds: number): string {
  const diff = epochSeconds - Date.now() / 1000;
  const steps: [number, Intl.RelativeTimeFormatUnit][] = [[60, "second"], [3600, "minute"], [86400, "hour"], [604800, "day"], [2629800, "week"], [31557600, "month"], [Infinity, "year"]];
  let unitSeconds = 1;
  for (const [limit, unit] of steps) {
    if (Math.abs(diff) < limit) return relative.format(Math.round(diff / unitSeconds), unit);
    unitSeconds = limit;
  }
  return "";
}

export const fmtDate = (d: Date, o: Intl.DateTimeFormatOptions) => d.toLocaleString(undefined, o);
