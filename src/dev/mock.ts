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

const params = new URLSearchParams(location.search);
mockWindows(location.hash === "#tray" ? "tray" : "main");
mockIPC((cmd, args: any) => {
  switch (cmd) {
    case "status":
      return {
        networks: params.has("empty") ? [] : networks,
        live: { active: [{ networkId: 1, interface: "en0", kind: "wifi" }], primary: 1, rate: [1_840_000, 96_000], lastSample: now / 1000 },
        firstCoveredHour: nowHour - 400 * 24,
        wifiNameAccess: params.get("access") ?? "granted",
        platform: params.get("platform") ?? "macos",
        sampleInterval: 30,
        retentionMonths: 24,
      };
    case "usage":
      return { rows: usage(args.network, args.fromHour, args.toHour), coverage: coverage.filter(([h]) => h >= args.fromHour && h <= args.toHour) };
    case "totals":
      return [1, 2, 3].map((id) => {
        const list = usage(id, args.fromHour, args.toHour);
        return { networkId: id, rx: list.reduce((s, r) => s + r.rx, 0), tx: list.reduce((s, r) => s + r.tx, 0) };
      });
    case "dropped_bytes":
      return 18_400_000;
    case "plugin:autostart|is_enabled":
      return false;
    default:
      return null;
  }
});
