import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { useEffect, useState } from "react";
import { api, displayName, Network, Status, useLive } from "../lib/api";
import { bytes } from "../lib/format";
import { Appearance } from "../lib/theme";
import { Icon, IconName } from "./Icon";
import { CapsPane } from "./Caps";
import { Dialog, Segmented, Setting, Toggle } from "./ui";

// Tauri commands reject with a string; anything else (the dev mock) throws an Error.
const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e));

export function RenameDialog({ network, onClose }: Readonly<{ network: Network; onClose: () => void }>) {
  const [name, setName] = useState(displayName(network));
  const save = (value: string | null) => {
    api.rename(network.id, value);
    onClose();
  };
  return (
    <Dialog onClose={onClose}>
      <form className="dialog-body" onSubmit={(e) => { e.preventDefault(); save(name); }}>
        <h2>Rename network</h2>
        <p>Only changes how this network appears in Overlook.</p>
        <input className="text" data-autofocus value={name} onChange={(e) => setName(e.target.value)} />
      </form>
      <div className="dialog-actions">
        {network.customName && <button className="btn ghost" style={{ marginRight: "auto" }} onClick={() => save(null)}>Use original name</button>}
        <button className="btn" onClick={onClose}>Cancel</button>
        <button className="btn primary" onClick={() => save(name)}>Save</button>
      </div>
    </Dialog>
  );
}

export function ConfirmDialog({ title, message, confirm, onConfirm, onClose }: Readonly<{ title: string; message: string; confirm: string; onConfirm: () => void; onClose: () => void }>) {
  return (
    <Dialog onClose={onClose}>
      <div className="dialog-body">
        <h2>{title}</h2>
        <p>{message}</p>
      </div>
      <div className="dialog-actions">
        <button className="btn" data-autofocus onClick={onClose}>Cancel</button>
        <button className="btn danger" onClick={() => { onConfirm(); onClose(); }}>{confirm}</button>
      </div>
    </Dialog>
  );
}

export function PerAppDialog({ status, onClose }: Readonly<{ status: Status; onClose: () => void }>) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const helper = status.platform === "windows" ? "admin rights" : "root";
  const turnOn = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.setPerApp(true);
      onClose();
    } catch (e) {
      setError(errorText(e));
      setBusy(false);
    }
  };
  const row = (label: string, text: string) => (
    <div style={{ display: "flex", gap: 10, marginBottom: 10 }}>
      <b style={{ width: 62, flex: "none" }}>{label}</b>
      <span className="muted">{text}</span>
    </div>
  );
  return (
    <Dialog onClose={onClose}>
      <div className="dialog-body">
        <h2>Turn on per-app tracking? {status.live.perApp.experimental && <span className="pill muted">Experimental</span>}</h2>
        {row("Pro", "See which apps use data on each network.")}
        {status.platform !== "macos" && row("Con", `Needs ${helper} to install a small background helper.`)}
        {status.platform !== "macos" && row("Con", "A helper process runs alongside Overlook.")}
        {row("Con", "Only tracks from now on. Earlier history stays per-network only.")}
        {row("Privacy", "App names and byte counts only, never contents or destinations. Stored only on this computer.")}
        {error && <span className="pill warn">{error}</span>}
      </div>
      <div className="dialog-actions">
        <button className="btn" data-autofocus onClick={onClose}>Cancel</button>
        <button className="btn primary" disabled={busy} onClick={turnOn}>Turn on</button>
      </div>
    </Dialog>
  );
}

