
pub mod apps;
pub mod caps;
pub mod enforce;
pub mod processor;
pub mod store;
pub mod system;

use apps::{app_key, AppTracker, Snapshot, SYSTEM_KEY};
use processor::{Baseline, Outcome, Processor};
use serde::Serialize;
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use store::{NetworkInput, Store};

pub const SAMPLE_INTERVAL: f64 = 30.0;

pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fingerprint {
    pub interface: String,
    pub kind: String,
    pub ssid: Option<String>,
    pub router_mac: Option<String>,
}

impl Fingerprint {
    pub fn key(&self) -> Option<String> {
        self.ssid.as_ref().map(|s| format!("wifi:{s}")).or_else(|| self.router_mac.as_ref().map(|m| format!("router:{m}")))
    }

    pub fn default_name(&self) -> String {
        if let Some(ssid) = &self.ssid {
            return ssid.clone();
        }
        let suffix = self.router_mac.as_ref().map(|m| format!(" · {}", m[m.len().saturating_sub(5)..].to_uppercase())).unwrap_or_default();
        match self.kind.as_str() {
            "wifi" => format!("Wi-Fi Network{suffix}"),
            "ethernet" => format!("Ethernet Network{suffix}"),
            _ => format!("Network{suffix}"),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveNetwork {
    pub network_id: i64,
    pub interface: String,
    pub kind: String,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStatus {
    pub active: Vec<ActiveNetwork>,
    pub primary: Option<i64>,
    pub rate: Option<(f64, f64)>,
    pub last_sample: f64,
    pub per_app: PerAppStatus,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerAppStatus {
    pub enabled: bool,
    pub error: Option<String>,
    pub experimental: bool,
}

/// Feeds one process snapshot into the store. Returns the error to show, if any.
pub fn sample_apps(store: &Store, tracker: &mut AppTracker, snapshot: Result<Snapshot, String>, network: Option<i64>, now: f64, gap: f64) -> Option<String> {
    let snap = match snapshot {
        Ok(s) => s,
        Err(e) => return Some(e),
    };
    let previous = tracker.last_at();
    let Some(deltas) = tracker.sample(&snap.session, snap.procs, network, now, gap) else { return None };
    if let Some(prev) = previous {
        store.add_app_coverage(prev, now);
    }
    let mut grouped: HashMap<String, (u64, u64)> = HashMap::new();
    for (path, rx, tx) in deltas {
        let e = grouped.entry(app_key(&path)).or_default();
        e.0 += rx;
        e.1 += tx;
    }
    for (key, (rx, tx)) in grouped {
        let id = store.app_id(&key).unwrap_or_else(|| {
            let (name, icon) = if key == SYSTEM_KEY { ("System".into(), None) } else { system::describe_app(&key) };
            store.insert_app(&key, &name, icon.as_deref())
        });
        if let Some(n) = network {
            store.add_app_usage(n, id, now, rx, tx);
        }
    }
    None
}

pub struct Collector {
    pub store: Store,
    processor: Processor,
    ssid_cache: HashMap<String, (Option<String>, Instant)>,
    mac_cache: HashMap<String, String>,
    pub status: LiveStatus,
    pub per_app: bool,
    pub tracker: AppTracker,
    last_keys: Vec<Option<String>>,
    last_full_sample: Option<Instant>,
}

impl Collector {
    pub fn new(store: Store) -> Collector {
        let per_app = store.meta("per_app").as_deref() == Some("on");
        Collector {
            per_app,
            tracker: AppTracker::default(),
            store,
            processor: Processor { sample_interval: SAMPLE_INTERVAL },
            ssid_cache: HashMap::new(),
            mac_cache: HashMap::new(),
            status: LiveStatus::default(),
            last_keys: vec![],
            last_full_sample: None,
        }
    }

    pub fn tick(&mut self) -> bool {
        let interfaces: Vec<_> = netdev::get_interfaces().into_iter().filter(system::is_physical).collect();
        let fingerprints = self.fingerprints(&interfaces);
        let keys: Vec<_> = fingerprints.iter().map(Fingerprint::key).collect();
        let due = self.last_full_sample.map_or(true, |t| t.elapsed() >= Duration::from_secs_f64(SAMPLE_INTERVAL));
        if !due && keys == self.last_keys {
            return false;
        }
        self.last_keys = keys;
        self.last_full_sample = Some(Instant::now());
        self.sample(&interfaces, &fingerprints, now());
        true
    }

    fn fingerprints(&mut self, interfaces: &[netdev::Interface]) -> Vec<Fingerprint> {
        interfaces
            .iter()
            .filter_map(|i| {
                let gateway = i.gateway.as_ref()?;
                let ip = gateway.ipv4.first().map(|a| a.to_string()).or_else(|| gateway.ipv6.first().map(|a| a.to_string()))?;
                let router_mac = system::normalize_mac(&gateway.mac_addr.to_string()).or_else(|| self.cached_mac(&ip, &i.name));
                let wifi = system::is_wifi(i);
                let ssid = if wifi { self.cached_ssid(i, router_mac.as_deref()) } else { None };
                Some(Fingerprint { interface: i.name.clone(), kind: if wifi { "wifi" } else { "ethernet" }.into(), ssid, router_mac })
            })
            .collect()
    }

    fn cached_mac(&mut self, ip: &str, interface: &str) -> Option<String> {
        let key = format!("{interface}|{ip}");
        if let Some(mac) = self.mac_cache.get(&key) {
            return Some(mac.clone());
        }
        let mac = system::neighbor_mac(ip, interface)?;
        self.mac_cache.insert(key, mac.clone());
        Some(mac)
    }

    fn cached_ssid(&mut self, i: &netdev::Interface, router: Option<&str>) -> Option<String> {
        let key = format!("{}|{}", i.name, router.unwrap_or(""));
        if let Some((ssid, at)) = self.ssid_cache.get(&key) {
            if at.elapsed() < Duration::from_secs(120) {
                return ssid.clone();
            }
        }
        let ssid = system::ssid(i);
        self.ssid_cache.insert(key, (ssid.clone(), Instant::now()));
        ssid
    }

    pub fn sample(&mut self, interfaces: &[netdev::Interface], fingerprints: &[Fingerprint], now: f64) {
        let boot = system::boot_time();
        let counters = system::counters(interfaces);
        let primary_iface = interfaces.iter().find(|i| i.default).map(|i| i.name.clone());
        let processor = &self.processor;
        let tracker = &mut self.tracker;
        let per_app = self.per_app;
        let snapshot = per_app.then(system::process_snapshot); // up to 2 s: run before the transaction opens
        let mut status = LiveStatus { last_sample: now, per_app: PerAppStatus { enabled: per_app, error: None, experimental: cfg!(not(target_os = "macos")) }, ..Default::default() };

        self.store.transaction(|store| {
            for fp in fingerprints {
                let Some(key) = fp.key() else { continue };
                let name = fp.default_name();
                let input = NetworkInput { key: &key, name: &name, kind: &fp.kind, ssid: fp.ssid.as_deref(), router_mac: fp.router_mac.as_deref(), interface: &fp.interface };
                if let Ok(id) = store.resolve_network(&input, now) {
                    status.active.push(ActiveNetwork { network_id: id, interface: fp.interface.clone(), kind: fp.kind.clone() });
                }
            }
            let primary = status.active.iter().find(|a| Some(&a.interface) == primary_iface.as_ref()).or(status.active.first()).cloned();
            status.primary = primary.as_ref().map(|p| p.network_id);

            for iface in interfaces {
                let Some(c) = counters.get(&iface.name) else { continue };
                let previous = store.baseline(&iface.name);
                let network = status.active.iter().find(|a| a.interface == iface.name).map(|a| a.network_id);
                if previous.is_none() && network.is_none() && c.rx == 0 && c.tx == 0 {
                    continue;
                }
                let (outcome, next) = processor.process(previous.as_ref(), &iface.name, c.rx, c.tx, boot, network, now);
                match &outcome {
                    Outcome::Usage(d) => store.add(d),
                    Outcome::Dropped { bytes, reason } => store.record_dropped(*bytes, reason.as_str(), now),
                    _ => {}
                }
                if let (Some(p), Some(prev)) = (&primary, &previous) {
                    status.rate = status.rate.or_else(|| live_rate(p, iface, prev, c, boot, now, processor));
                }
                store.save_baseline(&next);
            }

            if let Some(snapshot) = snapshot {
                status.per_app.error = sample_apps(store, tracker, snapshot, status.primary, now, processor.gap_threshold());
            }

            if let Some(last) = store.meta("last_sample").and_then(|v| v.parse::<f64>().ok()) {
                if now > last && now - last <= processor.gap_threshold() {
                    store.add_coverage(last, now);
                }
            }
            store.set_meta("last_sample", &now.to_string());
        });
        self.status = status;
    }
}

fn live_rate(primary: &ActiveNetwork, iface: &netdev::Interface, prev: &Baseline, c: &system::Counter, boot: i64, now: f64, p: &Processor) -> Option<(f64, f64)> {
    let elapsed = now - prev.at;
    (primary.interface == iface.name && prev.boot_time == boot && c.rx >= prev.rx && c.tx >= prev.tx && elapsed > 0.5 && elapsed <= p.gap_threshold())
        .then(|| ((c.rx - prev.rx) as f64 / elapsed, (c.tx - prev.tx) as f64 / elapsed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(at: f64, rx: u64, tx: u64) -> Snapshot {
        Snapshot { session: "s".into(), at, procs: vec![apps::ProcSample { pid: 7, path: "/Applications/Steam.app/Contents/MacOS/steam_osx".into(), rx, tx }] }
    }

    #[test] fn sample_apps_attributes_after_baseline() {
        let store = Store::open(":memory:").unwrap();
        let (t, gap) = (36000.0, 180.0);
        let mut tr = AppTracker::default();
        assert_eq!(sample_apps(&store, &mut tr, Ok(snap(t, 0, 0)), Some(1), t, gap), None);
        assert!(store.apps_report(None, 0, 100).apps.is_empty());
        assert_eq!(sample_apps(&store, &mut tr, Ok(snap(t + 30.0, 5000, 100)), Some(1), t + 30.0, gap), None);
        let r = store.apps_report(None, 0, 100);
        assert_eq!((r.apps.len(), r.apps[0].name.as_str(), r.apps[0].rx, r.apps[0].tx), (1, "Steam", 5000, 100));
        assert_eq!(r.since_hour, Some(10));
        assert_eq!(store.coverage(0, 100), vec![]); // network coverage is untouched; app coverage is separate
    }

    #[test] fn per_app_failure_keeps_network_collection() {
        let mut store = Store::open(":memory:").unwrap();
        let mut tr = AppTracker::default();
        store.transaction(|s| {
            let err = sample_apps(s, &mut tr, Err("Per-app helper isn't running".into()), Some(1), 36000.0, 180.0);
            assert_eq!(err.as_deref(), Some("Per-app helper isn't running"));
            s.add(&processor::UsageDelta { network_id: 1, at: 36000.0, rx: 10, tx: 5, estimated: false });
        });
        assert!(store.apps_report(None, 0, 100).apps.is_empty());
        assert!(!store.hours(None, 0, 100).is_empty());
    }

    #[test] fn fingerprint_never_keys_on_interface_alone() {
        let fp = Fingerprint { interface: "en0".into(), kind: "wifi".into(), ssid: None, router_mac: None };
        assert_eq!(fp.key(), None);
        let fp = Fingerprint { ssid: Some("Home".into()), router_mac: Some("aa:bb:cc:dd:ee:ff".into()), ..fp };
        assert_eq!(fp.key().as_deref(), Some("wifi:Home"));
    }

    #[test] fn default_name_uses_router_suffix() {
        let fp = Fingerprint { interface: "en5".into(), kind: "ethernet".into(), ssid: None, router_mac: Some("aa:bb:cc:dd:ee:ff".into()) };
        assert_eq!(fp.default_name(), "Ethernet Network · EE:FF");
    }

    #[test] fn live_collection_identifies_a_network() {
        let mut c = Collector::new(Store::open(":memory:").unwrap());
        c.tick();
        let interfaces: Vec<_> = netdev::get_interfaces().into_iter().filter(system::is_physical).collect();
        let fps = c.fingerprints(&interfaces);
        c.sample(&interfaces, &fps, now() + 1.0);
        eprintln!("fingerprints: {fps:?}\nstatus: {:?}", c.status);
        if fps.iter().any(|f| f.key().is_some()) {
            assert!(!c.status.active.is_empty());
            assert!(c.store.first_covered_hour().is_some());
        }
    }
}
