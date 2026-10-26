use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};

pub const SYSTEM_KEY: &str = "system";

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ProcSample { pub pid: u32, pub path: String, pub rx: u64, pub tx: u64 }

/// The JSON the helper writes.
#[derive(Deserialize)]
pub struct Snapshot { pub session: String, pub at: f64, pub procs: Vec<ProcSample> }

const SYSTEM_PREFIXES: [&str; 8] = ["/system/", "/usr/sbin/", "/usr/libexec/", "/sbin/", "/bin/", "/library/apple/", "/lib/systemd/", "/usr/lib/systemd/"];

pub fn app_key(path: &str) -> String {
    let lower = path.to_lowercase();
    if path.is_empty() || lower.starts_with("c:\\windows\\") || SYSTEM_PREFIXES.iter().any(|p| lower.starts_with(p)) {
        return SYSTEM_KEY.into();
    }
    if path.contains('\\') {
        return lower;
    }
    match path.find(".app/") {
        Some(i) => path[..i + 4].into(),
        None => path.into(),
    }
}

pub fn default_name(key: &str) -> String {
    let file = key.rsplit(['/', '\\']).next().unwrap_or(key);
    let lower = file.to_lowercase();
    let n = if lower.ends_with(".app") || lower.ends_with(".exe") { file.len() - 4 } else { file.len() };
    file[..n].into()
}

/// `nettop -P -L 1 -x` CSV rows: `name.pid,bytes_in,bytes_out,`.
pub fn parse_nettop(csv: &str) -> Vec<(u32, u64, u64)> {
    csv.lines()
        .filter_map(|line| {
            let mut f: Vec<&str> = line.split(',').collect();
            while f.last() == Some(&"") { f.pop(); }
            let tx = f.pop()?.trim().parse().ok()?;
            let rx = f.pop()?.trim().parse().ok()?;
            let pid = f.join(",").rsplit_once('.')?.1.trim().parse().ok()?;
            Some((pid, rx, tx))
        })
        .collect()
}

#[derive(Default)]
pub struct AppTracker {
    procs: HashMap<u32, (String, u64, u64)>,
    at: Option<f64>,
    network: Option<i64>,
    session: Option<String>,
}

impl AppTracker {
    /// `now` of the stored baseline; None before the first sample.
    pub fn last_at(&self) -> Option<f64> { self.at }

