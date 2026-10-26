import { useLayoutEffect, useRef, useState } from "react";
import { RangeId } from "./lib/analytics";
import { api, displayName, Network, Status, useStatus } from "./lib/api";
import { ago } from "./lib/format";
import { useAppearance } from "./lib/theme";
import { Dashboard } from "./components/Dashboard";
import { Icon, kindIcon } from "./components/Icon";
import { Onboarding, RenameDialog, ConfirmDialog, Settings } from "./components/Dialogs";
import { ExportButton, RangeMenu } from "./components/Toolbar";

type Overlay =
  | { kind: "settings"; tab?: string }
  | { kind: "rename"; network: Network }
  | { kind: "delete"; network: Network }
  | null;

export default function App() {
  const status = useStatus();
  const [appearance, setAppearance] = useAppearance();
  const [range, setRangeState] = useState<RangeId>(() => (localStorage.getItem("tally.range") as RangeId) || "week");
  const [selection, setSelection] = useState<number | null | undefined>(undefined);
  const [overlay, setOverlay] = useState<Overlay>(null);
  const [onboarded, setOnboarded] = useState(() => localStorage.getItem("tally.onboarded") === "1");
  const mainRef = useRef<HTMLDivElement>(null);
  const [narrow, setNarrow] = useState(false);

  useLayoutEffect(() => {
    const el = mainRef.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => setNarrow(e.contentRect.width < 820));
    ro.observe(el);
    return () => ro.disconnect();
  }, [status === undefined]);

  if (!status) return null;
  const setRange = (r: RangeId) => {
    localStorage.setItem("tally.range", r);
    setRangeState(r);
  };
  const selectedId = selection === undefined ? status.live.primary : selection;
  const network = status.networks.find((n) => n.id === selectedId) ?? null;

  return (
    <div className={`app platform-${status.platform}`}>
      <Sidebar status={status} selected={network?.id ?? null} onSelect={setSelection}
        onSettings={() => setOverlay({ kind: "settings" })}
        onRename={(n) => setOverlay({ kind: "rename", network: n })}
        onDelete={(n) => setOverlay({ kind: "delete", network: n })} />

      <main className={`main ${narrow ? "narrow" : ""}`} ref={mainRef}>
        <header className="topbar" data-tauri-drag-region>
          <h1 data-tauri-drag-region>{network ? displayName(network) : "All networks"}</h1>
          <span className="spacer" data-tauri-drag-region />
          <RangeMenu value={range} onChange={setRange} />
          <ExportButton network={network} range={range} />
        </header>
        <div className="content">
          {status.networks.length === 0 ? (
            <div className="empty">
              <div>
                <Icon name="arrows" size={28} className="faint" />
                <h2>{status.live.active.length ? "Collecting first measurements" : "No network connection"}</h2>
                <div>
                  {status.live.active.length
                    ? `Tally samples traffic every ${status.sampleInterval} seconds. Your first numbers will appear shortly.`
                    : "Tally records usage once you join a Wi-Fi or Ethernet network that routes to the internet."}
                </div>
              </div>
            </div>
          ) : (
            <Dashboard key={`${network?.id}-${range}`} status={status} network={network} range={range} narrow={narrow}
              onSelect={setSelection} onRename={(n) => setOverlay({ kind: "rename", network: n })}
              onAccuracy={() => setOverlay({ kind: "settings", tab: "accuracy" })} />
          )}
        </div>
      </main>

      {overlay?.kind === "settings" && (
        <Settings status={status} initialTab={overlay.tab} appearance={appearance} onAppearance={setAppearance} onClose={() => setOverlay(null)} />
      )}
      {overlay?.kind === "rename" && <RenameDialog network={overlay.network} onClose={() => setOverlay(null)} />}
      {overlay?.kind === "delete" && (
        <ConfirmDialog
          title={`Delete history for “${displayName(overlay.network)}”?`}
          message="Recorded usage for this network is permanently removed. If you're still connected, Tally starts a new history."
          confirm="Delete history"
          onConfirm={() => {
            api.deleteNetwork(overlay.network.id);
            setSelection(null);
          }}
          onClose={() => setOverlay(null)} />
      )}
      {!onboarded && (
        <Onboarding status={status} onDone={() => {
          localStorage.setItem("tally.onboarded", "1");
          setOnboarded(true);
        }} />
      )}
    </div>
  );
}

function Sidebar({ status, selected, onSelect, onSettings, onRename, onDelete }: {
  status: Status;
  selected: number | null;
  onSelect: (id: number | null) => void;
  onSettings: () => void;
  onRename: (n: Network) => void;
  onDelete: (n: Network) => void;
}) {
  const [menu, setMenu] = useState<{ n: Network; x: number; y: number } | null>(null);
  const showNotice = status.wifiNameAccess === "denied" || status.wifiNameAccess === "notDetermined";
  return (
    <aside className="sidebar" data-tauri-drag-region onClick={() => setMenu(null)}>
      <div className="brand" data-tauri-drag-region>
        <span className="brand-mark"><Icon name="binoculars" size={14} /></span>
        Tally
      </div>
      <nav className="nav">
        <button className={`nav-item ${selected === null ? "active" : ""}`} onClick={() => onSelect(null)}>
          <Icon name="grid" size={15} />
          <span className="name">All networks</span>
        </button>
        <div className="nav-label">Networks</div>
        {status.networks.map((n) => {
          const live = status.live.active.some((a) => a.networkId === n.id);
          return (
            <button key={n.id} className={`nav-item ${selected === n.id ? "active" : ""}`} onClick={() => onSelect(n.id)}
              onContextMenu={(e) => {
                e.preventDefault();
                setMenu({ n, x: e.clientX, y: e.clientY });
              }}>
              <Icon name={kindIcon(n.kind)} size={15} />
              <span className="name">
                {displayName(n)}
                <span className="sub">{live ? "Connected" : `Seen ${ago(n.lastSeen)}`}</span>
              </span>
              {live && <span className="dot" title="Connected now" />}
            </button>
          );
        })}
      </nav>
      <div className="sidebar-foot">
        {showNotice && (
          <div className="notice">
            <strong>Wi-Fi names hidden</strong>
            {status.platform === "macos" ? "macOS only shares network names with apps that have Location access." : "Windows only shares network names with apps that have Location access."}{" "}
            Networks are still tracked separately by router.
            <div style={{ marginTop: 8 }}>
              <button className="btn" onClick={() => api.requestWifiNames()}><Icon name="location" size={13} />{status.wifiNameAccess === "notDetermined" ? "Allow access" : "Open settings"}</button>
            </div>
          </div>
        )}
        <button className="nav-item" onClick={onSettings}><Icon name="settings" size={15} /><span className="name">Settings</span></button>
      </div>
      {menu && (
        <div className="tooltip" style={{ position: "fixed", left: menu.x, top: menu.y, pointerEvents: "auto", padding: 4, minWidth: 160 }}>
          <button className="nav-item" onClick={() => onRename(menu.n)}><Icon name="edit" size={13} />Rename…</button>
          <button className="nav-item" style={{ color: "var(--danger)" }} onClick={() => onDelete(menu.n)}><Icon name="trash" size={13} />Delete history…</button>
        </div>
      )}
    </aside>
  );
}

