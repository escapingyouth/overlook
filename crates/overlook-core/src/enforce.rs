//! What happens when a cap runs out. Blocking disconnects the adapter and needs no privileges;
//! throttling shapes the adapter's traffic and asks for administrator permission each time it changes.
use std::collections::{HashMap, HashSet};
use std::process::Command;

pub const CAN_THROTTLE: bool = cfg!(any(target_os = "macos", target_os = "linux"));

fn run(cmd: &mut Command) -> bool {
    cmd.status().map_or(false, |s| s.success())
}

/// Interface names come from the OS, but they end up in a root shell, so be strict.
#[cfg_attr(target_os = "windows", allow(dead_code))]
fn safe(interface: &str) -> bool {
    !interface.is_empty() && interface.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

#[cfg(target_os = "macos")]
pub fn disconnect(interface: &str, kind: &str) -> bool {
    use objc2_core_wlan::CWWiFiClient;
    use objc2_foundation::NSString;
    kind == "wifi"
        && objc2::rc::autoreleasepool(|_| unsafe {
            let Some(wifi) = CWWiFiClient::sharedWiFiClient().interfaceWithName(Some(&NSString::from_str(interface))) else { return false };
            wifi.disassociate();
            true
        })
}

#[cfg(target_os = "windows")]
pub fn disconnect(interface: &str, kind: &str) -> bool {
    use std::os::windows::process::CommandExt;
    let Some(name) = netdev::get_interfaces().into_iter().find(|i| i.name == interface).and_then(|i| i.friendly_name) else { return false };
    kind == "wifi" && run(Command::new("netsh").args(["wlan", "disconnect", &format!("interface={name}")]).creation_flags(0x0800_0000))
}

#[cfg(target_os = "linux")]
pub fn disconnect(interface: &str, _kind: &str) -> bool {
    run(Command::new("nmcli").args(["device", "disconnect", interface]))
}

#[cfg(target_os = "macos")]
fn as_admin(script: &str) -> bool {
    let quoted = script.replace('\\', "\\\\").replace('"', "\\\"");
    run(Command::new("osascript").args(["-e", &format!("do shell script \"{quoted}\" with administrator privileges")]))
}

#[cfg(target_os = "linux")]
fn as_admin(script: &str) -> bool {
    run(Command::new("pkexec").args(["sh", "-c", script]))
}

// macOS: a dummynet pipe in a com.apple/* sub-anchor, which the stock pf.conf already loads.
#[cfg(target_os = "macos")]
fn shape_script(interface: &str, kbps: Option<i64>) -> String {
    let anchor = format!("com.apple/overlook-{interface}");
    match kbps {
        Some(k) => format!(
            "/usr/sbin/dnctl pipe 61043 config bw {k}Kbit/s && printf 'dummynet in on {interface} all pipe 61043\\ndummynet out on {interface} all pipe 61043\\n' | /sbin/pfctl -a {anchor} -f - && (/sbin/pfctl -E 2>/dev/null; true)"
        ),
        None => format!("/sbin/pfctl -a {anchor} -F all 2>/dev/null; true"),
    }
}

// Linux: token bucket on egress, policing on ingress.
#[cfg(target_os = "linux")]
fn shape_script(interface: &str, kbps: Option<i64>) -> String {
    let clear = format!("tc qdisc del dev {interface} root 2>/dev/null; tc qdisc del dev {interface} ingress 2>/dev/null");
    match kbps {
        Some(k) => format!(
            "{clear}; tc qdisc add dev {interface} root tbf rate {k}kbit burst 32kbit latency 400ms && tc qdisc add dev {interface} handle ffff: ingress && tc filter add dev {interface} parent ffff: protocol all u32 match u32 0 0 police rate {k}kbit burst 64k drop flowid :1"
        ),
        None => format!("{clear}; true"),
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn shape(interface: &str, kbps: Option<i64>) -> bool {
    safe(interface) && as_admin(&shape_script(interface, kbps))
}

// ponytail: Windows only offers outbound QoS throttling without a driver, so throttle is macOS/Linux only.
#[cfg(target_os = "windows")]
fn shape(_interface: &str, _kbps: Option<i64>) -> bool {
    false
}

/// Marking a connection metered makes the OS hold off on updates and cloud sync over it.
/// macOS has no public way to set Low Data Mode, though it already treats phone hotspots as expensive.
pub const CAN_MARK_METERED: bool = cfg!(any(target_os = "windows", target_os = "linux"));

/// What marking a connection metered came to.
#[derive(Debug, PartialEq)]
pub enum Mark {
    /// Marked; holds what `unmark` needs to put the connection back.
    Done(String),
    /// Already metered (the user's own choice, left alone), or can't be marked here.
    Skip,
    /// The command failed; try again on a later tick.
    Failed,
}

#[cfg(target_os = "linux")]
fn mark(interface: &str, _kind: &str) -> Mark {
    let out = |args: &[&str]| Command::new("nmcli").args(args).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let Some(uuid) = out(&["-g", "GENERAL.CON-UUID", "device", "show", interface]).filter(|u| !u.is_empty()) else { return Mark::Failed };
    let Some(before) = out(&["-g", "connection.metered", "connection", "show", &uuid]) else { return Mark::Failed };
    if before == "yes" {
        return Mark::Skip;
    }
    if !run(Command::new("nmcli").args(["connection", "modify", &uuid, "connection.metered", "yes"])) {
        return Mark::Failed;
    }
    run(Command::new("nmcli").args(["device", "reapply", interface]));
    Mark::Done(format!("{uuid}|{before}|{interface}"))
}

#[cfg(target_os = "linux")]
fn unmark(token: &str) -> bool {
    let mut parts = token.splitn(3, '|');
    let (Some(uuid), Some(before), Some(interface)) = (parts.next(), parts.next(), parts.next()) else { return true };
    let ok = run(Command::new("nmcli").args(["connection", "modify", uuid, "connection.metered", before]));
    run(Command::new("nmcli").args(["device", "reapply", interface]));
    ok
}

// Windows: the Wi-Fi profile's cost, which Windows Update, OneDrive and Store respect. Ethernet needs the registry, so Wi-Fi only.
#[cfg(target_os = "windows")]
fn mark(interface: &str, kind: &str) -> Mark {
    use std::os::windows::process::CommandExt;
    let netsh = |args: &[&str]| Command::new("netsh").args(args).creation_flags(0x0800_0000).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    if kind != "wifi" {
        return Mark::Skip;
    }
    // The connected profile, which can be named differently from its SSID.
    let adapter = netdev::get_interfaces().into_iter().find(|i| i.name == interface).and_then(|i| i.friendly_name);
    let Some(profile) = adapter.and_then(|a| crate::system::parse_netsh(&netsh(&["wlan", "show", "interfaces"])?, &a, "Profile")) else { return Mark::Failed };
    let Some(cost) = netsh(&["wlan", "show", "profile", &format!("name={profile}")]).and_then(|o| parse_netsh_cost(&o)) else { return Mark::Failed };
    if cost != "Unrestricted" && cost != "Default" {
        return Mark::Skip;
    }
    match netsh(&["wlan", "set", "profileparameter", &format!("name={profile}"), "cost=Fixed"]) {
        Some(_) => Mark::Done(format!("{cost}|{profile}")),
        None => Mark::Failed,
    }
}

#[cfg(target_os = "windows")]
fn unmark(token: &str) -> bool {
    use std::os::windows::process::CommandExt;
    let Some((cost, profile)) = token.split_once('|') else { return true };
    run(Command::new("netsh").args(["wlan", "set", "profileparameter", &format!("name={profile}"), &format!("cost={cost}")]).creation_flags(0x0800_0000))
}

/// netsh prints "Cost : Unrestricted" for an ordinary profile, "Fixed"/"Variable" for a metered one.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn parse_netsh_cost(output: &str) -> Option<String> {
    output.lines().find_map(|l| l.trim().strip_prefix("Cost").and_then(|r| r.trim_start().strip_prefix(':'))).map(|c| c.trim().to_string()).filter(|c| !c.is_empty())
}

#[cfg(target_os = "macos")]
fn mark(_interface: &str, _kind: &str) -> Mark {
    Mark::Skip
}

#[cfg(target_os = "macos")]
fn unmark(_token: &str) -> bool {
    true
}

/// Keeps OS metered flags in line with the data saver, and puts each connection back the way it was.
#[derive(Debug, Default)]
pub struct Marker {
    /// network id → what `unmark` needs.
    marked: HashMap<i64, String>,
    /// Networks skipped this session (already metered by the user), so they're never touched.
    skipped: HashSet<i64>,
}

impl Marker {
    pub fn restore(saved: &str) -> Marker {
        Marker { marked: serde_json::from_str(saved).unwrap_or_default(), skipped: HashSet::new() }
    }

    pub fn saved(&self) -> String {
        serde_json::to_string(&self.marked).unwrap_or_default()
    }

    /// Network ids this marker made metered, so hotspot detection can ignore its own flag.
    pub fn marked_ids(saved: &str) -> HashSet<i64> {
        serde_json::from_str::<HashMap<i64, String>>(saved).map(|m| m.into_keys().collect()).unwrap_or_default()
    }

    /// `want`: (network id, interface, kind) the data saver covers right now.
    pub fn apply(&mut self, want: &[(i64, String, String)]) {
        self.apply_with(want, mark, unmark)
    }

    fn apply_with(&mut self, want: &[(i64, String, String)], mut mark: impl FnMut(&str, &str) -> Mark, mut unmark: impl FnMut(&str) -> bool) {
        let ids: HashSet<i64> = want.iter().map(|w| w.0).collect();
        self.marked.retain(|id, token| ids.contains(id) || !unmark(token));
        self.skipped.retain(|id| ids.contains(id));
        for (id, interface, kind) in want {
            if self.marked.contains_key(id) || self.skipped.contains(id) {
                continue;
            }
            match mark(interface, kind) {
                Mark::Done(token) => { self.marked.insert(*id, token); }
                Mark::Skip => { self.skipped.insert(*id); }
                Mark::Failed => {}
            }
        }
    }
}

/// Keeps adapter shaping in line with what the caps want, without re-prompting after a refusal.
#[derive(Debug, Default)]
pub struct Enforcer {
    shaped: HashMap<String, i64>,
    tried: HashMap<String, Option<i64>>,
}

impl Enforcer {
    /// Restores shaping state saved by `saved()`, so a restart can still lift a throttle.
    pub fn restore(saved: &str) -> Enforcer {
        let shaped = saved.split(',').filter_map(|p| p.split_once('=')).filter_map(|(i, k)| Some((i.to_string(), k.parse().ok()?))).collect();
        Enforcer { shaped, tried: HashMap::new() }
    }

    pub fn saved(&self) -> String {
        let mut parts: Vec<_> = self.shaped.iter().map(|(i, k)| format!("{i}={k}")).collect();
        parts.sort();
        parts.join(",")
    }

    pub fn apply(&mut self, block: &[(String, String)], throttle: &HashMap<String, i64>) {
        self.apply_with(block, throttle, disconnect, shape)
    }

    fn apply_with(&mut self, block: &[(String, String)], throttle: &HashMap<String, i64>, mut disconnect: impl FnMut(&str, &str) -> bool, mut shape: impl FnMut(&str, Option<i64>) -> bool) {
        for (interface, kind) in block {
            disconnect(interface, kind);
        }
        // Forget refusals once nothing is pending for an interface, so a later period can ask again.
        self.tried.retain(|i, _| throttle.contains_key(i) || self.shaped.contains_key(i));
        let interfaces: HashSet<String> = self.shaped.keys().chain(throttle.keys()).cloned().collect();
        for interface in interfaces {
            let want = throttle.get(&interface).copied();
            if self.shaped.get(&interface).copied() == want || self.tried.get(&interface) == Some(&want) {
                continue;
            }
            self.tried.insert(interface.clone(), want);
            if shape(&interface, want) {
                match want {
                    Some(k) => self.shaped.insert(interface, k),
                    None => self.shaped.remove(&interface),
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn throttle_reconciles_and_never_reprompts_after_refusal() {
        let mut e = Enforcer::default();
        let want: HashMap<_, _> = [("en0".to_string(), 1000)].into();
        let mut calls = vec![];
        e.apply_with(&[], &want, |_, _| true, |i, k| { calls.push((i.to_string(), k)); false });
        e.apply_with(&[], &want, |_, _| true, |i, k| { calls.push((i.to_string(), k)); true });
        assert_eq!(calls, vec![("en0".to_string(), Some(1000))]);

        let mut e = Enforcer::default();
        let mut calls = 0;
        e.apply_with(&[], &want, |_, _| true, |_, _| { calls += 1; false });
        e.apply_with(&[], &HashMap::new(), |_, _| true, |_, _| panic!("nothing to shape"));
        e.apply_with(&[], &want, |_, _| true, |_, _| { calls += 1; false });
        assert_eq!(calls, 2, "a new period asks again");

        let mut e = Enforcer::restore("en0=1000");
        e.apply_with(&[], &want, |_, _| true, |_, _| panic!("already shaped"));
        let mut lifted = vec![];
        e.apply_with(&[], &HashMap::new(), |_, _| true, |i, k| { lifted.push((i.to_string(), k)); true });
        assert_eq!(lifted, vec![("en0".to_string(), None)]);
        assert_eq!(e.saved(), "");
    }

    #[test] fn metered_marks_once_retries_failures_and_restores_when_saver_stops() {
        let want = vec![(1, "en0".to_string(), "wifi".to_string()), (2, "en1".to_string(), "wifi".to_string()), (3, "en2".to_string(), "wifi".to_string())];
        let mut m = Marker::default();
        let mut calls = vec![];
        // en0 marks, en1 is already metered by the user (never touched again), en2 fails and is retried each time.
        for _ in 0..3 {
            m.apply_with(&want, |i, _| { calls.push(i.to_string()); match i { "en0" => Mark::Done("Unrestricted|Home".into()), "en1" => Mark::Skip, _ => Mark::Failed } }, |_| panic!("nothing to restore"));
        }
        assert_eq!(calls, ["en0", "en1", "en2", "en2", "en2"]);
        assert_eq!(m.saved(), r#"{"1":"Unrestricted|Home"}"#);
        assert_eq!(Marker::marked_ids(&m.saved()), [1].into());

        let mut m = Marker::restore(&m.saved());
        let mut restored = vec![];
        m.apply_with(&[], |_, _| panic!("nothing wanted"), |t| { restored.push(t.to_string()); true });
        assert_eq!(restored, vec!["Unrestricted|Home"]);
        assert_eq!(m.saved(), "{}");
    }

    #[test] fn netsh_cost_is_read() {
        assert_eq!(parse_netsh_cost("    Cost                   : Unrestricted\n").as_deref(), Some("Unrestricted"));
        assert_eq!(parse_netsh_cost("    Cost                   : Fixed\n").as_deref(), Some("Fixed"));
        assert_eq!(parse_netsh_cost(""), None);
    }

    #[test] fn interface_names_are_checked_before_reaching_a_shell() {
        assert!(safe("en0") && safe("wlp2s0") && safe("enp0s31f6.10"));
        assert!(!safe("en0; rm -rf /") && !safe("") && !safe("a b"));
    }
}
