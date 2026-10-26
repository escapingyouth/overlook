//! Installs/removes the privileged per-app counter (`overlook-helper`) as a system service, with one
//! elevation prompt per toggle. Plain std so it can be checked for every target without Tauri.
//! Variable data (paths, uid, username) never becomes script text: the scripts below are constants and
//! the data travels as separate argv entries.
// Each OS's builders compile everywhere so their tests run on any host.
#![allow(dead_code)]

/// Linux: `sh -c` script run by pkexec. `$1` = sidecar path, `$2` = owner uid.
const LINUX_INSTALL: &str = r#"set -e
case "$2" in ''|*[!0-9]*) exit 2 ;; esac
install -Dm755 "$1" /usr/local/libexec/overlook-helper
cat > /etc/systemd/system/overlook-helper.service <<EOF
[Unit]
Description=Overlook per-app counter

[Service]
ExecStart=/usr/local/libexec/overlook-helper run --owner $2
Restart=always

[Install]
WantedBy=multi-user.target
EOF
systemctl daemon-reload
systemctl enable --now overlook-helper
"#;

const LINUX_UNINSTALL: &str = r#"systemctl disable --now overlook-helper || true
rm -f /etc/systemd/system/overlook-helper.service /usr/local/libexec/overlook-helper
systemctl daemon-reload
"#;

/// Linux: argv for `pkexec`.
pub fn linux_argv(install: bool, sidecar: &str, uid: u32) -> Vec<String> {
    let mut v: Vec<String> = vec!["/bin/sh".into(), "-c".into()];
    if install {
        v.extend([LINUX_INSTALL.into(), "sh".into(), sidecar.into(), uid.to_string()]);
    } else {
        v.extend([LINUX_UNINSTALL.into(), "sh".into()]);
    }
    v
}

/// Windows: the elevated script. Its params arrive as `x<hex of UTF-8>` tokens (see `windows_elevated_args`).
/// New-Service is used rather than `sc create` because Windows PowerShell 5.1 mangles embedded quotes in
/// native-command arguments, and the binPath needs quotes around the exe and the username.
const WIN_INSTALL: &str = r#"param($s, $u)
$ErrorActionPreference = 'Stop'
function d($h) { $h = $h.Substring(1); $b = New-Object byte[] ($h.Length / 2); for ($i = 0; $i -lt $b.Length; $i++) { $b[$i] = [Convert]::ToByte($h.Substring(2 * $i, 2), 16) }; [Text.Encoding]::UTF8.GetString($b) }
$src = d $s; $user = d $u
if ($user -notmatch '^[\w .\\@$-]{1,256}\z' -or $user -match '^[-/*]') { exit 2 }
if (Get-Service OverlookHelper -ErrorAction SilentlyContinue) { Stop-Service OverlookHelper -Force; sc.exe delete OverlookHelper | Out-Null }
$dir = Join-Path $env:ProgramFiles 'Overlook Helper'
$exe = Join-Path $dir 'overlook-helper.exe'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
Copy-Item -LiteralPath $src -Destination $exe -Force
New-Service -Name OverlookHelper -DisplayName 'Overlook Helper' -BinaryPathName ('"' + $exe + '" service --owner "' + $user + '"') -StartupType Automatic | Out-Null
Start-Service OverlookHelper
"#;

const WIN_UNINSTALL: &str = r#"$ErrorActionPreference = 'Stop'
if (Get-Service OverlookHelper -ErrorAction SilentlyContinue) { Stop-Service OverlookHelper -Force; sc.exe delete OverlookHelper | Out-Null; if ($LASTEXITCODE) { exit $LASTEXITCODE } }
$dir = Join-Path $env:ProgramFiles 'Overlook Helper'
if (Test-Path -LiteralPath $dir) { Remove-Item -LiteralPath $dir -Recurse -Force }
"#;

/// Non-elevated PowerShell: elevates one PowerShell with `$env:OVERLOOK_ARGS` (space-joined, space-free tokens).
/// A cancelled UAC prompt throws, which becomes exit 1.
pub const WIN_OUTER: &str = r#"try { $p = Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe') -Verb RunAs -Wait -PassThru -WindowStyle Hidden -ArgumentList ($env:OVERLOOK_ARGS -split ' '); exit $p.ExitCode } catch { exit 1 }"#;

/// Same rules as the helper's `Owner::parse` on Windows.
pub fn valid_windows_user(s: &str) -> bool {
    !s.is_empty() && s.len() <= 256 && !s.starts_with(['/', '-', '*'])
        && s.chars().all(|c| c.is_alphanumeric() || " ._-\\@$".contains(c))
}

fn hex_token(s: &str) -> String {
    let mut t = String::from("x");
    for b in s.bytes() {
        t.push_str(&format!("{b:02x}"));
    }
    t
}

