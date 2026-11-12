//! What the OS knows about the programs in a session: working directory and
//! name, for new panes and titles.

use std::path::PathBuf;

/// The working directory of process `pid`.
#[cfg(target_os = "macos")]
pub fn cwd(pid: i32) -> Option<PathBuf> {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStrExt;

    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as i32;
    // SAFETY: `info` is a valid buffer of `size` bytes for this flavor.
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            (&raw mut info).cast(),
            size,
        )
    };
    if n != size {
        return None;
    }
    // SAFETY: the kernel NUL-terminates the path within the buffer.
    let path = unsafe { CStr::from_ptr(info.pvi_cdir.vip_path.as_ptr().cast()) };
    let bytes = path.to_bytes();
    (!bytes.is_empty()).then(|| PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
}

#[cfg(target_os = "linux")]
pub fn cwd(pid: i32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn cwd(_pid: i32) -> Option<PathBuf> {
    None
}

/// The short name of process `pid` (`zsh`, `vim`).
#[cfg(target_os = "macos")]
pub fn name(pid: i32) -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: `buf` is valid for its length.
    let n = unsafe { libc::proc_name(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    (n > 0).then(|| String::from_utf8_lossy(&buf[..n as usize]).into_owned())
}

#[cfg(target_os = "linux")]
pub fn name(pid: i32) -> Option<String> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    Some(s.trim_end().to_owned())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn name(_pid: i32) -> Option<String> {
    None
}

/// A directory reported by the shell: OSC 7's `file://host/path` with
/// percent escapes, or a bare absolute path (OSC 9;9, OSC 1337).
pub fn parse_reported_dir(raw: &[u8]) -> Option<PathBuf> {
    let s = std::str::from_utf8(raw).ok()?.trim();
    let path = if let Some(rest) = s.strip_prefix("file://") {
        // Skip the host; the path starts at the next slash.
        &rest[rest.find('/')?..]
    } else if s.starts_with('/') {
        s
    } else {
        return None;
    };
    let mut out = Vec::with_capacity(path.len());
    let b = path.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    let decoded = String::from_utf8(out).ok()?;
    Some(PathBuf::from(decoded))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reported_dirs() {
        let p = |s: &str| parse_reported_dir(s.as_bytes());
        assert_eq!(
            p("file://mac.local/Users/a/My%20Dir"),
            Some("/Users/a/My Dir".into())
        );
        assert_eq!(p("file:///tmp"), Some("/tmp".into()));
        assert_eq!(p("/opt/x"), Some("/opt/x".into()));
        assert_eq!(p("relative"), None);
        assert_eq!(p("file://host"), None);
        assert_eq!(p("/a%2"), Some("/a%2".into()));
        assert_eq!(p("/caf%C3%A9/%é"), Some("/café/%é".into()));
    }

    #[cfg(unix)]
    #[test]
    fn own_process() {
        let me = std::process::id() as i32;
        assert_eq!(cwd(me), std::env::current_dir().ok());
        assert!(name(me).is_some_and(|n| !n.is_empty()));
    }
}