export function Onboarding({ status, onDone }: Readonly<{ status: Status; onDone: () => void }>) {
  const point = (icon: IconName, title: string, text: string) => (
    <div style={{ display: "flex", gap: 12, marginBottom: 14 }}>
      <Icon name={icon} size={16} style={{ color: "var(--download)", marginTop: 1, flex: "none" }} />
      <div>
        <div style={{ fontWeight: 600, marginBottom: 2 }}>{title}</div>
        <div className="muted" style={{ fontSize: 12 }}>{text}</div>
      </div>
    </div>
  );
  const gated = status.wifiNameAccess === "notDetermined" || status.wifiNameAccess === "denied";
  return (
    <Dialog onClose={onDone}>
      <div className="dialog-body">
        <img src="/icon.png" width={44} height={44} alt="" style={{ marginBottom: 14 }} />
        <h2 style={{ fontSize: 20 }}>Welcome to Overlook</h2>
        <p>See how much data you use on each network.</p>
        <div style={{ marginTop: 18 }}>
          {point("chart", "Measured, not guessed", `Overlook reads your computer's network byte counters every ${status.sampleInterval} seconds. Gaps show as gaps, and estimates are labelled.`)}
          {point("wifi", "Separate history per network", "Home, office and hotspot usage is recorded separately, including across switches, sleep and restarts.")}
          {point("lock", "Stays on this computer", "History is stored locally. Overlook sends nothing anywhere.")}
          {gated && point("location", "Wi-Fi names need Location access", "Your OS hides network names from apps without it. Overlook never reads your location; without access, networks are told apart by their router.")}
        </div>
      </div>
      <div className="dialog-actions">
        {gated && <button className="btn" style={{ marginRight: "auto" }} onClick={() => api.requestWifiNames()}>Allow Wi-Fi names</button>}
        <button className="btn primary" data-autofocus onClick={onDone}>Get started</button>
      </div>
    </Dialog>
  );
}

const TABS: { id: string; label: string; icon: IconName }[] = [
  { id: "general", label: "General", icon: "settings" },
  { id: "caps", label: "Caps & alerts", icon: "alert" },
  { id: "appearance", label: "Appearance", icon: "paint" },
  { id: "data", label: "Data", icon: "database" },
  { id: "accuracy", label: "Accuracy", icon: "info" },
];

export function Settings({ status, initialTab, appearance, onAppearance, onClose }: Readonly<{
  status: Status;
  initialTab?: string;
  appearance: Appearance;
  onAppearance: (a: Appearance) => void;
  onClose: () => void;
}>) {
  const [tab, setTab] = useState(initialTab ?? "general");
  return (
    <Dialog onClose={onClose} wide>
      <div className="settings">
        <nav className="settings-nav">
          {TABS.map((t) => (
            <button key={t.id} className={`nav-item ${tab === t.id ? "active" : ""}`} onClick={() => setTab(t.id)}>
              <Icon name={t.icon} size={14} />
              <span className="name">{t.label}</span>
            </button>
          ))}
          <button className="btn" style={{ marginTop: "auto" }} onClick={onClose}>Done</button>
        </nav>
        <div className="settings-pane">
          {tab === "general" && <General status={status} />}
          {tab === "caps" && <CapsPane status={status} />}
          {tab === "appearance" && <AppearancePane value={appearance} onChange={onAppearance} />}
          {tab === "data" && <DataPane status={status} />}
          {tab === "accuracy" && <Accuracy status={status} />}
        </div>
      </div>
    </Dialog>
  );
}

function General({ status }: Readonly<{ status: Status }>) {
  const [login, setLogin] = useState<boolean | null>(null);
  useEffect(() => {
    isEnabled().then(setLogin).catch(() => setLogin(false));
  }, []);
  const access = { granted: "Allowed", denied: "Not allowed", notDetermined: "Not requested", unknown: "No permission needed" }[status.wifiNameAccess];
  return (
    <>
      <h3>General</h3>
      <Setting title="Open at login" desc="Overlook only records usage while it's running. It stays in the menu bar / system tray when the window is closed.">
        <Toggle label="Open at login" checked={!!login} onChange={async (v) => {
          await (v ? enable() : disable()).catch(console.error);
          setLogin(await isEnabled());
        }} />
      </Setting>
      <Setting title="Wi-Fi network names" desc={status.platform === "linux" ? "Read with iw or NetworkManager." : "Your OS requires Location access to share Wi-Fi names. Overlook never reads or stores your location."}>
        <span style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span className="muted" style={{ whiteSpace: "nowrap" }}>{access}</span>
          {(status.wifiNameAccess === "denied" || status.wifiNameAccess === "notDetermined") && (
            <button className="btn" onClick={() => api.requestWifiNames()}>{status.wifiNameAccess === "notDetermined" ? "Allow…" : "Open settings…"}</button>
          )}
        </span>
      </Setting>
    </>
  );
}

