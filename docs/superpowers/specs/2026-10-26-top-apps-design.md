# Top apps: per-app data usage per network

Status: approved · 2026-10-26

## Why

Users have asked for this. The typical case is a user who sees a spike on one network, often a capped one like a phone hotspot, and wants to know which app caused it. Overlook currently can't answer that; the README lists "no per-app breakdown" as a limitation.

Success: someone opens the *iPhone Hotspot* view for the last 7 days and sees immediately that Steam used 25 GB of it.

## Goals

- A ranked list of apps by data used, **per network** and for the selected range. "All networks" shows apps summed across all networks.
- Every app shown with its real icon and a friendly name.
- macOS, Windows and Linux in the same release.
- Everything stays on the computer, as it does today.

## Non-goals

- Clicking an app to see its usage over time. The list is read-only.
- Per-app numbers in the tray popover.
- Per-app columns in the CSV export.
- Attributing history recorded before the feature was turned on.
- Per-app limits, alerts or blocking.

## Anti-goals (it fails if it does these, even if it works)

- **It asks for admin or root rights without first explaining why.**
- **It shows raw process names** (`com.apple.WebKit.Networking`, `chrome.exe` ×14) instead of apps. The Dribbble shot *Bandit* shows this failure: duplicate rows per PID and helper names nobody recognises.
- **App totals that don't add up to the network total.** Unattributed traffic must stay visible, not silently disappear.
- **It breaks or slows the existing per-network collection** when per-app tracking is off, denied or failing.

## Decided with the user

1. **Per network.** App usage is recorded against the network it happened on, using the same attribution Overlook already uses for network totals.
2. **All three platforms in the same release, with Windows and Linux marked experimental.** No Windows or Linux test machines are available, so those helpers ship behind an *Experimental* label in the opt-in dialog and Settings until they've been run on real machines. The macOS code is verified locally.
3. **App icons on every row.** If an icon can't be found (Linux CLI tools, daemons, anything else), a generic icon is shown.
4. **Elevated rights are acceptable** if the pros and cons are disclosed first. Per-app tracking is opt-in.
5. **Read-only list**, no drill-down.
6. **Layout B:** a full-width *Top apps* table placed under the *Download vs upload* / *Usage trend* row and above the *Recent days* / *Networks* table.

## How it behaves

### Opting in

- Off by default. Turned on from a new *Per-app tracking* toggle in Settings, and from the button in the Top apps empty state.
- Turning it on first opens a disclosure dialog (existing `Dialog` component) listing:
  - **Pro:** see which apps use data on each network.
  - **Con:** needs admin rights (Windows) or root (Linux) for a small background helper. On macOS no prompt is needed.
  - **Con:** a helper process runs alongside Overlook.
  - **Con:** only tracks from now on; earlier history stays per-network only.
  - **Privacy:** app names and byte counts only, never contents or destinations. Stored only on this computer.
- The OS elevation prompt appears only after the user confirms. If they cancel it or it fails, the toggle stays off, nothing else changes, and the card shows why.
- Turning it off stops and (where installed) removes the helper, then asks via `ConfirmDialog` whether to delete per-app history.
- Delete all data (existing) also deletes per-app data. Retention (existing) applies to per-app data too.

### What counts as one app

- **Helpers are grouped under their app.** On macOS, a process inside `Foo.app/Contents/...` belongs to `Foo.app` (outermost bundle). On Windows, processes are grouped by executable path, and well-known helper patterns roll up to their parent install (for example, all `chrome.exe` instances become *Google Chrome*). On Linux, processes are grouped by resolved executable, and where a `.desktop` entry matches, its `Name=` and `Icon=` are used.
- **Display name:** bundle display name (macOS), file-version `FileDescription`/`ProductName` (Windows), `.desktop` `Name=` (Linux), falling back to the executable name.
- **"System" row:** the network's total for the range minus the sum of attributed apps (clamped at zero), shown with a generic icon. This includes OS services, DNS, kernel traffic and anything the helper missed. It guarantees the table adds up to the network total. Because it's computed, not measured, it's only shown for ranges and networks where per-app tracking was active.

### Top apps table (layout B)

Styled like the existing `panel` tables (`NetworksTable` / `DaysTable`).

