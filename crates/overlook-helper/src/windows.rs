use crate::acc::Acc;
use ferrisetw::parser::Parser;
use ferrisetw::provider::kernel_providers::{KernelProvider, TCP_IP_PROVIDER};
use ferrisetw::provider::Provider;
use ferrisetw::trace::{stop_trace_by_name, KernelTrace};
use ferrisetw::{EventRecord, SchemaLocator, GUID};
use std::net::IpAddr;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::Duration;

const SERVICE_NAME: &str = "OverlookHelper";
const TRACE_NAME: &str = "OverlookHelper";
const SYSTEM_SID: &str = "*S-1-5-18";
// ferrisetw dispatches by provider GUID and only exports TcpIp; UdpIp (bf3a50c5-...) rides the same NETWORK_TCPIP flag.
static UDP_IP_PROVIDER: KernelProvider = KernelProvider::new(
    GUID::from_values(0xbf3a50c5, 0xa9c9, 0x4988, [0xa0, 0x05, 0x2d, 0xf0, 0xb7, 0xc8, 0x0f, 0x80]),
    0x0001_0000, // EVENT_TRACE_FLAG_NETWORK_TCPIP
);

pub struct Owner(String);

impl Owner {
    /// A Windows account name (`user`, `DOMAIN\user`, `user@domain`); no characters icacls treats as syntax.
    pub fn parse(s: &str) -> Result<Owner, String> {
        let ok = !s.is_empty() && s.len() <= 256 && !s.starts_with(['/', '-', '*'])
            && s.chars().all(|c| c.is_alphanumeric() || " ._-\\@$".contains(c));
        if ok { Ok(Owner(s.into())) } else { Err(format!("bad owner {s:?}")) }
    }
}

fn icacls(args: &[&str]) -> Result<(), String> {
    let exe = Path::new(&std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())).join(r"System32\icacls.exe");
    let out = Command::new(exe).args(args).output().map_err(|e| format!("icacls: {e}"))?;
    if out.status.success() { Ok(()) } else { Err(format!("icacls {args:?}: {}", String::from_utf8_lossy(&out.stdout).trim())) }
}

/// SYSTEM owns the directory; only SYSTEM/Administrators (full) and the owner (read) have access, inherited by the snapshot.
/// It is created with a protected DACL so no user can touch it before lockdown. ProgramData is user-writable, so a
/// pre-existing directory not owned by SYSTEM/Administrators (or a junction) is refused. Nothing here recurses.
pub fn secure_dir(dir: &Path, owner: &Owner) -> Result<(), String> {
    let err = |e: std::io::Error| format!("{}: {e}", dir.display());
    create_protected_dir(dir).map_err(err)?;
    let meta = std::fs::symlink_metadata(dir).map_err(err)?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(format!("{} is not a plain directory", dir.display()));
    }
    if !owned_by_admin(dir)? {
        return Err(format!("{} exists and is not owned by SYSTEM or Administrators; remove it and restart the service", dir.display()));
    }
    let d = dir.to_str().ok_or("non-UTF-8 snapshot dir")?;
    // Adds the owner's read grant. The SYSTEM grant and /inheritance:r change nothing on a directory we created;
    // they lock down an admin-made one that still inherits, without SYSTEM losing access.
    icacls(&[d, "/inheritance:r", "/grant:r", &format!("{SYSTEM_SID}:(OI)(CI)F"), &format!("{}:(OI)(CI)R", owner.0)])
}

/// CreateDirectoryW with `O:SYG:SYD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)`; an existing directory is left alone.
fn create_protected_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{LocalFree, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::CreateDirectoryW;
    let sddl: Vec<u16> = "O:SYG:SYD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)".encode_utf16().chain([0]).collect();
    let path: Vec<u16> = dir.as_os_str().encode_wide().chain([0]).collect();
    let mut sd = std::ptr::null_mut();
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), SDDL_REVISION_1, &mut sd, std::ptr::null_mut()) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let sa = SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: sd, bInheritHandle: 0 };
        let ok = CreateDirectoryW(path.as_ptr(), &sa) != 0;
        let e = std::io::Error::last_os_error();
        LocalFree(sd);
        if ok || e.raw_os_error() == Some(ERROR_ALREADY_EXISTS as i32) { Ok(()) } else { Err(e) }
    }
}

