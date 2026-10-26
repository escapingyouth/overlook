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

    #[test] fn interface_names_are_checked_before_reaching_a_shell() {
        assert!(safe("en0") && safe("wlp2s0") && safe("enp0s31f6.10"));
        assert!(!safe("en0; rm -rf /") && !safe("") && !safe("a b"));
    }
}
