import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import { buildReport, Granularity, HourRow, hourOf, queryStartHour, RangeId, Report } from "./analytics";

export interface Network {
  id: number;
  key: string;
  name: string;
  customName: string | null;
  kind: "wifi" | "ethernet" | string;
  ssid: string | null;
  routerMac: string | null;
  firstSeen: number;
  lastSeen: number;
  lastInterface: string | null;
}

export interface Status {
  networks: Network[];
  live: {
    active: { networkId: number; interface: string; kind: string }[];
    primary: number | null;
    rate: [number, number] | null;
    lastSample: number;
    perApp: { enabled: boolean; error: string | null; experimental: boolean };
  };
  firstCoveredHour: number | null;
  wifiNameAccess: "granted" | "denied" | "notDetermined" | "unknown";
  platform: "macos" | "windows" | "linux" | string;
  sampleInterval: number;
  retentionMonths: number;
}

export interface AppTotal {
  id: number;
  key: string;
  name: string;
  icon: string | null;
  rx: number;
  tx: number;
}

export interface AppsReport {
  apps: AppTotal[];
  coveredRx: number;
  coveredTx: number;
  sinceHour: number | null;
}

export const displayName = (n: Network) => n.customName ?? n.name;

export const api = {
  status: () => invoke<Status>("status"),
  usage: (network: number | null, fromHour: number, toHour: number) =>
    invoke<{ rows: HourRow[]; coverage: [number, number][] }>("usage", { network, fromHour, toHour }),
  apps: (network: number | null, fromHour: number, toHour: number) => invoke<AppsReport>("apps", { network, fromHour, toHour }),
  setPerApp: (enabled: boolean) => invoke("set_per_app", { enabled }),
  deleteAppData: () => invoke("delete_app_data"),
  totals: (fromHour: number, toHour: number) => invoke<{ networkId: number; rx: number; tx: number }[]>("totals", { fromHour, toHour }),
  droppedBytes: (since: number) => invoke<number>("dropped_bytes", { since }),
  rename: (id: number, name: string | null) => invoke("rename_network", { id, name }),
  deleteNetwork: (id: number) => invoke("delete_network", { id }),
  deleteAll: () => invoke("delete_all"),
  setRetention: (months: number) => invoke("set_retention", { months }),
  requestWifiNames: () => invoke("request_wifi_names"),
  revealData: () => invoke("reveal_data"),
  saveExport: (fileName: string, contents: string) => invoke<string>("save_export", { fileName, contents }),
  showMain: () => invoke("show_main"),
  quit: () => invoke("quit"),
};

export function useLive<T>(load: () => Promise<T>, deps: unknown[]): T | undefined {
  const [value, setValue] = useState<T>();
  const run = useCallback(load, deps);
  useEffect(() => {
    let alive = true;
    const refresh = () => run().then((v) => alive && setValue(v)).catch(console.error);
    refresh();
    const unlisten = listen("overlook://changed", refresh);
    return () => {
      alive = false;
      unlisten.then((f) => f());
    };
  }, [run]);
  return value;
}

export function useStatus() {
  return useLive(api.status, []);
}

export function useReport(network: number | null, range: RangeId, granularity: Granularity | undefined, historyStartHour: number | null) {
  return useLive<Report>(async () => {
    const now = new Date();
    const { rows, coverage } = await api.usage(network, queryStartHour(range, now), hourOf(now.getTime()) + 1);
    return buildReport({ range, granularity, rows, coverage, historyStartHour, now });
  }, [network, range, granularity, historyStartHour]);
}

export function useApps(network: number | null, range: RangeId) {
  return useLive<AppsReport>(() => {
    const now = new Date();
    return api.apps(network, queryStartHour(range, now), hourOf(now.getTime()) + 1);
  }, [network, range]);
}