fn owned_by_admin(dir: &Path) -> Result<bool, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{IsWellKnownSid, WinBuiltinAdministratorsSid, WinLocalSystemSid, OWNER_SECURITY_INFORMATION};
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain([0]).collect();
    let (mut sid, mut sd) = (std::ptr::null_mut(), std::ptr::null_mut());
    unsafe {
        let rc = GetNamedSecurityInfoW(wide.as_ptr(), SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION, &mut sid, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), &mut sd);
        if rc != 0 {
            return Err(format!("{}: GetNamedSecurityInfoW error {rc}", dir.display()));
        }
        let ok = IsWellKnownSid(sid, WinLocalSystemSid) != 0 || IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0;
        LocalFree(sd);
        Ok(ok)
    }
}

/// The snapshot inherits the directory ACL set by `secure_dir`.
pub fn secure_file(_f: &std::fs::File, _owner: &Owner) -> std::io::Result<()> { Ok(()) }

/// ETW pushes events into `acc`; each event is its own "socket" so it lands in the closed sum.
pub fn start(acc: Arc<Mutex<Acc>>) -> Result<KernelTrace, String> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let callback = move |acc: Arc<Mutex<Acc>>| {
        move |record: &EventRecord, locator: &SchemaLocator| {
            // TcpIp and UdpIp share opcodes: 10/26 send v4/v6, 11/27 receive v4/v6.
            let rx = match record.opcode() { 10 | 26 => false, 11 | 27 => true, _ => return };
            let Ok(schema) = locator.event_schema(record) else { return };
            let p = Parser::create(record, &schema);
            let (Ok(pid), Ok(size)) = (p.try_parse::<u32>("PID"), p.try_parse::<u32>("size")) else { return };
            if ["daddr", "saddr"].iter().any(|f| p.try_parse::<IpAddr>(f).is_ok_and(|a| a.to_canonical().is_loopback())) {
                return;
            }
            let size = size as u64;
            let (r, t) = if rx { (size, 0) } else { (0, size) };
            acc.lock().unwrap_or_else(|e| e.into_inner()).observe(SEQ.fetch_add(1, Ordering::Relaxed), pid, r, t);
        }
    };
    let _ = stop_trace_by_name(TRACE_NAME); // left over from a crashed run
    KernelTrace::new()
        .named(TRACE_NAME.into())
        .enable(Provider::kernel(&TCP_IP_PROVIDER).add_callback(callback(acc.clone())).build())
        .enable(Provider::kernel(&UDP_IP_PROVIDER).add_callback(callback(acc)).build())
        .start_and_process()
        .map_err(|e| format!("ETW kernel trace: {e:?}"))
}

/// Events arrive on the ETW thread, so there is nothing to poll.
pub fn poll(_acc: &mut Acc) {}

pub fn exe_path(pid: u32) -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    if pid == 0 {
        return None;
    }
    let mut buf = vec![0u16; 32768];
    let mut len = buf.len() as u32;
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        (ok != 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

static OWNER: OnceLock<String> = OnceLock::new();
windows_service::define_windows_service!(ffi_service_main, service_main);

/// SCM entry point: `service --owner <user>` comes from the service's binPath.
pub fn service(owner: String) -> Result<(), String> {
    OWNER.set(owner).ok();
    windows_service::service_dispatcher::start(SERVICE_NAME, ffi_service_main).map_err(|e| e.to_string())
}

fn service_main(_args: Vec<std::ffi::OsString>) {
    use windows_service::service::{ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType};
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    let (tx, rx) = mpsc::channel();
    let handler = move |event| match event {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            let _ = tx.send(());
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    let Ok(status) = service_control_handler::register(SERVICE_NAME, handler) else { return };
    let set = |state, accept, code| {
        let _ = status.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: accept,
            exit_code: ServiceExitCode::Win32(code),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        });
    };
    set(ServiceState::Running, ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN, 0);
    let result = crate::run(OWNER.get().map_or("", |s| s.as_str()), rx);
    if let Err(e) = &result {
        eprintln!("overlook-helper: {e}");
    }
    set(ServiceState::Stopped, ServiceControlAccept::empty(), if result.is_ok() { 0 } else { 1 });
}
