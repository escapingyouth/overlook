import { emit } from "@tauri-apps/api/event";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type { HourRow } from "../lib/analytics";

const HOUR = 3_600_000;
const now = Date.now();
const nowHour = Math.floor(now / HOUR);
let seed = 7;
const rand = () => ((seed = (seed * 16807) % 2147483647) / 2147483647);

const networks = [
  { id: 1, key: "wifi:Lakeview 5G", name: "Lakeview 5G", customName: null, kind: "wifi", ssid: "Lakeview 5G", routerMac: "a4:2b:b0:11:22:33", firstSeen: now / 1000 - 400 * 86400, lastSeen: now / 1000, lastInterface: "en0" },
  { id: 2, key: "router:3c:22:fb:44:55:66", name: "Ethernet Network · 55:66", customName: "Studio", kind: "ethernet", ssid: null, routerMac: "3c:22:fb:44:55:66", firstSeen: now / 1000 - 400 * 86400, lastSeen: now / 1000 - 15 * 3600, lastInterface: "en5" },
  { id: 3, key: "wifi:Sam’s iPhone", name: "Sam’s iPhone", customName: null, kind: "wifi", ssid: "Sam’s iPhone", routerMac: "92:12:4c:aa:bb:cc", firstSeen: now / 1000 - 300 * 86400, lastSeen: now / 1000 - 3 * 86400, lastInterface: "en0" },
];

const rows = new Map<number, HourRow[]>([[1, []], [2, []], [3, []]]);
const coverage: [number, number][] = [];
for (let h = nowHour - 400 * 24; h <= nowHour; h++) {
  const d = new Date(h * HOUR);
  const hour = d.getHours();
  const day = Math.floor(h / 24);
  const vacation = day % 97 < 6;
  if (hour < 7 || vacation) continue;
  coverage.push([h, 3600]);
  const workday = d.getDay() >= 1 && d.getDay() <= 5 && day % 11 !== 0;
  const net = workday && hour >= 9 && hour < 17 ? 2 : day % 23 === 0 && hour > 12 ? 3 : 1;
  const base = net === 2 ? 160e6 : net === 3 ? 45e6 : 120e6 * (hour >= 19 ? 3.2 : 1);
  const growth = 1 + ((h - (nowHour - 9600)) / 9600) * 0.5;
  const rx = Math.round(base * growth * (0.15 + rand() * 1.45));
  const tx = Math.round(rx * (0.04 + rand() * 0.18));
  const est = rand() < 0.025;
  rows.get(net)!.push({ hour: h, rx, tx, estimatedRx: est ? rx : 0, estimatedTx: est ? tx : 0 });
}

function usage(network: number | null, from: number, to: number) {
  const pick = (r: HourRow) => r.hour >= from && r.hour <= to;
  if (network !== null) return (rows.get(network) ?? []).filter(pick);
  const sum = new Map<number, HourRow>();
  for (const list of rows.values())
    for (const r of list.filter(pick)) {
      const s = sum.get(r.hour) ?? { hour: r.hour, rx: 0, tx: 0, estimatedRx: 0, estimatedTx: 0 };
      sum.set(r.hour, { hour: r.hour, rx: s.rx + r.rx, tx: s.tx + r.tx, estimatedRx: s.estimatedRx + r.estimatedRx, estimatedTx: s.estimatedTx + r.estimatedTx });
    }
  return [...sum.values()].sort((a, b) => a.hour - b.hour);
}

let caps = [
  { id: 1, networkId: 3, period: "month", reset: 1, limitBytes: 15e9, alerts: [80, 100], action: "block", throttleKbps: 1000 },
  { id: 2, networkId: null, period: "month", reset: 12, limitBytes: 1.2e12, alerts: [80, 100], action: "none", throttleKbps: 1000 },
  { id: 3, networkId: 1, period: "week", reset: 1, limitBytes: 20e9, alerts: [50, 80, 100], action: "throttle", throttleKbps: 1000 },
];
// Extra usage on top of the synthetic history so the mock shows a cap that's used up and one that's close.
const EXTRA_USED: Record<number, number> = { 1: 15.6e9, 3: 7e9 };

function periodBounds(period: string, reset: number): [Date, Date] {
  const today = new Date(new Date().setHours(0, 0, 0, 0));
  const [y, m, d] = [today.getFullYear(), today.getMonth(), today.getDate()];
  if (period === "day") return [today, new Date(y, m, d + 1)];
  if (period === "week") {
    const start = d - ((today.getDay() - reset + 7) % 7);
    return [new Date(y, m, start), new Date(y, m, start + 7)];
  }
  const month = m - (d < reset ? 1 : 0);
  return [new Date(y, month, reset), new Date(y, month + 1, reset)];
}

const sumUsed = (network: number | null, fromHour: number) => usage(network, fromHour, nowHour).reduce((s, r) => s + r.rx + r.tx, 0);