function AppearancePane({ value, onChange }: Readonly<{ value: Appearance; onChange: (a: Appearance) => void }>) {
  return (
    <>
      <h3>Appearance</h3>
      <div className="themes">
        {(["system", "light", "dark"] as const).map((a) => (
          <button key={a} className="theme-tile" aria-pressed={value === a} onClick={() => onChange(a)}>
            <div className="preview">
              {a === "system" ? (
                <div style={{ position: "relative", flex: 1, display: "flex" }}>
                  <MiniPreview dark={false} />
                  <div style={{ position: "absolute", inset: 0, display: "flex", clipPath: "polygon(60% 0, 100% 0, 100% 100%, 40% 100%)" }}><MiniPreview dark /></div>
                </div>
              ) : <MiniPreview dark={a === "dark"} />}
            </div>
            {a[0].toUpperCase() + a.slice(1)}
          </button>
        ))}
      </div>
      <div className="desc muted" style={{ fontSize: 12 }}>System follows your operating system's light or dark setting.</div>
    </>
  );
}

function MiniPreview({ dark }: Readonly<{ dark: boolean }>) {
  const c = dark
    ? { bg: "#0b0b0c", card: "#111112", line: "rgba(255,255,255,0.08)", ink: "#ededef", down: "#a29ef6", up: "#6366f1" }
    : { bg: "#fafafa", card: "#ffffff", line: "rgba(0,0,0,0.08)", ink: "#0a0a0b", down: "#9b98f3", up: "#4f46e5" };
  const bars = [0.5, 0.8, 0.35, 0.9, 0.6, 0.7];
  return (
    <div style={{ flex: 1, background: c.bg, display: "flex", overflow: "hidden" }}>
      <div style={{ width: 18, borderRight: `1px solid ${c.line}` }} />
      <div style={{ flex: 1, padding: 8 }}>
        <div style={{ width: 30, height: 4, borderRadius: 2, background: c.ink, opacity: 0.8, marginBottom: 6 }} />
        <div style={{ background: c.card, border: `1px solid ${c.line}`, borderRadius: 4, height: 46, display: "flex", alignItems: "flex-end", gap: 3, padding: 5 }}>
          {bars.map((h, i) => (
            <div key={i} style={{ flex: 1, display: "flex", flexDirection: "column", gap: 1 }}>
              <div style={{ height: 30 * h * 0.25, background: c.up, borderRadius: "1px 1px 0 0" }} />
              <div style={{ height: 30 * h * 0.75, background: c.down }} />
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function DataPane({ status }: Readonly<{ status: Status }>) {
  const [confirm, setConfirm] = useState(false);
  const [perApp, setPerApp] = useState(false);
  const [askDelete, setAskDelete] = useState(false);
  const [offError, setOffError] = useState<string | null>(null);
  const turnOff = async () => {
    setOffError(null);
    try {
      await api.setPerApp(false);
      setAskDelete(true);
    } catch (e) {
      setOffError(errorText(e));
    }
  };
  const dropped = useLive(() => api.droppedBytes(Date.now() / 1000 - 365 * 86400), []) ?? 0;
  const first = status.firstCoveredHour;
  return (
    <>
      <h3>Data</h3>
      <Setting title="Keep history for" desc="Stored as hourly totals, about 100 KB per network per year. Two years allows a full year-over-year comparison.">
        <Segmented size="sm" label="Retention" value={String(status.retentionMonths)} onChange={(v) => api.setRetention(Number(v))}
          options={[{ value: "12", label: "1 year" }, { value: "24", label: "2 years" }, { value: "60", label: "5 years" }, { value: "0", label: "Forever" }]} />
      </Setting>
      <Setting title="Usage database" desc={first ? `Collecting since ${new Date(first * 3_600_000).toLocaleDateString(undefined, { dateStyle: "medium" })} · ${status.networks.length} networks` : "No data yet"}>
        <button className="btn" onClick={() => api.revealData()}><Icon name="folder" size={13} />Show file</button>
      </Setting>
      {dropped > 0 && (
        <Setting title="Unattributed traffic (1 year)" desc="Measured while no identifiable network was connected, or across a gap during which the network changed. Not counted toward any network.">
          <span className="num">{bytes(dropped)}</span>
        </Setting>
      )}
      <Setting title="Per-app tracking" desc={`See which apps use data on each network.${status.platform !== "macos" ? " Runs a small helper process." : ""}`}>
        <span style={{ display: "flex", alignItems: "center", gap: 10 }}>
          {status.live.perApp.experimental && <span className="pill muted">Experimental</span>}
          {offError && <span className="pill warn">{offError}</span>}
          <Toggle label="Per-app tracking" checked={status.live.perApp.enabled} onChange={(v) => (v ? setPerApp(true) : turnOff())} />
        </span>
      </Setting>
      <Setting title="Delete all usage data" desc="Everything stays on this computer. Overlook makes no network requests of its own.">
        <button className="btn danger" onClick={() => setConfirm(true)}><Icon name="trash" size={13} />Delete…</button>
      </Setting>
      {perApp && <PerAppDialog status={status} onClose={() => setPerApp(false)} />}
      {askDelete && (
        <ConfirmDialog title="Delete per-app history?" message="Per-app tracking is off. Delete the recorded per-app usage too, or keep it?" confirm="Delete history"
          onConfirm={() => api.deleteAppData()} onClose={() => setAskDelete(false)} />
      )}
      {confirm && (
        <ConfirmDialog title="Delete all usage data?" message="All networks and their history are permanently removed." confirm="Delete all data"
          onConfirm={() => api.deleteAll()} onClose={() => setConfirm(false)} />
      )}
    </>
  );
}

export function Accuracy({ status }: Readonly<{ status: Status }>) {
  const counters = { macos: "the kernel's 64-bit interface counters (sysctl)", windows: "the 64-bit adapter counters from IP Helper (GetIfEntry2)", linux: "the kernel's interface statistics in /sys/class/net" }[status.platform] ?? "the OS interface counters";
  return (
    <div className="prose" style={{ overflowY: "auto", maxHeight: 420 }}>
      <h3 style={{ marginBottom: 14 }}>How Overlook measures</h3>
      <h4>What's counted</h4>
      <p>Every {status.sampleInterval} seconds, and within seconds of a network change, Overlook reads {counters} for each physical Wi-Fi and Ethernet adapter and records the difference. The counters are exact for what crosses the adapter.</p>
      <h4>Which network</h4>
      <p>Usage goes to the network the adapter was on: Wi-Fi by name (SSID), or by the router's hardware address when the name isn't available. Two networks with the same name are treated as one. Adapter names like en0 or "Wi-Fi" are never used alone, because they don't say which network you're on.</p>
      <h4>Estimates</h4>
      <p>Values are marked estimated when the bytes are real but their attribution is inferred: traffic around a network switch, or during sleep or while Overlook wasn't running, when the network was the same before and after. If the network changed during a gap, those bytes aren't counted and appear as unattributed in Data.</p>
      <h4>Not tracked</h4>
      <p>Overlook can't see traffic while it isn't running, and counters restart on reboot, so traffic between startup and Overlook launching is lost. Untracked periods show as gaps, never as zero.</p>
      <h4>Known limits</h4>
      <p>Counters include local-network traffic (casting, file sharing, backups to a NAS), which may not count against a data cap. VPN traffic is counted once, on the adapter that carries it. Virtual adapters (VPN, Hyper-V, Docker, WSL) are excluded. Per-app figures are opt-in and only cover the time since you turned them on; see the README for platform limits.</p>
      <h4>Privacy</h4>
      <p>All data stays in a local SQLite database. Overlook doesn't transmit anything and never reads your location; the Location permission only unlocks Wi-Fi names.</p>
    </div>
  );
}
