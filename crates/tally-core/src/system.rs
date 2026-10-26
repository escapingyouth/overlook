
use std::collections::HashMap;
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counter {
    pub rx: u64,
    pub tx: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WifiNameAccess {
    Granted,
    Denied,
    NotDetermined,
    Unknown,
}

#[cfg(target_os = "macos")]
pub fn counters(_interfaces: &[netdev::Interface]) -> HashMap<String, Counter> {
    use std::mem::size_of;
    let mut result = HashMap::new();
    let mut mib = [libc::CTL_NET, libc::PF_ROUTE, 0, 0, libc::NET_RT_IFLIST2, 0];
    let mut len: libc::size_t = 0;
    unsafe {
        if libc::sysctl(mib.as_mut_ptr(), 6, std::ptr::null_mut(), &mut len, std::ptr::null_mut(), 0) != 0 || len == 0 {
            return result;
        }
        len += 4096;
        let mut buf = vec![0u8; len];
        if libc::sysctl(mib.as_mut_ptr(), 6, buf.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0) != 0 {
            return result;
        }
        let mut offset = 0usize;
        while offset + 4 <= len {
            let msglen = u16::from_ne_bytes([buf[offset], buf[offset + 1]]) as usize;
            if msglen == 0 { break; }
            if buf[offset + 3] as i32 == libc::RTM_IFINFO2 && offset + size_of::<libc::if_msghdr2>() <= len {
                let m: libc::if_msghdr2 = std::ptr::read_unaligned(buf.as_ptr().add(offset).cast());
                let mut name = [0 as libc::c_char; libc::IF_NAMESIZE];
                if !libc::if_indextoname(m.ifm_index as u32, name.as_mut_ptr()).is_null() {
                    let name = std::ffi::CStr::from_ptr(name.as_ptr()).to_string_lossy().into_owned();
                    result.insert(name, Counter { rx: m.ifm_data.ifi_ibytes, tx: m.ifm_data.ifi_obytes });
                }
            }
            offset += msglen;
        }
    }
    result
}

#[cfg(target_os = "linux")]
pub fn counters(interfaces: &[netdev::Interface]) -> HashMap<String, Counter> {
    let read = |name: &str, file: &str| {
        std::fs::read_to_string(format!("/sys/class/net/{name}/statistics/{file}")).ok()?.trim().parse::<u64>().ok()
    };
    interfaces
        .iter()
        .filter_map(|i| Some((i.name.clone(), Counter { rx: read(&i.name, "rx_bytes")?, tx: read(&i.name, "tx_bytes")? })))
        .collect()
}

#[cfg(target_os = "windows")]
pub fn counters(interfaces: &[netdev::Interface]) -> HashMap<String, Counter> {
    interfaces
        .iter()
        .filter_map(|i| i.stats.as_ref().map(|s| (i.name.clone(), Counter { rx: s.rx_bytes, tx: s.tx_bytes })))
        .collect()
}

#[cfg(target_os = "macos")]
pub fn boot_time() -> i64 {
    let mut mib = [libc::CTL_KERN, libc::KERN_BOOTTIME];
    let mut tv = libc::timeval { tv_sec: 0, tv_usec: 0 };
    let mut size = std::mem::size_of::<libc::timeval>();
    unsafe { libc::sysctl(mib.as_mut_ptr(), 2, (&mut tv as *mut libc::timeval).cast(), &mut size, std::ptr::null_mut(), 0) };
    tv.tv_sec
}

#[cfg(target_os = "linux")]
pub fn boot_time() -> i64 {
    std::fs::read_to_string("/proc/stat")
        .ok()
        .and_then(|s| s.lines().find_map(|l| l.strip_prefix("btime ")?.trim().parse().ok()))
        .unwrap_or(0)
}

#[cfg(target_os = "windows")]
pub fn boot_time() -> i64 {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetTickCount64() -> u64;
    }
    let uptime = unsafe { GetTickCount64() } as f64 / 1000.0;
    (crate::now() - uptime).round() as i64
}

