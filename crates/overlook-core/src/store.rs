
use crate::caps::Cap;
use crate::processor::{Baseline, UsageDelta};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

pub fn hour_of(seconds: f64) -> i64 {
    (seconds / 3600.0).floor() as i64
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Network {
    pub id: i64,
    pub key: String,
    pub name: String,
    pub custom_name: Option<String>,
    pub kind: String,
    pub ssid: Option<String>,
    pub router_mac: Option<String>,
    pub first_seen: f64,
    pub last_seen: f64,
    pub last_interface: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HourRow {
    pub hour: i64,
    pub rx: i64,
    pub tx: i64,
    pub estimated_rx: i64,
    pub estimated_tx: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkTotal {
    pub network_id: i64,
    pub rx: i64,
    pub tx: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppTotal {
    pub id: i64,
    pub key: String,
    pub name: String,
    pub icon: Option<String>,
    pub rx: i64,
    pub tx: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppsReport {
    pub apps: Vec<AppTotal>,
    pub covered_rx: i64,
    pub covered_tx: i64,
    pub since_hour: Option<i64>,
}

pub struct Store {
    db: Connection,
}

pub struct NetworkInput<'a> {
    pub key: &'a str,
    pub name: &'a str,
    pub kind: &'a str,
    pub ssid: Option<&'a str>,
    pub router_mac: Option<&'a str>,
    pub interface: &'a str,
}

impl Store {
    pub fn open(path: &str) -> rusqlite::Result<Store> {
        let db = Connection::open(path)?;
        db.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS networks (
                 id INTEGER PRIMARY KEY, key TEXT NOT NULL UNIQUE, name TEXT NOT NULL, custom_name TEXT,
                 kind TEXT NOT NULL, ssid TEXT, router_mac TEXT,
                 first_seen REAL NOT NULL, last_seen REAL NOT NULL, last_interface TEXT);
             CREATE TABLE IF NOT EXISTS usage (
                 network_id INTEGER NOT NULL, hour INTEGER NOT NULL,
                 rx INTEGER NOT NULL DEFAULT 0, tx INTEGER NOT NULL DEFAULT 0,
                 est_rx INTEGER NOT NULL DEFAULT 0, est_tx INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY (network_id, hour)) WITHOUT ROWID;
             CREATE INDEX IF NOT EXISTS usage_hour ON usage(hour);
             CREATE TABLE IF NOT EXISTS coverage (hour INTEGER PRIMARY KEY, seconds REAL NOT NULL);
             CREATE TABLE IF NOT EXISTS apps (
                 id INTEGER PRIMARY KEY, key TEXT NOT NULL UNIQUE, name TEXT NOT NULL, icon TEXT);
             CREATE TABLE IF NOT EXISTS app_usage (
                 network_id INTEGER NOT NULL, hour INTEGER NOT NULL, app_id INTEGER NOT NULL,
                 rx INTEGER NOT NULL DEFAULT 0, tx INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY (network_id, hour, app_id)) WITHOUT ROWID;
             CREATE INDEX IF NOT EXISTS app_usage_hour ON app_usage(hour);
             CREATE TABLE IF NOT EXISTS app_coverage (hour INTEGER PRIMARY KEY, seconds REAL NOT NULL);
             CREATE TABLE IF NOT EXISTS baselines (
                 interface TEXT PRIMARY KEY, boot INTEGER NOT NULL, rx INTEGER NOT NULL, tx INTEGER NOT NULL,
                 network_id INTEGER, at REAL NOT NULL);
             CREATE TABLE IF NOT EXISTS dropped (id INTEGER PRIMARY KEY, at REAL NOT NULL, bytes INTEGER NOT NULL, reason TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS caps (
                 id INTEGER PRIMARY KEY, network_id INTEGER, period TEXT NOT NULL, reset INTEGER NOT NULL,
                 limit_bytes INTEGER NOT NULL, alerts TEXT NOT NULL, action TEXT NOT NULL, throttle_kbps INTEGER NOT NULL, app_id INTEGER);",
        )?;
        // Databases from before per-app caps.
        if db.prepare("SELECT app_id FROM caps LIMIT 0").is_err() {
            db.execute("ALTER TABLE caps ADD COLUMN app_id INTEGER", [])?;
        }
        Ok(Store { db })
    }

    pub fn transaction<T>(&mut self, body: impl FnOnce(&Store) -> T) -> T {
        self.db.execute_batch("BEGIN").ok();
        let result = body(self);
        self.db.execute_batch("COMMIT").ok();
        result
    }

    pub fn resolve_network(&self, n: &NetworkInput, now: f64) -> rusqlite::Result<i64> {
        let mut id: Option<i64> = self.db.query_row("SELECT id FROM networks WHERE key = ?", [n.key], |r| r.get(0)).optional()?;
        if id.is_none() {
            if let Some(mac) = n.router_mac {
                if n.ssid.is_some() {
                    id = self.db.query_row("SELECT id FROM networks WHERE key = ? AND ssid IS NULL", [format!("router:{mac}")], |r| r.get(0)).optional()?;
                    if let Some(id) = id {
                        self.db.execute("UPDATE networks SET key = ?, name = ?, ssid = ? WHERE id = ?", params![n.key, n.name, n.ssid, id])?;
                    }
                } else {
                    id = self.db.query_row("SELECT id FROM networks WHERE router_mac = ? ORDER BY last_seen DESC LIMIT 1", [mac], |r| r.get(0)).optional()?;
                }
            }
        }
        if let Some(id) = id {
            self.db.execute(
                "UPDATE networks SET last_seen = ?, last_interface = ?, router_mac = COALESCE(?, router_mac) WHERE id = ?",
                params![now, n.interface, n.router_mac, id],
            )?;
            return Ok(id);
        }
        self.db.execute(
            "INSERT INTO networks (key, name, kind, ssid, router_mac, first_seen, last_seen, last_interface) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![n.key, n.name, n.kind, n.ssid, n.router_mac, now, now, n.interface],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    pub fn networks(&self) -> Vec<Network> {
        let mut s = self.db.prepare("SELECT id, key, name, custom_name, kind, ssid, router_mac, first_seen, last_seen, last_interface FROM networks ORDER BY last_seen DESC").unwrap();
        s.query_map([], |r| {
            Ok(Network {
                id: r.get(0)?, key: r.get(1)?, name: r.get(2)?, custom_name: r.get(3)?, kind: r.get(4)?,
                ssid: r.get(5)?, router_mac: r.get(6)?, first_seen: r.get(7)?, last_seen: r.get(8)?, last_interface: r.get(9)?,
            })
        })
        .unwrap()
        .filter_map(Result::ok)
        .collect()
    }

    pub fn rename(&self, id: i64, name: Option<&str>) {
        let name = name.map(str::trim).filter(|s| !s.is_empty());
        self.db.execute("UPDATE networks SET custom_name = ? WHERE id = ?", params![name, id]).ok();
    }

    pub fn delete_network(&mut self, id: i64) {
        self.transaction(|s| {
            s.db.execute("DELETE FROM usage WHERE network_id = ?", [id]).ok();
            s.db.execute("DELETE FROM app_usage WHERE network_id = ?", [id]).ok();
            s.db.execute("DELETE FROM networks WHERE id = ?", [id]).ok();
            s.db.execute("DELETE FROM caps WHERE network_id = ?", [id]).ok();
            s.forget_deleted_cap_alerts();
            s.db.execute("UPDATE baselines SET network_id = NULL WHERE network_id = ?", [id]).ok();
        });
    }

    pub fn add(&self, d: &UsageDelta) {
        let (rx, tx) = (d.rx.min(i64::MAX as u64) as i64, d.tx.min(i64::MAX as u64) as i64);
        let (erx, etx) = if d.estimated { (rx, tx) } else { (0, 0) };
        self.db.execute(
            "INSERT INTO usage (network_id, hour, rx, tx, est_rx, est_tx) VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(network_id, hour) DO UPDATE SET rx = rx + excluded.rx, tx = tx + excluded.tx,
               est_rx = est_rx + excluded.est_rx, est_tx = est_tx + excluded.est_tx",
            params![d.network_id, hour_of(d.at), rx, tx, erx, etx],
        )
        .ok();
    }

    pub fn hours(&self, network: Option<i64>, from_hour: i64, to_hour: i64) -> Vec<HourRow> {
        let map = |r: &rusqlite::Row| Ok(HourRow { hour: r.get(0)?, rx: r.get(1)?, tx: r.get(2)?, estimated_rx: r.get(3)?, estimated_tx: r.get(4)? });
        match network {
            None => {
                let mut s = self.db.prepare("SELECT hour, SUM(rx), SUM(tx), SUM(est_rx), SUM(est_tx) FROM usage WHERE hour BETWEEN ? AND ? GROUP BY hour ORDER BY hour").unwrap();
                s.query_map([from_hour, to_hour], map).unwrap().filter_map(Result::ok).collect()
            }
            Some(id) => {
                let mut s = self.db.prepare("SELECT hour, rx, tx, est_rx, est_tx FROM usage WHERE hour BETWEEN ? AND ? AND network_id = ? ORDER BY hour").unwrap();
                s.query_map([from_hour, to_hour, id], map).unwrap().filter_map(Result::ok).collect()
            }
        }
    }

    pub fn totals_by_network(&self, from_hour: i64, to_hour: i64) -> Vec<NetworkTotal> {
        let mut s = self.db.prepare("SELECT network_id, SUM(rx), SUM(tx) FROM usage WHERE hour BETWEEN ? AND ? GROUP BY network_id").unwrap();
        s.query_map([from_hour, to_hour], |r| Ok(NetworkTotal { network_id: r.get(0)?, rx: r.get(1)?, tx: r.get(2)? }))
            .unwrap()
            .filter_map(Result::ok)
            .collect()
    }

    pub fn add_coverage(&self, from: f64, to: f64) {
        self.add_coverage_to("coverage", from, to);
    }

    pub fn add_app_coverage(&self, from: f64, to: f64) {
        self.add_coverage_to("app_coverage", from, to);
    }

    fn add_coverage_to(&self, table: &str, from: f64, to: f64) {
        let sql = format!("INSERT INTO {table} (hour, seconds) VALUES (?, ?) ON CONFLICT(hour) DO UPDATE SET seconds = MIN(3600, seconds + excluded.seconds)");
        let mut cursor = from;
        while cursor < to {
            let hour = hour_of(cursor);
            let end = to.min((hour + 1) as f64 * 3600.0);
            self.db.execute(&sql, params![hour, end - cursor]).ok();
            cursor = end;
        }
    }

    pub fn app_id(&self, key: &str) -> Option<i64> {
        self.db.query_row("SELECT id FROM apps WHERE key = ?", [key], |r| r.get(0)).optional().ok().flatten()
    }

    pub fn insert_app(&self, key: &str, name: &str, icon: Option<&str>) -> i64 {
        self.db.execute("INSERT OR IGNORE INTO apps (key, name, icon) VALUES (?, ?, ?)", params![key, name, icon]).ok();
        self.app_id(key).unwrap_or(0)
    }

    pub fn add_app_usage(&self, network_id: i64, app_id: i64, at: f64, rx: u64, tx: u64) {
        let (rx, tx) = (rx.min(i64::MAX as u64) as i64, tx.min(i64::MAX as u64) as i64);
        self.db.execute(
            "INSERT INTO app_usage (network_id, hour, app_id, rx, tx) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(network_id, hour, app_id) DO UPDATE SET rx = rx + excluded.rx, tx = tx + excluded.tx",
            params![network_id, hour_of(at), app_id, rx, tx],
        )
        .ok();
    }

    pub fn apps_report(&self, network: Option<i64>, from_hour: i64, to_hour: i64) -> AppsReport {
        // network_id = ?3 OR ?3 IS NULL covers both the single-network and all-network cases
        let mut s = self
            .db
            .prepare(
                "SELECT a.id, a.key, a.name, a.icon, SUM(u.rx), SUM(u.tx) FROM app_usage u JOIN apps a ON a.id = u.app_id
                 WHERE u.hour BETWEEN ?1 AND ?2 AND (u.network_id = ?3 OR ?3 IS NULL)
                 GROUP BY a.id ORDER BY SUM(u.rx) + SUM(u.tx) DESC",
            )
            .unwrap();
        let apps = s
            .query_map(params![from_hour, to_hour, network], |r| {
                Ok(AppTotal { id: r.get(0)?, key: r.get(1)?, name: r.get(2)?, icon: r.get(3)?, rx: r.get(4)?, tx: r.get(5)? })
            })
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        let (covered_rx, covered_tx) = self
            .db
            .query_row(
                "SELECT COALESCE(SUM(rx), 0), COALESCE(SUM(tx), 0) FROM usage
                 WHERE hour BETWEEN ?1 AND ?2 AND (network_id = ?3 OR ?3 IS NULL) AND hour IN (SELECT hour FROM app_coverage)",
                params![from_hour, to_hour, network],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((0, 0));
        let since_hour = self.db.query_row("SELECT MIN(hour) FROM app_coverage", [], |r| r.get(0)).ok().flatten();
        AppsReport { apps, covered_rx, covered_tx, since_hour }
    }

    pub fn app_bytes(&self, app_id: i64, network: Option<i64>, from_hour: i64, to_hour: i64) -> i64 {
        self.db
            .query_row(
                "SELECT COALESCE(SUM(rx + tx), 0) FROM app_usage WHERE app_id = ?1 AND hour BETWEEN ?2 AND ?3 AND (network_id = ?4 OR ?4 IS NULL)",
                params![app_id, from_hour, to_hour, network],
                |r| r.get(0),
            )
            .unwrap_or(0)
    }

    pub fn app_name(&self, id: i64) -> Option<String> {
        self.db.query_row("SELECT name FROM apps WHERE id = ?", [id], |r| r.get(0)).optional().ok().flatten()
    }

    pub fn delete_app_data(&mut self) {
        self.transaction(|s| {
            s.db.execute("DELETE FROM caps WHERE app_id IS NOT NULL", []).ok();
            s.forget_deleted_cap_alerts();
            for table in ["app_usage", "app_coverage", "apps"] {
                s.db.execute(&format!("DELETE FROM {table}"), []).ok();
            }
        });
    }

    pub fn coverage(&self, from_hour: i64, to_hour: i64) -> Vec<(i64, f64)> {
        let mut s = self.db.prepare("SELECT hour, seconds FROM coverage WHERE hour BETWEEN ? AND ?").unwrap();
        s.query_map([from_hour, to_hour], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().filter_map(Result::ok).collect()
    }

    /// Hours of per-app collection in a range, partial hours counted by their share.
    pub fn app_covered_hours(&self, from_hour: i64, to_hour: i64) -> f64 {
        self.db
            .query_row("SELECT COALESCE(SUM(seconds), 0) / 3600.0 FROM app_coverage WHERE hour BETWEEN ? AND ?", [from_hour, to_hour], |r| r.get(0))
            .unwrap_or(0.0)
    }

    pub fn first_covered_hour(&self) -> Option<i64> {
        self.db.query_row("SELECT MIN(hour) FROM coverage", [], |r| r.get(0)).ok().flatten()
    }

    pub fn baseline(&self, interface: &str) -> Option<Baseline> {
        self.db
            .query_row("SELECT boot, rx, tx, network_id, at FROM baselines WHERE interface = ?", [interface], |r| {
                Ok(Baseline {
                    interface: interface.to_string(),
                    boot_time: r.get(0)?,
                    rx: r.get::<_, i64>(1)? as u64,
                    tx: r.get::<_, i64>(2)? as u64,
                    network_id: r.get(3)?,
                    at: r.get(4)?,
                })
            })
            .optional()
            .ok()
            .flatten()
    }

    pub fn save_baseline(&self, b: &Baseline) {
        self.db
            .execute(
                "INSERT OR REPLACE INTO baselines (interface, boot, rx, tx, network_id, at) VALUES (?, ?, ?, ?, ?, ?)",
                params![b.interface, b.boot_time, b.rx as i64, b.tx as i64, b.network_id, b.at],
            )
            .ok();
    }

    pub fn record_dropped(&self, bytes: u64, reason: &str, at: f64) {
        self.db.execute("INSERT INTO dropped (at, bytes, reason) VALUES (?, ?, ?)", params![at, bytes.min(i64::MAX as u64) as i64, reason]).ok();
    }

    pub fn dropped_bytes(&self, since: f64) -> i64 {
        self.db.query_row("SELECT COALESCE(SUM(bytes), 0) FROM dropped WHERE at >= ?", [since], |r| r.get(0)).unwrap_or(0)
    }

    pub fn meta(&self, key: &str) -> Option<String> {
        self.db.query_row("SELECT value FROM meta WHERE key = ?", [key], |r| r.get(0)).optional().ok().flatten()
    }

    pub fn set_meta(&self, key: &str, value: &str) {
        self.db.execute("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", [key, value]).ok();
    }

    /// True the first time a key is seen; used so each alert fires once.
    pub fn mark_once(&self, key: &str) -> bool {
        self.db.execute("INSERT OR IGNORE INTO meta (key, value) VALUES (?, '1')", [format!("sent:{key}")]).map_or(false, |n| n == 1)
    }

    pub fn caps(&self) -> Vec<Cap> {
        let mut s = self.db.prepare("SELECT id, network_id, period, reset, limit_bytes, alerts, action, throttle_kbps, app_id FROM caps ORDER BY id").unwrap();
        s.query_map([], |r| {
            let alerts: String = r.get(5)?;
            Ok(Cap {
                id: r.get(0)?, network_id: r.get(1)?, period: r.get(2)?, reset: r.get(3)?, limit_bytes: r.get(4)?,
                alerts: alerts.split(',').filter_map(|a| a.parse().ok()).collect(), action: r.get(6)?, throttle_kbps: r.get(7)?,
                app_id: r.get(8)?,
            })
        })
        .unwrap()
        .filter_map(Result::ok)
        .collect()
    }

    pub fn save_cap(&self, c: &Cap) {
        let alerts = c.alerts.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        let id = (c.id > 0).then_some(c.id);
        let action = if c.app_id.is_some() { "none" } else { c.action.as_str() };
        // A changed limit or threshold should be able to alert again this period.
        self.db.execute("DELETE FROM meta WHERE key LIKE ?", [format!("sent:cap:{}:%", c.id)]).ok();
        self.db
            .execute(
                "INSERT OR REPLACE INTO caps (id, network_id, period, reset, limit_bytes, alerts, action, throttle_kbps, app_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![id, c.network_id, c.period, c.reset, c.limit_bytes.max(1), alerts, action, c.throttle_kbps.max(64), c.app_id],
            )
            .ok();
    }

    pub fn delete_cap(&self, id: i64) {
        self.db.execute("DELETE FROM caps WHERE id = ?", [id]).ok();
        self.db.execute("DELETE FROM meta WHERE key LIKE ?", [format!("sent:cap:{id}:%")]).ok();
    }

    /// SQLite reuses a deleted cap's id, so its sent-alert keys would silence the next cap's alerts.
    fn forget_deleted_cap_alerts(&self) {
        self.db
            .execute("DELETE FROM meta WHERE key LIKE 'sent:cap:%' AND NOT EXISTS (SELECT 1 FROM caps WHERE meta.key LIKE 'sent:cap:' || caps.id || ':%')", [])
            .ok();
    }

    pub fn prune(&mut self, before: f64) {
        let hour = hour_of(before);
        self.transaction(|s| {
            s.db.execute("DELETE FROM usage WHERE hour < ?", [hour]).ok();
            s.db.execute("DELETE FROM coverage WHERE hour < ?", [hour]).ok();
            s.db.execute("DELETE FROM app_usage WHERE hour < ?", [hour]).ok();
            s.db.execute("DELETE FROM app_coverage WHERE hour < ?", [hour]).ok();
            s.db.execute("DELETE FROM dropped WHERE at < ?", [before]).ok();
        });
    }

    pub fn delete_all(&mut self) {
        self.transaction(|s| {
            for table in ["usage", "coverage", "app_usage", "app_coverage", "apps", "networks", "baselines", "dropped"] {
                s.db.execute(&format!("DELETE FROM {table}"), []).ok();
            }
            s.db.execute("DELETE FROM caps WHERE network_id IS NOT NULL OR app_id IS NOT NULL", []).ok();
            s.forget_deleted_cap_alerts();
        });
        self.db.execute_batch("VACUUM").ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store { Store::open(":memory:").unwrap() }
    fn net<'a>(key: &'a str, ssid: Option<&'a str>, mac: &'a str) -> NetworkInput<'a> {
        NetworkInput { key, name: ssid.unwrap_or("Network"), kind: "wifi", ssid, router_mac: Some(mac), interface: "en0" }
    }

    #[test] fn hourly_upsert_accumulates_and_separates_networks() {
        let s = store();
        let now = 1_800_000_000.0;
        let home = s.resolve_network(&net("wifi:Home", Some("Home"), "aa"), now).unwrap();
        let office = s.resolve_network(&net("router:bb", None, "bb"), now).unwrap();
        assert_ne!(home, office);
        s.add(&UsageDelta { network_id: home, rx: 100, tx: 10, estimated: false, at: now });
        s.add(&UsageDelta { network_id: home, rx: 50, tx: 5, estimated: true, at: now + 60.0 });
        s.add(&UsageDelta { network_id: office, rx: 7, tx: 7, estimated: false, at: now });
        let h = hour_of(now);
        assert_eq!(s.hours(Some(home), h - 1, h + 1), vec![HourRow { hour: h, rx: 150, tx: 15, estimated_rx: 50, estimated_tx: 5 }]);
        assert_eq!(s.hours(None, h - 1, h + 1)[0].rx, 157);
        assert_eq!(s.totals_by_network(h, h).iter().find(|t| t.network_id == office).unwrap().tx, 7);
    }

    #[test] fn network_gaining_ssid_keeps_history() {
        let s = store();
        let anon = s.resolve_network(&net("router:aa", None, "aa"), 1.0).unwrap();
        let named = s.resolve_network(&net("wifi:Cafe", Some("Cafe"), "aa"), 2.0).unwrap();
        assert_eq!(anon, named);
        let again = s.resolve_network(&net("router:aa", None, "aa"), 3.0).unwrap();
        assert_eq!(again, named);
        assert_eq!(s.networks().len(), 1);
        assert_eq!(s.networks()[0].name, "Cafe");
    }

    #[test] fn different_ssids_on_same_router_stay_separate() {
        let s = store();
        let a = s.resolve_network(&net("wifi:Home", Some("Home"), "aa"), 1.0).unwrap();
        let b = s.resolve_network(&net("wifi:Guest", Some("Guest"), "aa"), 1.0).unwrap();
        assert_ne!(a, b);
    }

    #[test] fn coverage_splits_hours_and_caps() {
        let s = store();
        let start = 1_800_000_000.0 - (1_800_000_000.0f64 % 3600.0) + 3500.0;
        s.add_coverage(start, start + 200.0);
        let h = hour_of(start);
        let cov: std::collections::HashMap<_, _> = s.coverage(h, h + 1).into_iter().collect();
        assert_eq!(cov[&h], 100.0);
        assert_eq!(cov[&(h + 1)], 100.0);
        s.add_coverage(start - 7200.0, start - 3500.0);
        s.add_coverage(start - 7200.0, start - 3500.0);
        assert_eq!(s.coverage(h - 1, h - 1)[0].1, 3600.0);
    }

    #[test] fn baseline_round_trips_large_counters() {
        let s = store();
        let b = Baseline { interface: "en0".into(), boot_time: 5, rx: u64::MAX - 10, tx: 3, network_id: None, at: 1000.0 };
        s.save_baseline(&b);
        assert_eq!(s.baseline("en0"), Some(b));
    }

    #[test] fn app_usage_upserts_and_sorts_per_network() {
        let s = store();
        let (a, b) = (s.insert_app("/A.app", "A", None), s.insert_app("/B.app", "B", Some("icon")));
        assert_eq!(s.app_id("/A.app"), Some(a));
        assert_eq!(s.app_id("/nope"), None);
        let t = 1_800_000_000.0;
        let h = hour_of(t);
        s.add_app_usage(1, a, t, 100, 10);
        s.add_app_usage(1, a, t, 100, 10);
        s.add_app_usage(1, b, t, 500, 0);
        s.add_app_usage(2, a, t, 7, 0);
        let r = s.apps_report(Some(1), h, h);
        let got: Vec<_> = r.apps.iter().map(|x| (x.key.as_str(), x.rx, x.tx)).collect();
        assert_eq!(got, vec![("/B.app", 500, 0), ("/A.app", 200, 20)]);
        assert_eq!(r.apps[0].icon.as_deref(), Some("icon"));
        let all = s.apps_report(None, h, h);
        let a_tot = all.apps.iter().find(|x| x.id == a).unwrap();
        assert_eq!(a_tot.rx + a_tot.tx, 227);
        assert_eq!(all.apps.iter().find(|x| x.id == b).unwrap().rx, 500);
    }

    #[test] fn delete_network_removes_its_app_usage() {
        let mut s = store();
        let a = s.insert_app("/A.app", "A", None);
        let t = 1_800_000_000.0;
        let h = hour_of(t);
        s.add_app_usage(1, a, t, 100, 10);
        s.add_app_usage(2, a, t, 7, 0);
        s.delete_network(1);
        assert!(s.apps_report(Some(1), h, h).apps.is_empty());
        let all = s.apps_report(None, h, h);
        assert_eq!(all.apps.iter().map(|x| x.rx).collect::<Vec<_>>(), vec![7]);
    }

    #[test] fn covered_totals_only_count_app_covered_hours() {
        let s = store();
        let h0 = hour_of(1_800_000_000.0);
        let h1 = h0 + 1;
        for (h, rx) in [(h0, 1000), (h1, 300)] {
            s.add(&UsageDelta { network_id: 1, rx, tx: 0, estimated: false, at: h as f64 * 3600.0 });
        }
        s.add_app_coverage(h1 as f64 * 3600.0, h1 as f64 * 3600.0 + 60.0);
        let r = s.apps_report(Some(1), h0, h1);
        assert_eq!((r.covered_rx, r.since_hour), (300, Some(h1)));
        let r = s.apps_report(None, h0, h1);
        assert_eq!((r.covered_rx, r.since_hour), (300, Some(h1)));
    }

    #[test] fn prune_and_delete_app_data_clear_app_tables() {
        let mut s = store();
        let t = 1_800_000_000.0;
        let h = hour_of(t);
        let fill = |s: &Store| {
            let a = s.insert_app("/A.app", "A", None);
            s.add_app_usage(1, a, t, 1, 1);
            s.add_app_coverage(t, t + 10.0);
        };
        fill(&s);
        s.prune(t + 7200.0);
        let r = s.apps_report(None, h - 1, h + 1);
        assert!(r.apps.is_empty() && r.since_hour.is_none());
        s.delete_app_data();
        s.delete_app_data();
        s.add_app_coverage(t, t + 10.0);
        s.delete_app_data();
        assert_eq!(s.app_id("/A.app"), None);
        fill(&s);
        s.delete_app_data();
        let r = s.apps_report(None, h - 1, h + 1);
        assert!(r.apps.is_empty() && r.since_hour.is_none());
        assert_eq!(s.app_id("/A.app"), None);
    }
}
