use crate::apps::SYSTEM_KEY;
use crate::store::{hour_of, Store};
use crate::ActiveNetwork;
use chrono::{Datelike, Days, Local, Months, NaiveDate, TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ponytail: fixed heuristic, make it a setting if people find it too chatty or too quiet.
const UPLOAD_FLOOR: i64 = 250_000_000;
const UPLOAD_FACTOR: i64 = 5;
/// An app needs a day of per-app history before it can look unusual; until then everything is new.
const APP_HISTORY_HOURS: f64 = 24.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cap {
    pub id: i64,
    pub network_id: Option<i64>,
    pub period: String,
    /// Week: first weekday (0 = Sunday). Month: billing day (1–28).
    pub reset: u32,
    pub limit_bytes: i64,
    pub alerts: Vec<u32>,
    pub action: String,
    pub throttle_kbps: i64,
    /// Counts one app's usage instead of the network's. Alerts only; apps can't be blocked or throttled.
    #[serde(default)]
    pub app_id: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapUsage {
    #[serde(flatten)]
    pub cap: Cap,
    pub used: i64,
    pub start: f64,
    pub end: f64,
    pub app_name: Option<String>,
}

#[derive(Debug, PartialEq)]
pub struct Notice {
    pub title: String,
    pub body: String,
}

#[derive(Debug, Default)]
pub struct Verdict {
    pub notices: Vec<Notice>,
    /// (interface, kind) to disconnect.
    pub block: Vec<(String, String)>,
    /// interface → kbit/s.
    pub throttle: HashMap<String, i64>,
}

pub fn period_bounds<Tz: TimeZone>(tz: &Tz, period: &str, reset: u32, now: f64) -> (f64, f64) {
    let today = tz.timestamp_opt(now as i64, 0).single().map(|t| t.date_naive()).unwrap_or_default();
    let (start, end) = match period {
        "day" => (today, today + Days::new(1)),
        "week" => {
            let back = (today.weekday().num_days_from_sunday() + 7 - reset % 7) % 7;
            let s = today - Days::new(back as u64);
            (s, s + Days::new(7))
        }
        _ => {
            let day = reset.clamp(1, 28);
            let this = today.with_day(day).unwrap();
            let s = if today.day() >= day { this } else { this - Months::new(1) };
            (s, s + Months::new(1))
        }
    };
    (midnight(tz, start), midnight(tz, end))
}

fn midnight<Tz: TimeZone>(tz: &Tz, d: NaiveDate) -> f64 {
    // A DST jump can skip midnight; 01:00 exists then.
    [0, 1].iter().find_map(|&h| tz.from_local_datetime(&d.and_hms_opt(h, 0, 0)?).earliest()).map_or(0.0, |t| t.timestamp() as f64)
}

pub fn usage(store: &Store, now: f64) -> Vec<CapUsage> {
    store
        .caps()
        .into_iter()
        .map(|cap| {
            let (start, end) = period_bounds(&Local, &cap.period, cap.reset, now);
            let (from, to) = (hour_of(start), hour_of(now));
            let used = match cap.app_id {
                Some(app) => store.app_bytes(app, cap.network_id, from, to),
                None => store.hours(cap.network_id, from, to).iter().map(|r| r.rx + r.tx).sum(),
            };
            let app_name = cap.app_id.and_then(|id| store.app_name(id));
            CapUsage { cap, used, start, end, app_name }
        })
        .collect()
}

fn period_word(p: &str) -> &'static str {
    match p {
        "day" => "daily",
        "week" => "weekly",
        _ => "monthly",
    }
}

fn gb(b: i64) -> String {
    let v = b as f64 / 1e9;
    if v >= 10.0 { format!("{v:.0} GB") } else if v >= 1.0 { format!("{v:.1} GB") } else { format!("{:.0} MB", b as f64 / 1e6) }
}

pub fn evaluate(store: &Store, active: &[ActiveNetwork], now: f64) -> Verdict {
    let mut v = Verdict::default();
    let names: HashMap<i64, String> = store.networks().into_iter().map(|n| (n.id, n.custom_name.unwrap_or(n.name))).collect();
    let name = |id: Option<i64>| id.map_or("All networks".to_string(), |id| names.get(&id).cloned().unwrap_or("A network".into()));
    let scope = |u: &CapUsage| match (&u.app_name, u.cap.network_id) {
        (Some(app), Some(_)) => format!("{app} on {}", name(u.cap.network_id)),
        (Some(app), None) => app.clone(),
        (None, _) => name(u.cap.network_id),
    };

    for u in usage(store, now) {
        let cap = &u.cap;
        let pct = u.used as f64 * 100.0 / cap.limit_bytes.max(1) as f64;
        let out = pct >= 100.0;
        let crossed: Vec<u32> = cap.alerts.iter().copied().filter(|&a| pct >= a as f64).filter(|a| store.mark_once(&format!("cap:{}:{}:{a}", cap.id, u.start as i64))).collect();
        if let Some(top) = crossed.iter().max() {
            let days = ((u.end - now) / 86_400.0).ceil().max(1.0) as i64;
            let then = match (out, cap.action.as_str()) {
                (true, "block") => " Overlook will keep Wi-Fi disconnected from it until the cap resets.",
                (true, "throttle") => " Overlook is limiting its speed until the cap resets.",
                _ => "",
            };
            v.notices.push(Notice {
                title: format!("{}: {top}% of {} cap used", scope(&u), period_word(&cap.period)),
                body: format!("{} of {} used. Resets in {days} day{}.{then}", gb(u.used), gb(cap.limit_bytes), if days == 1 { "" } else { "s" }),
            });
        }
        if out && cap.app_id.is_none() {
            for a in active.iter().filter(|a| cap.network_id.map_or(true, |id| id == a.network_id)) {
                match cap.action.as_str() {
                    "block" => v.block.push((a.interface.clone(), a.kind.clone())),
                    "throttle" => {
                        let k = v.throttle.entry(a.interface.clone()).or_insert(cap.throttle_kbps);
                        *k = (*k).min(cap.throttle_kbps);
                    }
                    _ => {}
                }
            }
        }
    }

    // One nudge per hotspot, and only while nothing caps it specifically: phone plans run out fast.
    let capped: Vec<i64> = store.caps().iter().filter(|c| c.app_id.is_none()).filter_map(|c| c.network_id).collect();
    for a in active.iter().filter(|a| a.hotspot && !capped.contains(&a.network_id)) {
        if store.mark_once(&format!("hotspot:{}", a.network_id)) {
            v.notices.push(Notice {
                title: format!("{} looks like a phone hotspot", name(Some(a.network_id))),
                body: "Overlook is counting its data separately. Open Overlook to set a cap and get alerts before your plan runs out.".into(),
            });
        }
    }

    if store.meta("weekly_summary").as_deref() != Some("0") {
        v.notices.extend(weekly_summary(store, &Local, now));
    }

    if store.meta("upload_alerts").as_deref() != Some("0") {
        let h = hour_of(now);
        let mut ids: Vec<i64> = active.iter().map(|a| a.network_id).collect();
        ids.dedup();
        for id in ids {
            let rows = store.hours(Some(id), h - 14 * 24, h);
            let past: Vec<i64> = rows.iter().filter(|r| r.hour < h).map(|r| r.tx).collect();
            let current = rows.iter().find(|r| r.hour == h).map_or(0, |r| r.tx);
            let typical = past.iter().sum::<i64>() / past.len().max(1) as i64;
            if current > (typical * UPLOAD_FACTOR).max(UPLOAD_FLOOR) && store.mark_once(&format!("upload:{id}:{h}")) {
                v.notices.push(Notice {
                    title: format!("Unusual upload on {}", name(Some(id))),
                    body: format!("{} uploaded this hour, vs about {} in a typical hour.", gb(current), gb(typical)),
                });
            }
            app_spikes(store, &mut v, id, &name(Some(id)), h);
        }
    }
    v
}

/// Last week in one notification, from 09:00 on Monday (or whenever Overlook next runs that week).
// ponytail: weeks start Monday for everyone; pass the UI's locale week start if Sunday-first users ask.
fn weekly_summary<Tz: TimeZone>(store: &Store, tz: &Tz, now: f64) -> Option<Notice> {
    let (this_week, _) = period_bounds(tz, "week", 1, now);
    let (last_week, _) = period_bounds(tz, "week", 1, this_week - 1.0);
    let (week_before, _) = period_bounds(tz, "week", 1, last_week - 1.0);
    if now < this_week + 9.0 * 3600.0 {
        return None;
    }
    let (from, to) = (hour_of(last_week), hour_of(this_week) - 1);
    let sum = |from, to| store.hours(None, from, to).iter().map(|r| r.rx + r.tx).sum::<i64>();
    let total = sum(from, to);
    if total == 0 || !store.mark_once(&format!("summary:{}", last_week as i64)) {
        return None;
    }

    let mut parts = vec![];
    let previous = sum(hour_of(week_before), from - 1);
    if previous > 0 && !store.coverage(hour_of(week_before), from - 1).is_empty() {
        let change = (total - previous) as f64 / previous as f64;
        parts.push(match change {
            c if c.abs() < 0.05 => "About the same as the week before.".to_string(),
            c => format!("{} {:.0}% on the week before.", if c > 0.0 { "Up" } else { "Down" }, c.abs() * 100.0),
        });
    }
    let names: HashMap<i64, String> = store.networks().into_iter().map(|n| (n.id, n.custom_name.unwrap_or(n.name))).collect();
    let used: Vec<_> = store.totals_by_network(from, to).into_iter().filter(|t| t.rx + t.tx > 0).collect();
    if let Some(top) = used.iter().max_by_key(|t| t.rx + t.tx) {
        let name = names.get(&top.network_id).map_or("one network", String::as_str);
        parts.push(if used.len() == 1 { format!("All on {name}.") } else { format!("Most on {name} ({}).", gb(top.rx + top.tx)) });
    }
    // Turning per-app off keeps its history, but the summary should follow the setting.
    let per_app = store.meta("per_app").as_deref() == Some("on");
    if let Some(app) = store.apps_report(None, from, to).apps.into_iter().find(|a| per_app && a.key != SYSTEM_KEY && a.rx + a.tx > 0) {
        parts.push(format!("Top app: {} ({}).", app.name, gb(app.rx + app.tx)));
    }
    Some(Notice { title: format!("Last week: {} used", gb(total)), body: parts.join(" ") })
}

/// An app using far more this hour than its typical hour on this network (or a lot, for an app new here).
/// Once per app, network and day, so a long download alerts once rather than every hour.
fn app_spikes(store: &Store, v: &mut Verdict, network: i64, network_name: &str, h: i64) {
    let history = store.app_covered_hours(h - 14 * 24, h - 1);
    if history < APP_HISTORY_HOURS {
        return;
    }
    let past = store.apps_report(Some(network), h - 14 * 24, h - 1).apps;
    let local_day = Local.timestamp_opt(h * 3600, 0).single().map_or(h.div_euclid(24).to_string(), |t| t.date_naive().to_string());
    for app in store.apps_report(Some(network), h, h).apps.into_iter().filter(|a| a.key != SYSTEM_KEY) {
        let current = app.rx + app.tx;
        let typical = past.iter().find(|p| p.id == app.id).map_or(0, |p| ((p.rx + p.tx) as f64 / history) as i64);
        if current > (typical * UPLOAD_FACTOR).max(UPLOAD_FLOOR) && store.mark_once(&format!("app:{}:{network}:{local_day}", app.id)) {
            let usual = if typical > 0 { format!("vs about {} in a typical hour", gb(typical)) } else { "and it rarely uses data here".into() };
            v.notices.push(Notice {
                title: format!("{} is using a lot of data", app.name),
                body: format!("{} on {network_name} this hour, {usual}.", gb(current)),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processor::UsageDelta;
    use crate::store::NetworkInput;
    use chrono::Utc;

    fn ts(y: i32, m: u32, d: u32) -> f64 {
        Utc.with_ymd_and_hms(y, m, d, 12, 0, 0).unwrap().timestamp() as f64
    }

    #[test] fn periods_follow_reset_days() {
        let day = |y, m, d| Utc.with_ymd_and_hms(y, m, d, 0, 0, 0).unwrap().timestamp() as f64;
        // 2026-10-26 is a Monday.
        assert_eq!(period_bounds(&Utc, "day", 0, ts(2026, 10, 26)), (day(2026, 10, 26), day(2026, 10, 27)));
        assert_eq!(period_bounds(&Utc, "week", 0, ts(2026, 10, 26)), (day(2026, 10, 25), day(2026, 11, 1)));
        assert_eq!(period_bounds(&Utc, "week", 1, ts(2026, 10, 26)), (day(2026, 10, 26), day(2026, 11, 2)));
        assert_eq!(period_bounds(&Utc, "month", 1, ts(2026, 10, 26)), (day(2026, 10, 1), day(2026, 11, 1)));
        assert_eq!(period_bounds(&Utc, "month", 28, ts(2026, 10, 26)), (day(2026, 9, 28), day(2026, 10, 28)));
        assert_eq!(period_bounds(&Utc, "month", 15, ts(2027, 1, 20)), (day(2027, 1, 15), day(2027, 2, 15)));
    }

    #[test] fn alerts_fire_once_and_caps_enforce() {
        let s = Store::open(":memory:").unwrap();
        let now = crate::now();
        let net = NetworkInput { key: "wifi:Hotspot", name: "Hotspot", kind: "wifi", ssid: Some("Hotspot"), router_mac: None, interface: "en0" };
        let id = s.resolve_network(&net, now).unwrap();
        let active = vec![ActiveNetwork { network_id: id, interface: "en0".into(), kind: "wifi".into(), hotspot: false }];
        s.add(&UsageDelta { network_id: id, rx: 850_000_000, tx: 0, estimated: false, at: now });
        s.save_cap(&Cap { id: 0, network_id: Some(id), period: "day".into(), reset: 0, limit_bytes: 1_000_000_000, alerts: vec![80, 100], action: "block".into(), throttle_kbps: 1000, app_id: None });

        let v = evaluate(&s, &active, now);
        assert_eq!(v.notices.len(), 1);
        assert!(v.notices[0].title.contains("80%"));
        assert!(v.block.is_empty());
        assert!(evaluate(&s, &active, now).notices.is_empty());

        s.add(&UsageDelta { network_id: id, rx: 200_000_000, tx: 0, estimated: false, at: now });
        let v = evaluate(&s, &active, now);
        assert!(v.notices[0].title.contains("100%"));
        assert_eq!(v.block, vec![("en0".to_string(), "wifi".to_string())]);
    }

    #[test] fn app_caps_count_one_app_and_only_alert() {
        let s = Store::open(":memory:").unwrap();
        let now = crate::now();
        let net = NetworkInput { key: "wifi:Hotspot", name: "Hotspot", kind: "wifi", ssid: Some("Hotspot"), router_mac: None, interface: "en0" };
        let id = s.resolve_network(&net, now).unwrap();
        let active = vec![ActiveNetwork { network_id: id, interface: "en0".into(), kind: "wifi".into(), hotspot: false }];
        let dropbox = s.insert_app("/Applications/Dropbox.app", "Dropbox", None);
        let other = s.insert_app("/Applications/Safari.app", "Safari", None);
        s.add(&UsageDelta { network_id: id, rx: 3_000_000_000, tx: 0, estimated: false, at: now });
        s.add_app_usage(id, other, now, 2_000_000_000, 0);
        s.add_app_usage(id, dropbox, now, 600_000_000, 500_000_000);
        // Even if a stale client sends "block", an app cap must never disconnect the network.
        s.save_cap(&Cap { id: 0, network_id: Some(id), period: "day".into(), reset: 0, limit_bytes: 1_000_000_000, alerts: vec![100], action: "block".into(), throttle_kbps: 1000, app_id: Some(dropbox) });

        let u = &usage(&s, now)[0];
        assert_eq!((u.used, u.app_name.as_deref(), u.cap.action.as_str()), (1_100_000_000, Some("Dropbox"), "none"));
        let v = evaluate(&s, &active, now);
        assert_eq!(v.notices[0].title, "Dropbox on Hotspot: 100% of daily cap used");
        assert!(v.block.is_empty() && v.throttle.is_empty());
    }

    #[test] fn old_databases_gain_app_caps() {
        let path = std::env::temp_dir().join(format!("overlook-migrate-{}.sqlite", std::process::id()));
        rusqlite::Connection::open(&path).unwrap().execute_batch(
            "CREATE TABLE caps (id INTEGER PRIMARY KEY, network_id INTEGER, period TEXT NOT NULL, reset INTEGER NOT NULL,
             limit_bytes INTEGER NOT NULL, alerts TEXT NOT NULL, action TEXT NOT NULL, throttle_kbps INTEGER NOT NULL);
             INSERT INTO caps VALUES (1, NULL, 'day', 0, 100, '80', 'none', 1000);").unwrap();
        let caps = Store::open(&path.to_string_lossy()).unwrap().caps();
        Store::open(&path.to_string_lossy()).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!((caps.len(), caps[0].app_id), (1, None));
    }

    #[test] fn a_reused_cap_id_alerts_again() {
        let mut s = Store::open(":memory:").unwrap();
        let now = crate::now();
        let net = NetworkInput { key: "wifi:Hotspot", name: "Hotspot", kind: "wifi", ssid: Some("Hotspot"), router_mac: None, interface: "en0" };
        let id = s.resolve_network(&net, now).unwrap();
        let active = vec![ActiveNetwork { network_id: id, interface: "en0".into(), kind: "wifi".into(), hotspot: false }];
        let app = s.insert_app("/Applications/Dropbox.app", "Dropbox", None);
        s.add_app_usage(id, app, now, 900, 0);
        let cap = Cap { id: 0, network_id: None, period: "day".into(), reset: 0, limit_bytes: 1000, alerts: vec![80], action: "none".into(), throttle_kbps: 1000, app_id: Some(app) };
        s.save_cap(&cap);
        assert_eq!(evaluate(&s, &active, now).notices.len(), 1);

        s.delete_app_data();
        let app = s.insert_app("/Applications/Dropbox.app", "Dropbox", None);
        s.add_app_usage(id, app, now, 900, 0);
        s.save_cap(&Cap { app_id: Some(app), ..cap });
        assert_eq!(s.caps()[0].id, 1, "SQLite reuses the deleted id");
        assert_eq!(evaluate(&s, &active, now).notices.len(), 1);
    }

    #[test] fn hotspots_without_a_cap_nudge_once() {
        let s = Store::open(":memory:").unwrap();
        let now = crate::now();
        let net = |key, name| NetworkInput { key, name, kind: "wifi", ssid: Some(name), router_mac: None, interface: "en0" };
        let phone = s.resolve_network(&net("wifi:Sam’s iPhone", "Sam’s iPhone"), now).unwrap();
        let capped = s.resolve_network(&net("wifi:Pixel", "Pixel"), now).unwrap();
        s.save_cap(&Cap { id: 0, network_id: Some(capped), period: "month".into(), reset: 1, limit_bytes: 1_000_000_000, alerts: vec![], action: "none".into(), throttle_kbps: 1000, app_id: None });
        let active = |id| ActiveNetwork { network_id: id, interface: "en0".into(), kind: "wifi".into(), hotspot: true };

        let v = evaluate(&s, &[active(phone), active(capped)], now);
        assert_eq!(v.notices.len(), 1);
        assert_eq!(v.notices[0].title, "Sam’s iPhone looks like a phone hotspot");
        assert!(evaluate(&s, &[active(phone)], now).notices.is_empty());
    }

    #[test] fn app_spikes_alert_once_a_day_after_a_day_of_history() {
        let s = Store::open(":memory:").unwrap();
        let h = hour_of(crate::now());
        let at = |hour: i64| hour as f64 * 3600.0 + 60.0;
        let net = NetworkInput { key: "wifi:Home", name: "Home", kind: "wifi", ssid: Some("Home"), router_mac: None, interface: "en0" };
        let id = s.resolve_network(&net, at(h)).unwrap();
        let active = vec![ActiveNetwork { network_id: id, interface: "en0".into(), kind: "wifi".into(), hotspot: false }];
        let steady = s.insert_app("/Applications/Spotify.app", "Spotify", None);
        let backup = s.insert_app("/Applications/Backblaze.app", "Backblaze", None);
        let system = s.insert_app(SYSTEM_KEY, "System", None);
        let busy_hour = |s: &Store| {
            s.add_app_usage(id, steady, at(h), 400_000_000, 0); // 400 MB, but Spotify always does about that
            s.add_app_usage(id, backup, at(h), 0, 300_000_000); // 300 MB from an app that's usually idle here
            s.add_app_usage(id, system, at(h), 900_000_000, 0); // System is the unattributed rest, never singled out
        };

        // Brand-new per-app tracking: everything would look unusual, so nothing alerts.
        busy_hour(&s);
        assert!(evaluate(&s, &active, at(h)).notices.is_empty());

        for past in h - 30..h {
            s.add_app_coverage(past as f64 * 3600.0, (past + 1) as f64 * 3600.0);
            s.add_app_usage(id, steady, at(past), 350_000_000, 0);
        }
        let v = evaluate(&s, &active, at(h));
        assert_eq!(v.notices.len(), 1);
        assert_eq!(v.notices[0].title, "Backblaze is using a lot of data");
        assert_eq!(v.notices[0].body, "300 MB on Home this hour, and it rarely uses data here.");
        assert!(evaluate(&s, &active, at(h)).notices.is_empty());
    }

    #[test] fn weekly_summary_compares_weeks_and_names_where_it_went() {
        let s = Store::open(":memory:").unwrap();
        let day = |d: u32, h: u32| Utc.with_ymd_and_hms(2026, 10, d, h, 0, 0).unwrap().timestamp() as f64;
        let net = |key, name| NetworkInput { key, name, kind: "wifi", ssid: Some(name), router_mac: None, interface: "en0" };
        let home = s.resolve_network(&net("wifi:Home", "Home"), day(12, 8)).unwrap();
        let cafe = s.resolve_network(&net("wifi:Cafe", "Cafe"), day(12, 8)).unwrap();
        let steam = s.insert_app("/Applications/Steam.app", "Steam", None);
        // 2026-10-19 and 10-26 are Mondays: the week before is Oct 12–18, last week Oct 19–25.
        s.add_coverage(day(14, 10), day(14, 11));
        s.add(&UsageDelta { network_id: home, rx: 20_000_000_000, tx: 0, estimated: false, at: day(14, 10) });
        s.add(&UsageDelta { network_id: home, rx: 18_000_000_000, tx: 1_000_000_000, estimated: false, at: day(21, 10) });
        s.add(&UsageDelta { network_id: cafe, rx: 4_000_000_000, tx: 0, estimated: false, at: day(25, 15) });
        s.add_app_usage(home, steam, day(21, 10), 9_000_000_000, 0);
        s.set_meta("per_app", "on");

        assert_eq!(weekly_summary(&s, &Utc, day(26, 8)), None, "waits for 09:00");
        let n = weekly_summary(&s, &Utc, day(26, 9)).unwrap();
        assert_eq!(n.title, "Last week: 23 GB used");
        assert_eq!(n.body, "Up 15% on the week before. Most on Home (19 GB). Top app: Steam (9.0 GB).");
        assert_eq!(weekly_summary(&s, &Utc, day(27, 9)), None, "once a week");

        // Per-app turned off: its history stays, but the next summary leaves the app out.
        s.set_meta("per_app", "off");
        s.add(&UsageDelta { network_id: home, rx: 1_000_000_000, tx: 0, estimated: false, at: day(28, 10) });
        s.add_app_usage(home, steam, day(28, 10), 1_000_000_000, 0);
        let n = weekly_summary(&s, &Utc, Utc.with_ymd_and_hms(2026, 11, 2, 9, 0, 0).unwrap().timestamp() as f64).unwrap();
        assert!(!n.body.contains("Steam"), "{}", n.body);
    }

    #[test] fn weekly_summary_skips_an_empty_week_and_untracked_comparisons() {
        let s = Store::open(":memory:").unwrap();
        let day = |d: u32, h: u32| Utc.with_ymd_and_hms(2026, 10, d, h, 0, 0).unwrap().timestamp() as f64;
        assert_eq!(weekly_summary(&s, &Utc, day(26, 12)), None);
        let net = NetworkInput { key: "wifi:Home", name: "Home", kind: "wifi", ssid: Some("Home"), router_mac: None, interface: "en0" };
        let id = s.resolve_network(&net, day(28, 12)).unwrap();
        s.add(&UsageDelta { network_id: id, rx: 2_000_000_000, tx: 0, estimated: false, at: day(28, 12) });
        let n = weekly_summary(&s, &Utc, Utc.with_ymd_and_hms(2026, 11, 2, 10, 0, 0).unwrap().timestamp() as f64).unwrap();
        assert_eq!(n.body, "All on Home.");
    }

    #[test] fn unusual_upload_alerts_once_per_hour() {
        let s = Store::open(":memory:").unwrap();
        let now = crate::now();
        let net = NetworkInput { key: "wifi:Home", name: "Home", kind: "wifi", ssid: Some("Home"), router_mac: None, interface: "en0" };
        let id = s.resolve_network(&net, now).unwrap();
        let active = vec![ActiveNetwork { network_id: id, interface: "en0".into(), kind: "wifi".into(), hotspot: false }];
        s.set_meta("weekly_summary", "0"); // the 48 h of history can fall in last week, depending on today's date
        for h in 1..48 {
            s.add(&UsageDelta { network_id: id, rx: 0, tx: 20_000_000, estimated: false, at: now - h as f64 * 3600.0 });
        }
        s.add(&UsageDelta { network_id: id, rx: 0, tx: 200_000_000, estimated: false, at: now });
        assert!(evaluate(&s, &active, now).notices.is_empty());
        s.add(&UsageDelta { network_id: id, rx: 0, tx: 100_000_000, estimated: false, at: now });
        assert_eq!(evaluate(&s, &active, now).notices.len(), 1);
        assert!(evaluate(&s, &active, now).notices.is_empty());
    }
}