pub fn is_physical(i: &netdev::Interface) -> bool {
    use netdev::prelude::InterfaceType::*;
    if i.is_loopback() || i.is_tun() {
        return false;
    }
    if !matches!(i.if_type, Ethernet | Wireless80211 | GigabitEthernet | FastEthernetT | FastEthernetFx) {
        return false;
    }
    let name = i.name.to_lowercase();
    let label = format!("{} {}", i.friendly_name.clone().unwrap_or_default(), i.description.clone().unwrap_or_default()).to_lowercase();
    const VIRTUAL: [&str; 12] = ["virtual", "hyper-v", "vethernet", "vmware", "virtualbox", "tap-", "wireguard", "vpn", "loopback", "bluetooth", "docker", "wsl"];
    if VIRTUAL.iter().any(|v| label.contains(v)) {
        return false;
    }
    if cfg!(target_os = "linux") {
        return std::path::Path::new(&format!("/sys/class/net/{}/device", i.name)).exists();
    }
    if cfg!(target_os = "macos") {
        return name.starts_with("en");
    }
    true
}

pub fn is_wifi(i: &netdev::Interface) -> bool {
    i.if_type == netdev::prelude::InterfaceType::Wireless80211
        || (cfg!(target_os = "linux") && std::path::Path::new(&format!("/sys/class/net/{}/wireless", i.name)).exists())
}

#[cfg(target_os = "macos")]
pub fn neighbor_mac(_ip: &str, interface: &str) -> Option<String> {
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;
    let store = system_configuration::dynamic_store::SCDynamicStoreBuilder::new("Tally").build()?;
    let keys = store.get_keys("State:/Network/Service/[^/]+/IPv4")?;
    keys.iter().find_map(|key| {
        let value = store.get(key.clone())?;
        let dict: CFDictionary<CFString, CFType> = unsafe { CFDictionary::wrap_under_get_rule(value.as_concrete_TypeRef() as _) };
        let get = |name: &str| dict.find(CFString::new(name)).and_then(|v| v.downcast::<CFString>()).map(|s| s.to_string());
        (get("InterfaceName").as_deref() == Some(interface)).then(|| normalize_mac(&get("ARPResolvedHardwareAddress")?)).flatten()
    })
}

#[cfg(target_os = "linux")]
pub fn neighbor_mac(ip: &str, interface: &str) -> Option<String> {
    std::fs::read_to_string("/proc/net/arp").ok()?.lines().skip(1).find_map(|l| {
        let f: Vec<_> = l.split_whitespace().collect();
        (f.len() >= 6 && f[0] == ip && f[5] == interface).then(|| normalize_mac(f[3])).flatten()
    })
}

#[cfg(target_os = "windows")]
pub fn neighbor_mac(_ip: &str, _interface: &str) -> Option<String> {
    None
}

pub fn parse_arp(output: &str, ip: &str, interface: &str) -> Option<String> {
    let needle = format!("({ip})");
    output.lines().find_map(|l| {
        let f: Vec<_> = l.split_whitespace().collect();
        let on = f.iter().position(|t| *t == "on")?;
        (f.contains(&needle.as_str()) && f.get(on + 1) == Some(&interface)).then(|| normalize_mac(f.get(3)?)).flatten()
    })
}

pub fn normalize_mac(raw: &str) -> Option<String> {
    let parts: Vec<_> = raw.split([':', '-']).collect();
    if parts.len() != 6 || !parts.iter().all(|p| (1..=2).contains(&p.len()) && p.chars().all(|c| c.is_ascii_hexdigit())) {
        return None;
    }
    let mac = parts.iter().map(|p| format!("{:0>2}", p.to_lowercase())).collect::<Vec<_>>().join(":");
    (mac != "00:00:00:00:00:00" && mac != "ff:ff:ff:ff:ff:ff").then_some(mac)
}

#[cfg(target_os = "macos")]
pub fn ssid(interface: &netdev::Interface) -> Option<String> {
    use objc2_core_wlan::CWWiFiClient;
    use objc2_foundation::NSString;
    objc2::rc::autoreleasepool(|_| unsafe {
        let client = CWWiFiClient::sharedWiFiClient();
        let wifi = client.interfaceWithName(Some(&NSString::from_str(&interface.name)))?;
        wifi.ssid().map(|s| s.to_string())
    })
}

#[cfg(target_os = "windows")]
pub fn ssid(interface: &netdev::Interface) -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = Command::new("netsh").args(["wlan", "show", "interfaces"]).creation_flags(CREATE_NO_WINDOW).output().ok()?;
    parse_netsh(&String::from_utf8_lossy(&out.stdout), interface.friendly_name.as_deref()?)
}