    pub fn sample(&mut self, session: &str, procs: Vec<ProcSample>, network: Option<i64>, now: f64, gap: f64) -> Option<Vec<(String, u64, u64)>> {
        let attributable = network.is_some()
            && network == self.network
            && self.session.as_deref() == Some(session)
            && self.at.is_some_and(|at| now - at <= gap);
        let mut out: BTreeMap<&str, (u64, u64)> = BTreeMap::new();
        if attributable {
            for p in &procs {
                // ponytail: a pid first seen mid-interval is counted from zero; bytes from processes that start and exit between samples are lost, upgrade by having the source report exits.
                let (rx, tx) = match self.procs.get(&p.pid) {
                    Some((path, rx, tx)) if *path == p.path => (p.rx.saturating_sub(*rx), p.tx.saturating_sub(*tx)),
                    _ => (p.rx, p.tx),
                };
                let e = out.entry(&p.path).or_default();
                e.0 += rx;
                e.1 += tx;
            }
        }
        let result = attributable.then(|| out.into_iter().filter(|(_, (rx, tx))| rx + tx > 0).map(|(k, (rx, tx))| (k.to_string(), rx, tx)).collect());
        self.procs = procs.into_iter().map(|p| (p.pid, (p.path, p.rx, p.tx))).collect();
        self.at = Some(now);
        self.network = network;
        self.session = Some(session.into());
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: f64 = 180.0;
    fn t(pid: u32, path: &str, rx: u64, tx: u64) -> ProcSample { ProcSample { pid, path: path.into(), rx, tx } }

    #[test] fn mac_helpers_group_under_outer_bundle() {
        assert_eq!(app_key("/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Helpers/Google Chrome Helper (Renderer).app/Contents/MacOS/Google Chrome Helper (Renderer)"), "/Applications/Google Chrome.app");
        assert_eq!(default_name("/Applications/Google Chrome.app"), "Google Chrome");
    }
    #[test] fn os_paths_are_system() {
        for p in ["", "/usr/sbin/mDNSResponder", "/System/Library/PrivateFrameworks/X", "C:\\Windows\\System32\\svchost.exe", "/lib/systemd/systemd-resolved", "/usr/lib/systemd/systemd-resolved"] { assert_eq!(app_key(p), SYSTEM_KEY, "{p}"); }
    }
    #[test] fn windows_and_linux_keys() {
        assert_eq!(app_key("C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"), "c:\\program files\\google\\chrome\\application\\chrome.exe");
        assert_eq!(default_name("c:\\program files\\steam\\steam.exe"), "steam");
        assert_eq!(app_key("/usr/bin/firefox"), "/usr/bin/firefox"); // /usr/bin is NOT system: Linux apps live there
    }
    #[test] fn nettop_names_with_dots_and_commas() {
        let csv = ",bytes_in,bytes_out,\nCode Helper (Plugin).812,10,20,\nFoo, Inc..44,5,6,\nlaunchd.1,0,0,\n";
        assert_eq!(parse_nettop(csv), vec![(812, 10, 20), (44, 5, 6), (1, 0, 0)]);
    }
    #[test] fn tracker_first_sample_is_baseline() {
        let mut k = AppTracker::default();
        assert_eq!(k.last_at(), None);
        assert_eq!(k.sample("s", vec![t(1, "/a", 100, 10)], Some(1), 0.0, G), None);
        assert_eq!(k.last_at(), Some(0.0));
    }
    #[test] fn tracker_continuous_delta() {
        let mut k = AppTracker::default();
        k.sample("s", vec![t(1, "/a", 100, 10)], Some(1), 0.0, G);
        assert_eq!(k.sample("s", vec![t(1, "/a", 300, 15)], Some(1), 30.0, G), Some(vec![("/a".into(), 200, 5)]));
    }
    #[test] fn tracker_counter_decrease_counts_zero() {
        let mut k = AppTracker::default();
        k.sample("s", vec![t(1, "/a", 300, 10)], Some(1), 0.0, G);
        assert_eq!(k.sample("s", vec![t(1, "/a", 100, 10)], Some(1), 30.0, G), Some(vec![]));
    }
    #[test] fn tracker_new_pid_counted_fully() {
        let mut k = AppTracker::default();
        k.sample("s", vec![t(1, "/a", 1, 1)], Some(1), 0.0, G);
        assert_eq!(k.sample("s", vec![t(1, "/a", 1, 1), t(9, "/b", 50, 5)], Some(1), 30.0, G), Some(vec![("/b".into(), 50, 5)]));
    }
    #[test] fn tracker_pid_reuse_is_new_process() {
        let mut k = AppTracker::default();
        k.sample("s", vec![t(7, "/a", 1000, 0)], Some(1), 0.0, G);
        assert_eq!(k.sample("s", vec![t(7, "/b", 40, 0)], Some(1), 30.0, G), Some(vec![("/b".into(), 40, 0)]));
    }
    #[test] fn tracker_gap_network_change_session_change_drop() {
        let p = || vec![t(1, "/a", 1, 1)];
        let mut k = AppTracker::default();
        k.sample("s1", p(), Some(1), 0.0, G);
        assert_eq!(k.sample("s1", p(), Some(1), 3600.0, G), None);
        assert_eq!(k.sample("s1", p(), Some(2), 3630.0, G), None);
        assert_eq!(k.sample("s2", p(), Some(2), 3660.0, G), None);
        assert_eq!(k.sample("s2", p(), None, 3690.0, G), None);
        assert!(k.sample("s2", p(), Some(2), 3720.0, G).is_none()); // previous had no network
        assert!(k.sample("s2", p(), Some(2), 3750.0, G).is_some());
    }
}
