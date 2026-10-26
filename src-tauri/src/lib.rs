mod helper;

use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::Duration;
use overlook_core::apps::AppTracker;
use overlook_core::caps::{self, Cap, CapUsage, Verdict};
use overlook_core::enforce::{self, Enforcer};
use overlook_core::store::{AppsReport, HourRow, Network, NetworkTotal, Store};
use overlook_core::system::{self, WifiNameAccess};
use overlook_core::{Collector, LiveStatus, SAMPLE_INTERVAL};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WindowEvent};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_positioner::{Position, WindowExt};

struct AppState {
    collector: Mutex<Collector>,
    db_path: PathBuf,
    tray_rate: AtomicBool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    networks: Vec<Network>,
    live: LiveStatus,
    first_covered_hour: Option<i64>,
    wifi_name_access: WifiNameAccess,
    platform: &'static str,
    sample_interval: f64,
    retention_months: i64,
    upload_alerts: bool,
    can_throttle: bool,
    tray_rate: bool,
}

#[derive(Serialize)]
struct Usage {
    rows: Vec<HourRow>,
    coverage: Vec<(i64, f64)>,
}

fn retention(store: &Store) -> i64 {
    store.meta("retention_months").and_then(|v| v.parse().ok()).unwrap_or(24)
}

#[tauri::command]
fn status(state: State<AppState>) -> Status {
    let c = state.collector.lock().unwrap();
    Status {
        networks: c.store.networks(),
        live: c.status.clone(),
        first_covered_hour: c.store.first_covered_hour(),
        wifi_name_access: system::wifi_name_access(),
        platform: std::env::consts::OS,
        sample_interval: SAMPLE_INTERVAL,
        retention_months: retention(&c.store),
        upload_alerts: c.store.meta("upload_alerts").as_deref() != Some("0"),
        can_throttle: enforce::CAN_THROTTLE,
        tray_rate: state.tray_rate.load(Ordering::Relaxed),
    }
}

#[tauri::command]
fn usage(state: State<AppState>, network: Option<i64>, from_hour: i64, to_hour: i64) -> Usage {
    let c = state.collector.lock().unwrap();
    Usage { rows: c.store.hours(network, from_hour, to_hour), coverage: c.store.coverage(from_hour, to_hour) }
}

#[tauri::command]
fn totals(state: State<AppState>, from_hour: i64, to_hour: i64) -> Vec<NetworkTotal> {
    state.collector.lock().unwrap().store.totals_by_network(from_hour, to_hour)
}

#[tauri::command]
fn dropped_bytes(state: State<AppState>, since: f64) -> i64 {
    state.collector.lock().unwrap().store.dropped_bytes(since)
}

#[tauri::command]
fn apps(state: State<AppState>, network: Option<i64>, from_hour: i64, to_hour: i64) -> AppsReport {
    state.collector.lock().unwrap().store.apps_report(network, from_hour, to_hour)
}

// async so the elevation prompt doesn't block the main thread; the collector isn't locked while it's up.
#[tauri::command]
async fn set_per_app(app: AppHandle, state: State<'_, AppState>, enabled: bool) -> Result<(), String> {
    helper::set_installed(enabled)?;
    {
        let mut c = state.collector.lock().unwrap();
        c.store.set_meta("per_app", if enabled { "on" } else { "off" });
        c.per_app = enabled;
        c.status.per_app.enabled = enabled;
        c.status.per_app.error = None;
        if !enabled {
            c.tracker = AppTracker::default();
        }
    }
    app.emit("overlook://changed", ()).ok();
    Ok(())
}

#[tauri::command]
fn delete_app_data(app: AppHandle, state: State<AppState>) {
    state.collector.lock().unwrap().store.delete_app_data();
    app.emit("overlook://changed", ()).ok();
}

#[tauri::command]
fn rename_network(app: AppHandle, state: State<AppState>, id: i64, name: Option<String>) {
    state.collector.lock().unwrap().store.rename(id, name.as_deref());
    app.emit("overlook://changed", ()).ok();
}

#[tauri::command]
fn delete_network(app: AppHandle, state: State<AppState>, id: i64) {
    state.collector.lock().unwrap().store.delete_network(id);
    app.emit("overlook://changed", ()).ok();
}

#[tauri::command]
fn delete_all(app: AppHandle, state: State<AppState>) {
    state.collector.lock().unwrap().store.delete_all();
    app.emit("overlook://changed", ()).ok();
}

#[tauri::command]
fn set_retention(app: AppHandle, state: State<AppState>, months: i64) {
    let mut c = state.collector.lock().unwrap();
    c.store.set_meta("retention_months", &months.to_string());
    prune(&mut c.store);
    app.emit("overlook://changed", ()).ok();
}