#[cfg(target_os = "linux")]
pub fn ssid(interface: &netdev::Interface) -> Option<String> {
    let iw = Command::new("iw").args(["dev", &interface.name, "link"]).output().ok();
    if let Some(name) = iw.and_then(|o| parse_iw(&String::from_utf8_lossy(&o.stdout))) {
        return Some(name);
    }
    let nm = Command::new("nmcli").args(["-t", "-g", "GENERAL.CONNECTION", "device", "show", &interface.name]).output().ok()?;
    Some(String::from_utf8_lossy(&nm.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

pub fn parse_netsh(output: &str, adapter: &str) -> Option<String> {
    let mut in_block = false;
    for line in output.lines() {
        let Some((k, v)) = line.split_once(':') else { continue };
        let (k, v) = (k.trim(), v.trim());
        if k == "Name" {
            in_block = v == adapter;
        } else if in_block && k == "SSID" && !v.is_empty() {
            return Some(v.to_string());
        }
    }
    None
}

pub fn parse_iw(output: &str) -> Option<String> {
    output.lines().find_map(|l| l.trim().strip_prefix("SSID: ").map(str::to_string)).filter(|s| !s.is_empty())
}

#[cfg(target_os = "macos")]
pub fn wifi_name_access() -> WifiNameAccess {
    use objc2_core_location::{CLAuthorizationStatus, CLLocationManager};
    let status = unsafe { CLLocationManager::new().authorizationStatus() };
    match status {
        CLAuthorizationStatus::AuthorizedAlways | CLAuthorizationStatus::AuthorizedWhenInUse => WifiNameAccess::Granted,
        CLAuthorizationStatus::NotDetermined => WifiNameAccess::NotDetermined,
        _ => WifiNameAccess::Denied,
    }
}

#[cfg(not(target_os = "macos"))]
pub fn wifi_name_access() -> WifiNameAccess {
    WifiNameAccess::Unknown
}

#[cfg(target_os = "macos")]
pub fn request_wifi_names() {
    use objc2_core_location::CLLocationManager;
    thread_local! { static MANAGER: objc2::rc::Retained<CLLocationManager> = unsafe { CLLocationManager::new() }; }
    if wifi_name_access() == WifiNameAccess::NotDetermined {
        MANAGER.with(|m| unsafe { m.requestWhenInUseAuthorization() });
    } else {
        Command::new("open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_LocationServices").spawn().ok();
    }
}

#[cfg(target_os = "windows")]
pub fn request_wifi_names() {
    Command::new("cmd").args(["/C", "start", "ms-settings:privacy-location"]).spawn().ok();
}

#[cfg(target_os = "linux")]
pub fn request_wifi_names() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn netsh_picks_ssid_for_adapter_not_bssid() {
        let out = "There is 1 interface on the system:\n\n    Name                   : Wi-Fi 2\n    SSID                   : Other\n\n    Name                   : Wi-Fi\n    Description            : Intel\n    BSSID                  : aa:bb:cc:dd:ee:ff\n    SSID                   : Home Net\n";
        assert_eq!(parse_netsh(out, "Wi-Fi").as_deref(), Some("Home Net"));
        assert_eq!(parse_netsh(out, "Ethernet"), None);
    }

    #[test] fn arp_fallback_matches_ip_and_interface() {
        let out = "? (10.0.0.9) at 1:2:3:4:5:6 on en1 ifscope [ethernet]\n? (10.0.0.1) at 8a:79:3:bc:ad:59 on en0 ifscope [ethernet]\n? (10.0.0.7) at (incomplete) on en0 ifscope [ethernet]";
        assert_eq!(parse_arp(out, "10.0.0.1", "en0").as_deref(), Some("8a:79:03:bc:ad:59"));
        assert_eq!(parse_arp(out, "10.0.0.1", "en1"), None);
        assert_eq!(parse_arp(out, "10.0.0.7", "en0"), None);
        assert_eq!(normalize_mac("AA-BB-CC-DD-EE-0F").as_deref(), Some("aa:bb:cc:dd:ee:0f"));
    }

    #[test] fn iw_link() {
        assert_eq!(parse_iw("Connected to 11:22:33:44:55:66 (on wlan0)\n\tSSID: Cafe\n\tfreq: 5180").as_deref(), Some("Cafe"));
        assert_eq!(parse_iw("Not connected."), None);
    }

    #[test] fn counters_and_boot_time_are_readable() {
        let interfaces = netdev::get_interfaces();
        assert!(boot_time() > 1_000_000_000);
        assert!(!counters(&interfaces).is_empty());
    }
}