- **Header:** *Top apps*, with subtitle "{total} across {n} apps · {range}", which follows the same range picker and network selection as the rest of the dashboard.
- **Columns:** rank · icon + name · Downloaded · Uploaded · Total · Share. Sorted by total, descending.
- **Share** is the percentage of the network's total, plus a thin lavender-to-indigo bar behind or under the figure (from Peec AI's *Domain types*, Jomo and Brink on Mobbin).
- **Rows:** the top 10 apps, then a "+ N more" row with up to 3 overlapping mini icons of the hidden apps (from Jomo's "Other" row on Mobbin). Clicking it expands the table in place to show all apps. Rows are not clickable.
- **Partial-history note:** if the range starts before per-app tracking was turned on, a muted line reads "Per-app data since {date}" (Forest on Mobbin uses this kind of footnote).
- **Empty and off states:**
  - *Off:* a muted line explaining the feature with a **Turn on per-app tracking** button.
  - *On but no data yet:* "No app traffic recorded in this range yet." (Pinterest's "Not enough data to display").
  - *Helper failed:* a warn pill with the reason and a **Retry** button.
- Dark and light palettes, typography and tabular numerals follow the existing README design notes. No new colours.
- **Dev mock:** `src/dev/mock.ts` gets synthetic app rows (with placeholder icons) so `npm run dev` shows the table in every state.

### Inspiration referenced

- Mobbin: [Peec AI rankings and domain-type bars](https://mobbin.com/screens/0711fee8-0de2-4f0e-8b7c-16387d22f091), [Linktree top sources with rank column](https://mobbin.com/screens/d4ae5957-1851-4279-9483-025e60f2771c), [Jomo "Other" row with stacked icons](https://mobbin.com/screens/7af61157-8994-4849-bfe0-43a98ced0dca), [Forest privacy and threshold footnote](https://mobbin.com/screens/1e3bd848-9286-4b60-85be-5765e9073782), [Brink inline share bars](https://mobbin.com/screens/bc9c937f-3e31-4a51-8ad2-d215b4e451df).
- Dribbble: [Bandit network monitor](https://dribbble.com/shots/5952169-Bandit-Network-bandwidth-monitoring-app), which shows the right columns (name, download, upload) but also the anti-goal (raw process names, a row per PID). [Screen Time usage stats](https://dribbble.com/shots/27655910-Screen-Time-App-Usage-Stats-iOS-UI-App-Details-Usage-Heatmap), where the "71% of time" share is shown as secondary text.

## Constraints from the existing system

- Collection runs in the Tauri shell's 5 s collector thread (`src-tauri/src/lib.rs`), which calls `Collector::tick` in `overlook-core`. Samples are written every 30 s or on network change. Per-app sampling should follow the same cadence and write in the same transaction, so app deltas are attributed to the same network as the interface deltas.
- Storage is SQLite with hourly buckets (`crates/overlook-core/src/store.rs`). Per-app data should also be hourly buckets keyed by (network, hour, app), so ranges, retention and pruning work the same way.
- Analytics run in the UI (`src/lib/analytics.ts`) so DST and week starts use OS rules. Per-app aggregation by range can use the same hour range the dashboard already queries.
- Platform code lives in `overlook-core/src/system.rs` behind `cfg(target_os)`. The core crate must still type-check for all three targets from any host.
- No UI or chart libraries. Hugeicons only for generic icons.
- Existing per-network collection must keep working unchanged when per-app tracking is off or broken.

## Left to the builder

- **Per-OS measurement mechanism.** Suggested starting points:
  - macOS: `nettop` (`-P -L` CSV of cumulative per-process bytes) or the NetworkStatistics framework it wraps. No elevation.
  - Windows: ETW `Microsoft-Windows-Kernel-Network` per-process send/receive events, from an elevated helper (a Windows service installed once with UAC).
  - Linux: a root helper using eBPF or `sock_diag` plus `/proc/*/fd` socket-to-PID mapping, installed via `pkexec` as a systemd unit.
  Pick whatever gives reliable per-process byte counts without measurable CPU cost. Document the choice and its limits in the README's *How it measures* table.
- **Helper ↔ app IPC** (local socket or named pipe, restricted to the current user). The helper must only expose per-process byte counts.
- **Schema details** (likely an `apps` table with key, display name and cached icon, plus an `app_usage(network_id, hour, app_id, rx, tx)` table) and the migration.
- **Icon extraction and caching:** NSWorkspace icon (macOS), shell icon from the exe (Windows), freedesktop icon-theme lookup (Linux), all rendered to small PNGs and cached by app key.
- **How app traffic is split when two physical networks are active at once.** Default: attribute it to the primary network, matching the live rate. If the measurement mechanism reports the interface, use it.
- **Tests:** unit tests for grouping helpers into apps, delta and reset handling for per-app counters (same rules as `processor.rs`), and the "System" remainder calculation.

## Accounting rules (settled after builder review)

- **Per-app coverage is tracked separately.** It is a table like the existing `coverage`, recording the hours when the per-app helper was delivering data. In the Top apps table, the network total, the System row and Share are all computed **only over hours with per-app coverage**. The subtitle says so ("41.2 GB across 9 apps · per-app data since Oct 26" when partial). Uncovered hours never inflate System.
- **Gaps, switches and sleep.** Per-app deltas are recorded only for normal sampling intervals on an unchanged network. Deltas that span a gap, a network switch or a counter reset are discarded, using the same reset and gap rules as `processor.rs`. That traffic stays in the network total and shows up in System. No estimated flag on per-app rows.
- **Helper lifecycle.** The helper on Windows and Linux runs persistently as a service, started at boot. It reports cumulative per-process counters plus a session id that changes when it restarts. Overlook treats these like interface counters (baseline, delta, new baseline on reset). Traffic while Overlook isn't running is lost to the gap rule above, consistent with how Overlook handles everything else. The macOS collector runs in-process with Overlook.
- **What traffic counts.** Loopback and tunnel/virtual adapters are excluded where the mechanism reports the interface, matching the network totals. LAN traffic counts, as it does for network totals. If attributed apps exceed the network total for a range, System is clamped to 0 and Share uses the app sum as its denominator, so percentages still add up to 100.
- **Linux AppImage.** Enabling the feature copies the helper binary to a fixed system path (`/usr/local/libexec/overlook-helper`) before installing the systemd unit, so it doesn't depend on the AppImage mount.

## Calls I made (not discussed with the user)

- Top 10 rows, then "+ N more" that expands in place.
- A computed "System" row so totals reconcile.
- Per-app history follows the existing retention setting and Delete all data.
- No tray or CSV changes in this version.
- Turning off per-app tracking asks whether to delete per-app history.
- Traffic is attributed to the primary network when two are active and the platform doesn't report the interface.