#[tauri::command]
fn caps(state: State<AppState>) -> Vec<CapUsage> {
    caps::usage(&state.collector.lock().unwrap().store, overlook_core::now())
}

#[tauri::command]
fn save_cap(app: AppHandle, state: State<AppState>, cap: Cap) {
    state.collector.lock().unwrap().store.save_cap(&cap);
    app.emit("overlook://changed", ()).ok();
}

#[tauri::command]
fn delete_cap(app: AppHandle, state: State<AppState>, id: i64) {
    state.collector.lock().unwrap().store.delete_cap(id);
    app.emit("overlook://changed", ()).ok();
}

#[tauri::command]
fn set_upload_alerts(app: AppHandle, state: State<AppState>, on: bool) {
    state.collector.lock().unwrap().store.set_meta("upload_alerts", if on { "1" } else { "0" });
    app.emit("overlook://changed", ()).ok();
}

#[tauri::command]
fn set_tray_rate(app: AppHandle, state: State<AppState>, on: bool) {
    state.collector.lock().unwrap().store.set_meta("tray_rate", if on { "1" } else { "0" });
    state.tray_rate.store(on, Ordering::Relaxed);
    app.emit("overlook://changed", ()).ok();
}

#[tauri::command]
fn request_wifi_names(app: AppHandle) {
    app.run_on_main_thread(system::request_wifi_names).ok();
}

#[tauri::command]
fn reveal_data(state: State<AppState>) {
    tauri_plugin_opener::reveal_item_in_dir(&state.db_path).ok();
}

#[tauri::command]
fn save_export(app: AppHandle, file_name: String, contents: String) -> Result<String, String> {
    let dir = app.path().download_dir().map_err(|e| e.to_string())?;
    let name: String = file_name.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '\0')).collect();
    let (stem, ext) = name.rsplit_once('.').unwrap_or((&name, "csv"));
    let mut path = dir.join(&name);
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem} {n}.{ext}"));
        n += 1;
    }
    std::fs::write(&path, contents).map_err(|e| e.to_string())?;
    tauri_plugin_opener::reveal_item_in_dir(&path).ok();
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
fn show_main(app: AppHandle) {
    show_main_window(&app);
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

/// Bytes per second in a few characters, decimal units like the UI.
fn short_rate(b: f64) -> String {
    let (v, unit) = [(1e9, "G"), (1e6, "M"), (1e3, "K")].into_iter().find(|&(d, _)| b >= d).map_or((0.0, "K"), |(d, u)| (b / d, u));
    if v >= 10.0 || v == 0.0 { format!("{v:.0}{unit}") } else { format!("{v:.1}{unit}") }
}

fn prune(store: &mut Store) {
    let months = retention(store);
    if months > 0 {
        store.prune(overlook_core::now() - months as f64 * 30.44 * 86_400.0);
    }
}

fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("tray") {
        w.hide().ok();
    }
    if let Some(w) = app.get_webview_window("main") {
        w.show().ok();
        w.unminimize().ok();
        w.set_focus().ok();
    }
}