fn base64(s: &str) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in s.as_bytes().chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() { A[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

/// Windows: `-ArgumentList` entries for the elevated powershell.exe. Start-Process joins entries with spaces and
/// does no quoting, so every entry is space- and quote-free: the fixed script is decoded from base64 (keeps the
/// whole command line well under ShellExecute's ~2K limit) inside a space-free bootstrap, and the data follow as hex tokens that `-Command` hands to the script block as args.
pub fn windows_elevated_args(install: bool, sidecar: &str, user: &str) -> Vec<String> {
    let script = if install { WIN_INSTALL } else { WIN_UNINSTALL };
    let boot = format!("&([scriptblock]::Create([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{}'))))", base64(script));
    let mut v: Vec<String> = ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command"].map(String::from).into();
    v.push(boot);
    if install {
        v.extend([hex_token(sidecar), hex_token(user)]);
    }
    v
}

fn sidecar() -> Result<std::path::PathBuf, String> {
    let name = if cfg!(windows) { "overlook-helper.exe" } else { "overlook-helper" };
    let path = std::env::current_exe().map_err(|e| e.to_string())?.parent().ok_or("no exe dir")?.join(name);
    if path.exists() { Ok(path) } else { Err(format!("{} is missing from this install", path.display())) }
}

fn ok(status: std::io::Result<std::process::ExitStatus>) -> Result<(), String> {
    match status {
        Ok(s) if s.success() => Ok(()),
        _ => Err("Permission wasn't granted".into()),
    }
}

#[cfg(target_os = "linux")]
pub fn set_installed(install: bool) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let mut src = std::path::PathBuf::new();
    let mut tmp = None;
    if install {
        src = sidecar()?;
        // An AppImage's FUSE mount isn't readable by root, so stage a copy outside it.
        if std::env::var_os("APPIMAGE").is_some() {
            let t = std::env::temp_dir().join(format!("overlook-helper-{}", std::process::id()));
            std::fs::copy(&src, &t).map_err(|e| e.to_string())?;
            src = t.clone();
            tmp = Some(t);
        }
    }
    let uid = std::fs::metadata("/proc/self").map_err(|e| e.to_string())?.uid();
    let argv = linux_argv(install, &src.to_string_lossy(), uid);
    let res = ok(std::process::Command::new("pkexec").args(&argv).status());
    if let Some(t) = tmp {
        std::fs::remove_file(t).ok();
    }
    res
}

#[cfg(windows)]
pub fn set_installed(install: bool) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let (src, user) = if install {
        let user = match std::env::var("USERDOMAIN") {
            Ok(d) if !d.is_empty() => format!("{d}\\{}", std::env::var("USERNAME").unwrap_or_default()),
            _ => std::env::var("USERNAME").unwrap_or_default(),
        };
        if !valid_windows_user(&user) {
            return Err(format!("Unsupported user name {user:?}"));
        }
        (sidecar()?.to_string_lossy().into_owned(), user)
    } else {
        (String::new(), String::new())
    };
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let ps = std::path::Path::new(&root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let status = std::process::Command::new(ps)
        .args(["-NoProfile", "-NonInteractive", "-Command", WIN_OUTER])
        .env("OVERLOOK_ARGS", windows_elevated_args(install, &src, &user).join(" "))
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .status();
    ok(status)
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn set_installed(_install: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NASTY: &str = r#"/tmp/a b'; rm -rf / #"$(id)`x`"#;

    #[test]
    fn linux_data_only_as_separate_argv() {
        let v = linux_argv(true, NASTY, 1000);
        assert_eq!(v[..2], ["/bin/sh", "-c"]);
        assert_eq!(v[2], LINUX_INSTALL);
        assert_eq!(v[3..], ["sh", NASTY, "1000"]);
        assert!(!LINUX_INSTALL.contains("/tmp/a"));
        assert_eq!(linux_argv(false, NASTY, 1000), ["/bin/sh", "-c", LINUX_UNINSTALL, "sh"]);
    }

    #[test]
    fn windows_entries_are_space_and_quote_free_and_data_is_hex_only() {
        let user = r#"DOM\Jane O'Neil"#;
        let v = windows_elevated_args(true, NASTY, user);
        for e in &v {
            assert!(!e.contains([' ', '"', '\t']), "{e}");
            assert!(!e.contains("Jane") && !e.contains("/tmp"), "{e}");
        }
        assert_eq!(v[v.len() - 2], hex_token(NASTY));
        assert_eq!(v[v.len() - 1], hex_token(user));
        assert!(v[v.len() - 1].starts_with('x') && v[v.len() - 1][1..].bytes().all(|b| b.is_ascii_hexdigit()));
        // The bootstrap carries only the constant script.
        assert!(v[5].contains(&base64(WIN_INSTALL)));
        assert_eq!(windows_elevated_args(false, NASTY, user).len(), 6);
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64("a"), "YQ==");
        assert_eq!(base64("hi"), "aGk=");
        assert_eq!(base64("abc"), "YWJj");
        assert_eq!(base64("abcd"), "YWJjZA==");
    }

    #[test]
    fn windows_user_validation() {
        assert!(valid_windows_user(r"DESKTOP-1\Jane Doe"));
        assert!(!valid_windows_user(""));
        assert!(!valid_windows_user("-x"));
        assert!(!valid_windows_user("a\"b"));
        assert!(!valid_windows_user("a;b"));
    }
}
