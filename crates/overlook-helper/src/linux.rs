// ponytail: TCP only, UDP/QUIC falls into System; upgrade to eBPF cgroup_skb for full coverage
use crate::acc::Acc;
use netlink_packet_core::{NetlinkHeader, NetlinkMessage, NetlinkPayload, NLM_F_DUMP, NLM_F_REQUEST};
use netlink_packet_sock_diag::{
    constants::*,
    inet::{nlas::Nla, ExtensionFlags, InetRequest, SocketId, StateFlags},
    SockDiagMessage,
};
use netlink_sys::{protocols::NETLINK_SOCK_DIAG, Socket, SocketAddr};
use std::collections::HashMap;
use std::ffi::CString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct Owner { gid: u32 }

impl Owner {
    /// Numeric uid or username, resolved through NSS; the snapshot is readable by its primary group.
    pub fn parse(s: &str) -> Result<Owner, String> {
        let pw = if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
            let uid: u32 = s.parse().map_err(|_| format!("bad uid {s:?}"))?;
            unsafe { libc::getpwuid(uid) }
        } else {
            let name = CString::new(s).map_err(|_| format!("bad user {s:?}"))?;
            unsafe { libc::getpwnam(name.as_ptr()) }
        };
        if pw.is_null() {
            return Err(format!("unknown owner {s:?}"));
        }
        Ok(Owner { gid: unsafe { (*pw).pw_gid } })
    }
}

pub fn secure_dir(dir: &Path, owner: &Owner) -> Result<(), String> {
    let err = |e: std::io::Error| format!("{}: {e}", dir.display());
    fs::create_dir_all(dir).map_err(err)?;
    if !fs::symlink_metadata(dir).map_err(err)?.is_dir() {
        return Err(format!("{} is not a directory", dir.display()));
    }
    std::os::unix::fs::chown(dir, Some(0), Some(owner.gid)).map_err(err)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o750)).map_err(err)
}

pub fn secure_file(f: &fs::File, owner: &Owner) -> std::io::Result<()> {
    std::os::unix::fs::fchown(f, Some(0), Some(owner.gid))?;
    f.set_permissions(fs::Permissions::from_mode(0o640))
}

/// Sockets are polled each round, so there is nothing running in the background.
pub fn start(_acc: Arc<Mutex<Acc>>) -> Result<(), String> { Ok(()) }

pub fn poll(acc: &mut Acc) {
    let pids = socket_pids();
    for family in [AF_INET, AF_INET6] {
        if let Err(e) = dump(family, |inode, rx, tx| {
            if let Some(&pid) = pids.get(&inode) {
                acc.observe(inode, pid, rx, tx);
            }
        }) {
            eprintln!("overlook-helper: sock_diag: {e}");
        }
    }
}

pub fn exe_path(pid: u32) -> Option<String> {
    fs::read_link(format!("/proc/{pid}/exe")).ok().map(|p| p.to_string_lossy().into_owned())
}

/// socket inode -> pid, from /proc/*/fd links `socket:[N]`.
// ponytail: a socket shared across processes goes to its lowest pid; if that pid exits first the survivor recounts it once.
fn socket_pids() -> HashMap<u64, u32> {
    let mut out = HashMap::new();
    let Ok(procs) = fs::read_dir("/proc") else { return out };
    for p in procs.flatten() {
        let Some(pid) = p.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        let Ok(fds) = fs::read_dir(p.path().join("fd")) else { continue };
        for fd in fds.flatten() {
            let Ok(link) = fs::read_link(fd.path()) else { continue };
            let link = link.to_string_lossy();
            if let Some(inode) = link.strip_prefix("socket:[").and_then(|s| s.strip_suffix(']')).and_then(|s| s.parse().ok()) {
                out.entry(inode).and_modify(|p: &mut u32| *p = (*p).min(pid)).or_insert(pid);
            }
        }
    }
    out
}

/// Dumps TCP sockets of `family` with INET_DIAG_INFO, calling `f(inode, bytes_received, bytes_acked)` for non-loopback peers.
fn dump(family: u8, mut f: impl FnMut(u64, u64, u64)) -> Result<(), String> {
    let mut socket = Socket::new(NETLINK_SOCK_DIAG).map_err(|e| e.to_string())?;
    socket.bind_auto().map_err(|e| e.to_string())?;
    socket.connect(&SocketAddr::new(0, 0)).map_err(|e| e.to_string())?;

    let mut header = NetlinkHeader::default();
    header.flags = NLM_F_REQUEST | NLM_F_DUMP;
    let mut packet = NetlinkMessage::new(
        header,
        SockDiagMessage::InetRequest(InetRequest {
            family,
            protocol: IPPROTO_TCP,
            extensions: ExtensionFlags::INFO,
            states: StateFlags::all(),
            socket_id: if family == AF_INET { SocketId::new_v4() } else { SocketId::new_v6() },
        })
        .into(),
    );
    packet.finalize();
    let mut buf = vec![0; packet.buffer_len()];
    packet.serialize(&mut buf);
    socket.send(&buf, 0).map_err(|e| e.to_string())?;

    loop {
        let (data, _) = socket.recv_from_full().map_err(|e| e.to_string())?;
        let mut offset = 0;
        while offset < data.len() {
            let msg = NetlinkMessage::<SockDiagMessage>::deserialize(&data[offset..]).map_err(|e| e.to_string())?;
            match msg.payload {
                NetlinkPayload::InnerMessage(SockDiagMessage::InetResponse(r)) => {
                    let dst = r.header.socket_id.destination_address.to_canonical();
                    if !dst.is_loopback() && !dst.is_unspecified() {
                        if let Some((rx, tx)) = r.nlas.iter().find_map(|n| match n { Nla::TcpInfo(b) => tcp_bytes(b), _ => None }) {
                            f(r.header.inode as u64, rx, tx);
                        }
                    }
                }
                NetlinkPayload::Done(_) => return Ok(()),
                NetlinkPayload::Error(e) => return Err(format!("{e:?}")),
                _ => {}
            }
            if msg.header.length == 0 {
                break;
            }
            offset += msg.header.length as usize;
        }
    }
}

/// (tcpi_bytes_received, tcpi_bytes_acked) from a raw struct tcp_info (offsets 128 and 120, kernel >= 4.1).
fn tcp_bytes(b: &[u8]) -> Option<(u64, u64)> {
    let u64_at = |o: usize| Some(u64::from_ne_bytes(b.get(o..o + 8)?.try_into().ok()?));
    Some((u64_at(128)?, u64_at(120)?))
}