fn migrate_legacy(db: &PathBuf) {
    if db.exists() {
        return;
    }
    let mut sources = vec![];
    if let Some(parent) = db.parent().and_then(|d| d.parent()) {
        sources.push(parent.join("app.tally.desktop/usage.sqlite"));
    }
    if cfg!(target_os = "macos") {
        if let Some(home) = std::env::var_os("HOME") {
            sources.push(PathBuf::from(home).join("Library/Application Support/Tally/usage.sqlite"));
        }
    }
    let Some(legacy) = sources.into_iter().find(|p| p.exists()) else { return };
    for suffix in ["", "-wal", "-shm"] {
        let from = PathBuf::from(format!("{}{suffix}", legacy.display()));
        if from.exists() {
            std::fs::copy(&from, format!("{}{suffix}", db.display())).ok();
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show_main_window(app)))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--background"])))
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let dir = std::env::var("OVERLOOK_DATA_DIR").map(PathBuf::from).unwrap_or(app.path().app_data_dir()?);
            std::fs::create_dir_all(&dir)?;
            let db_path = dir.join("usage.sqlite");
            migrate_legacy(&db_path);
            let mut store = Store::open(&db_path.to_string_lossy())?;
            prune(&mut store);
            let shaped = store.meta("throttled").unwrap_or_default();
            let tray_rate = AtomicBool::new(store.meta("tray_rate").as_deref() == Some("1"));
            let mut collector = Collector::new(store);
            collector.tick();
            app.manage(AppState { collector: Mutex::new(collector), db_path, tray_rate });

            // Enforcement can sit on an admin password prompt, so it gets its own thread and only acts on the latest verdict.
            let (enforce_tx, enforce_rx) = mpsc::channel::<Verdict>();
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut enforcer = Enforcer::restore(&shaped);
                while let Ok(mut verdict) = enforce_rx.recv() {
                    while let Ok(newer) = enforce_rx.try_recv() {
                        verdict = newer;
                    }
                    let before = enforcer.saved();
                    enforcer.apply(&verdict.block, &verdict.throttle);
                    if enforcer.saved() != before {
                        handle.state::<AppState>().collector.lock().unwrap().store.set_meta("throttled", &enforcer.saved());
                    }
                }
            });

            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(5));
                let state = handle.state::<AppState>();
                let verdict = {
                    let mut c = state.collector.lock().unwrap();
                    c.tick().then(|| caps::evaluate(&c.store, &c.status.active, overlook_core::now()))
                };
                if let Some(verdict) = verdict {
                    for n in &verdict.notices {
                        handle.notification().builder().title(&n.title).body(&n.body).show().ok();
                    }
                    enforce_tx.send(verdict).ok();
                    handle.emit("overlook://changed", ()).ok();
                }
            });

            // Live rate: cheap per-second counter diff on the primary interface, never touches the store.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut last: Option<(String, system::Counter, std::time::Instant)> = None;
                let mut shown: Option<String> = None;
                loop {
                    std::thread::sleep(Duration::from_secs(1));
                    let state = handle.state::<AppState>();
                    let primary = {
                        let c = state.collector.lock().unwrap();
                        c.status.active.iter().find(|a| Some(a.network_id) == c.status.primary).map(|a| a.interface.clone())
                    };
                    let sample = primary.and_then(|name| system::counter(&name).map(|now| (name, now, std::time::Instant::now())));
                    let rate = match (&last, &sample) {
                        (Some((prev_name, prev, prev_at)), Some((name, now, at))) => {
                            let secs = at.duration_since(*prev_at).as_secs_f64();
                            (prev_name == name && now.rx >= prev.rx && now.tx >= prev.tx && secs > 0.0)
                                .then(|| ((now.rx - prev.rx) as f64 / secs, (now.tx - prev.tx) as f64 / secs))
                        }
                        _ => None,
                    };
                    last = sample;
                    if let Some(r) = rate {
                        handle.emit("overlook://rate", r).ok();
                    }
                    // Menu-bar text (macOS) / indicator label (Linux); Windows trays have no title.
                    let title = if state.tray_rate.load(Ordering::Relaxed) { rate.map(|(rx, tx)| format!("↓{} ↑{}", short_rate(rx), short_rate(tx))) } else { None };
                    // Only remember what actually reached the tray, so a failed update retries next second.
                    if title != shown && handle.tray_by_id("overlook").is_some_and(|t| t.set_title(title.as_deref()).is_ok()) {
                        shown = title;
                    }
                }
            });

            if std::env::args().any(|a| a == "--background") {
                if let Some(w) = app.get_webview_window("main") {
                    w.hide().ok();
                }
            }

            let open = MenuItem::with_id(app, "open", "Open Overlook", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Overlook", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &quit])?;
            TrayIconBuilder::with_id("overlook")
                .icon(tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?)
                .icon_as_template(true)
                .tooltip("Overlook")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                        let Some(w) = tray.app_handle().get_webview_window("tray") else { return };
                        if w.is_visible().unwrap_or(false) {
                            w.hide().ok();
                        } else {
                            w.move_window(Position::TrayCenter).ok();
                            w.show().ok();
                            w.set_focus().ok();
                        }
                    }
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
                api.prevent_close();
                window.hide().ok();
            }
            WindowEvent::Focused(false) if window.label() == "tray" => {
                window.hide().ok();
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            status, usage, totals, apps, set_per_app, delete_app_data, dropped_bytes, rename_network, delete_network, delete_all,
            set_retention, caps, save_cap, delete_cap, set_upload_alerts, set_tray_rate, request_wifi_names, reveal_data, save_export, show_main, quit
        ])
        .build(tauri::generate_context!())
        .expect("error while building Overlook");

    app.run(|app, event| {
        #[cfg(target_os = "macos")]
        if let RunEvent::Reopen { .. } = event {
            show_main_window(app);
        }
        let _ = (app, event);
    });
}

#[cfg(test)]
mod tests {
    #[test] fn short_rate_stays_compact() {
        let cases = [(0.0, "0K"), (400.0, "0K"), (1_500.0, "1.5K"), (12_345.0, "12K"), (2_340_000.0, "2.3M"), (1.2e9, "1.2G")];
        for (b, s) in cases {
            assert_eq!(super::short_rate(b), s);
        }
    }
}
