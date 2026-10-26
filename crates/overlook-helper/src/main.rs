#![cfg_attr(not(any(target_os = "linux", target_os = "windows")), allow(dead_code))]
//! Privileged per-app byte counter. Runs as root (systemd) or SYSTEM (SCM) and
//! writes a snapshot that overlook-core reads; never listens on the network.
mod acc;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as platform;

// Same shape as overlook-core's apps::{ProcSample, Snapshot}; duplicated so the helper doesn't link SQLite.
#[derive(serde::Serialize)]
struct ProcSample { pid: u32, path: String, rx: u64, tx: u64 }

#[derive(serde::Serialize)]
struct Snapshot { session: String, at: f64, procs: Vec<ProcSample> }

#[cfg(target_os = "linux")]
const SNAPSHOT_PATH: &str = "/run/overlook-helper/counters.json";
#[cfg(target_os = "windows")]
const SNAPSHOT_PATH: &str = r"C:\ProgramData\Overlook\counters.json";

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn main() {
    println!("unsupported");
    std::process::exit(1);
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mode, owner) = match args.as_slice() {
        [mode, flag, owner] if flag == "--owner" => (mode.as_str(), owner.clone()),
        _ => usage(),
    };
    let result = match mode {
        "run" => {
            let (_keep, stop) = std::sync::mpsc::channel();
            run(&owner, stop)
        }
        #[cfg(target_os = "windows")]
        "service" => platform::service(owner),
        _ => usage(),
    };
    if let Err(e) = result {
        eprintln!("overlook-helper: {e}");
        std::process::exit(1);
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn usage() -> ! {
    eprintln!("usage: overlook-helper run --owner <user>{}", if cfg!(windows) { "\n       overlook-helper service --owner <user>" } else { "" });
    std::process::exit(2);
}

/// Polls every 5 s until `stop` fires (or its sender is dropped).
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn run(owner: &str, stop: std::sync::mpsc::Receiver<()>) -> Result<(), String> {
    use std::sync::{mpsc::RecvTimeoutError, Arc, Mutex};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    let now = || SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());

    let owner = platform::Owner::parse(owner)?;
    let path = std::path::Path::new(SNAPSHOT_PATH);
    platform::secure_dir(path.parent().unwrap(), &owner)?;
    let session = format!("{}", now() as u64);
    let acc = Arc::new(Mutex::new(acc::Acc::default()));
    let _source = platform::start(acc.clone())?;
    loop {
        let procs = {
            let mut acc = acc.lock().unwrap_or_else(|e| e.into_inner());
            platform::poll(&mut acc);
            acc.end_round();
            let paths: std::collections::HashMap<u32, String> = acc.totals().keys().filter_map(|&pid| Some((pid, platform::exe_path(pid)?))).collect();
            acc.retain(|pid| paths.contains_key(&pid));
            acc.totals().into_iter().map(|(pid, (rx, tx))| ProcSample { pid, path: paths[&pid].clone(), rx, tx }).collect()
        };
        let json = serde_json::to_vec(&Snapshot { session: session.clone(), at: now(), procs }).map_err(|e| e.to_string())?;
        if let Err(e) = write_atomic(path, &json, &owner) {
            eprintln!("overlook-helper: write {SNAPSHOT_PATH}: {e}");
        }
        match stop.recv_timeout(Duration::from_secs(5)) {
            Err(RecvTimeoutError::Timeout) => {}
            _ => return Ok(()),
        }
    }
}

/// Writes `<path>.tmp` fresh (never through a pre-existing file or link), then renames over `path`.
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn write_atomic(path: &std::path::Path, data: &[u8], owner: &platform::Owner) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("json.tmp");
    match std::fs::remove_file(&tmp) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(target_os = "linux")]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o400);
    let mut f = opts.open(&tmp)?;
    platform::secure_file(&f, owner)?;
    f.write_all(data)?;
    drop(f);
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    #[test]
    fn snapshot_json_matches_core() {
        let s = super::Snapshot { session: "1".into(), at: 2.5, procs: vec![super::ProcSample { pid: 3, path: "/a".into(), rx: 4, tx: 5 }] };
        assert_eq!(serde_json::to_string(&s).unwrap(), r#"{"session":"1","at":2.5,"procs":[{"pid":3,"path":"/a","rx":4,"tx":5}]}"#);
    }
}