const params = new URLSearchParams(location.search);
const appsMode = params.get("apps");
const platform = params.get("platform") ?? "macos";
let perAppOn = appsMode !== "off" && appsMode !== "deny";

// name, colour, share of covered traffic (sums to ~0.84, the rest shows as System), upload weight
const appDefs: [string, string, number, number][] = [
  ["Steam", "#1b2838", 0.18, 0.2], ["Dropbox", "#0061ff", 0.1, 3], ["Google Chrome", "#ea4335", 0.1, 0.8], ["Zoom", "#2d8cff", 0.08, 2.5],
  ["Spotify", "#1db954", 0.07, 0.3], ["Slack", "#611f69", 0.05, 0.9], ["Safari", "#0a84ff", 0.05, 0.7], ["Microsoft Teams", "#5b5fc7", 0.04, 1.8],
  ["Figma", "#f24e1e", 0.04, 1.2], ["Docker", "#2496ed", 0.03, 1], ["iCloud Drive", "#4aa8f0", 0.03, 2], ["Discord", "#5865f2", 0.02, 0.8], ["OneDrive", "#0f6cbd", 0.02, 2.2],
];
const letterIcon = (name: string, color: string) =>
  "data:image/svg+xml;utf8," + encodeURIComponent(
    `<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><rect width="40" height="40" rx="9" fill="${color}"/><text x="20" y="27" font-family="sans-serif" font-size="20" font-weight="600" text-anchor="middle" fill="#fff">${name[0]}</text></svg>`);
const sinceHour = nowHour - 20 * 24;

function appsReport(network: number | null, from: number, to: number) {
  if (appsMode === "empty") return { apps: [], coveredRx: 0, coveredTx: 0, sinceHour: null };
  const list = usage(network, Math.max(from, sinceHour), to);
  const rx = list.reduce((s, r) => s + r.rx, 0);
  const tx = list.reduce((s, r) => s + r.tx, 0);
  const apps = appDefs.map(([name, color, w, up], i) => {
    const f = w * (1 + 0.4 * Math.sin(i * (network ?? 5)));
    return { id: i + 1, key: name.toLowerCase().replace(/ /g, "-"), name, icon: letterIcon(name, color), rx: Math.round(rx * f), tx: Math.round(tx * Math.min(0.2, w * up * 0.6)) };
  });
  return { apps, coveredRx: rx, coveredTx: tx, sinceHour };
}
mockWindows(location.hash === "#tray" ? "tray" : "main");
mockIPC((cmd, args: any) => {
  switch (cmd) {
    case "status":
      return {
        networks: params.has("empty") ? [] : networks,
        live: { active: [{ networkId: 1, interface: "en0", kind: "wifi" }], primary: 1, rate: [1_840_000, 96_000], lastSample: now / 1000,
          perApp: { enabled: perAppOn, error: appsMode === "error" ? "The per-app helper stopped responding" : null, experimental: platform !== "macos" } },
        firstCoveredHour: nowHour - 400 * 24,
        wifiNameAccess: params.get("access") ?? "granted",
        platform,
        sampleInterval: 30,
        retentionMonths: 24,
        uploadAlerts: true,
        canThrottle: params.get("platform") !== "windows",
      };
    case "usage":
      return { rows: usage(args.network, args.fromHour, args.toHour), coverage: coverage.filter(([h]) => h >= args.fromHour && h <= args.toHour) };
    case "totals":
      return [1, 2, 3].map((id) => {
        const list = usage(id, args.fromHour, args.toHour);
        return { networkId: id, rx: list.reduce((s, r) => s + r.rx, 0), tx: list.reduce((s, r) => s + r.tx, 0) };
      });
    case "caps":
      return caps.map((c) => {
        const [start, end] = periodBounds(c.period, c.reset);
        return { ...c, used: sumUsed(c.networkId, Math.floor(start.getTime() / HOUR)) + (EXTRA_USED[c.id] ?? 0), start: start.getTime() / 1000, end: end.getTime() / 1000 };
      });
    case "save_cap":
      caps = args.cap.id ? caps.map((c) => (c.id === args.cap.id ? args.cap : c)) : [...caps, { ...args.cap, id: Date.now() }];
      setTimeout(() => emit("overlook://changed"));
      return null;
    case "delete_cap":
      caps = caps.filter((c) => c.id !== args.id);
      setTimeout(() => emit("overlook://changed"));
      return null;
    case "apps":
      return appsReport(args.network, args.fromHour, args.toHour);
    case "set_per_app":
      if (args.enabled && appsMode === "deny") throw new Error("Permission wasn't granted");
      perAppOn = args.enabled;
      return null;
    case "delete_app_data":
      return null;
    case "dropped_bytes":
      return 18_400_000;
    case "plugin:autostart|is_enabled":
      return false;
    default:
      return null;
  }
}, { shouldMockEvents: true });
