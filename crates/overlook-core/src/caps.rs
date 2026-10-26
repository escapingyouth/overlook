use crate::store::{hour_of, Store};
use crate::ActiveNetwork;
use chrono::{Datelike, Days, Local, Months, NaiveDate, TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ponytail: fixed heuristic, make it a setting if people find it too chatty or too quiet.
const UPLOAD_FLOOR: i64 = 250_000_000;
const UPLOAD_FACTOR: i64 = 5;

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
        }
    }
    v
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
        let active = vec![ActiveNetwork { network_id: id, interface: "en0".into(), kind: "wifi".into() }];
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
        let active = vec![ActiveNetwork { network_id: id, interface: "en0".into(), kind: "wifi".into() }];
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

    #[test] fn unusual_upload_alerts_once_per_hour() {
        let s = Store::open(":memory:").unwrap();
        let now = crate::now();
        let net = NetworkInput { key: "wifi:Home", name: "Home", kind: "wifi", ssid: Some("Home"), router_mac: None, interface: "en0" };
        let id = s.resolve_network(&net, now).unwrap();
        let active = vec![ActiveNetwork { network_id: id, interface: "en0".into(), kind: "wifi".into() }];
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
