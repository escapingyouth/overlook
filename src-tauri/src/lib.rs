use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tally_core::store::{HourRow, Network, NetworkTotal, Store};
use tally_core::system::{self, WifiNameAccess};
use tally_core::{Collector, LiveStatus, SAMPLE_INTERVAL};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WindowEvent};
use tauri_plugin_positioner::{Position, WindowExt};

struct AppState {
    collector: Mutex<Collector>,
    db_path: PathBuf,
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
fn rename_network(app: AppHandle, state: State<AppState>, id: i64, name: Option<String>) {
    state.collector.lock().unwrap().store.rename(id, name.as_deref());
    app.emit("tally://changed", ()).ok();
}

#[tauri::command]
fn delete_network(app: AppHandle, state: State<AppState>, id: i64) {
    state.collector.lock().unwrap().store.delete_network(id);
    app.emit("tally://changed", ()).ok();
}

#[tauri::command]
fn delete_all(app: AppHandle, state: State<AppState>) {
    state.collector.lock().unwrap().store.delete_all();
    app.emit("tally://changed", ()).ok();
}

#[tauri::command]
fn set_retention(app: AppHandle, state: State<AppState>, months: i64) {
    let mut c = state.collector.lock().unwrap();
    c.store.set_meta("retention_months", &months.to_string());
    prune(&mut c.store);
    app.emit("tally://changed", ()).ok();
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
fn show_main(app: AppHandle) {
    show_main_window(&app);
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

fn prune(store: &mut Store) {
    let months = retention(store);
    if months > 0 {
        store.prune(tally_core::now() - months as f64 * 30.44 * 86_400.0);
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
    if db.exists() || !cfg!(target_os = "macos") {
        return;
    }
    let Some(home) = std::env::var_os("HOME") else { return };
    let legacy = PathBuf::from(home).join("Library/Application Support/Tally/usage.sqlite");
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
        .setup(|app| {
            let dir = std::env::var("TALLY_DATA_DIR").map(PathBuf::from).unwrap_or(app.path().app_data_dir()?);
            std::fs::create_dir_all(&dir)?;
            let db_path = dir.join("usage.sqlite");
            migrate_legacy(&db_path);
            let mut store = Store::open(&db_path.to_string_lossy())?;
            prune(&mut store);
            let mut collector = Collector::new(store);
            collector.tick();
            app.manage(AppState { collector: Mutex::new(collector), db_path });

            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(5));
                let state = handle.state::<AppState>();
                let sampled = state.collector.lock().unwrap().tick();
                if sampled {
                    handle.emit("tally://changed", ()).ok();
                }
            });

            if std::env::args().any(|a| a == "--background") {
                if let Some(w) = app.get_webview_window("main") {
                    w.hide().ok();
                }
            }

            let open = MenuItem::with_id(app, "open", "Open Tally", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Tally", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &quit])?;
            TrayIconBuilder::with_id("tally")
                .icon(tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?)
                .icon_as_template(true)
                .tooltip("Tally")
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
            status, usage, totals, dropped_bytes, rename_network, delete_network, delete_all,
            set_retention, request_wifi_names, reveal_data, show_main, quit
        ])
        .build(tauri::generate_context!())
        .expect("error while building Tally");

    app.run(|app, event| {
        #[cfg(target_os = "macos")]
        if let RunEvent::Reopen { .. } = event {
            show_main_window(app);
        }
        let _ = (app, event);
    });
}
