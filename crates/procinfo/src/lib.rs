//! What the OS says about a process: its working directory, name and
//! arguments, for titles, new panes' directories and telling agents apart;
//! and, on Windows, which process runs in a console's foreground.
//!
//! Its own crate (no terminal emulator) so its Windows half builds and is
//! checked anywhere: `cargo check -p procinfo --target x86_64-pc-windows-msvc`.

#[cfg(not(windows))]
use std::path::PathBuf;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{argv, cwd, name, newest_descendant};

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

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
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

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub fn name(_pid: i32) -> Option<String> {
    None
}

/// The arguments process `pid` was started with, `argv[0]` first. Tells
/// apart programs whose process name says little (a native binary named
/// after its version, or `node` running a script).
#[cfg(target_os = "macos")]
pub fn argv(pid: i32) -> Option<Vec<String>> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut size: libc::size_t = 0;
    // SAFETY: a size query (null buffer) for a valid mib.
    let r = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if r != 0 || size < 4 {
        return None;
    }
    let mut buf = vec![0u8; size];
    // SAFETY: `buf` holds `size` bytes.
    let r = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buf.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if r != 0 {
        return None;
    }
    parse_procargs2(&buf[..size])
}

/// `KERN_PROCARGS2`: argc (native-endian i32), the executable path, NUL
/// padding, then argc NUL-terminated arguments (then the environment).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_procargs2(buf: &[u8]) -> Option<Vec<String>> {
    let argc = i32::from_ne_bytes(buf.get(..4)?.try_into().ok()?);
    let rest = &buf[4..];
    let exe_end = rest.iter().position(|&b| b == 0)?;
    let mut i = exe_end;
    while rest.get(i) == Some(&0) {
        i += 1;
    }
    let args = rest[i..]
        .split(|&b| b == 0)
        .take(usize::try_from(argc).ok()?)
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect();
    Some(args)
}

#[cfg(target_os = "linux")]
pub fn argv(pid: i32) -> Option<Vec<String>> {
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let mut args: Vec<String> = raw
        .split(|&b| b == 0)
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect();
    if args.last().is_some_and(String::is_empty) {
        args.pop();
    }
    Some(args)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub fn argv(_pid: i32) -> Option<Vec<String>> {
    None
}

/// The newest process started under `shell` (its children, theirs…), on
/// Windows, where a console has no foreground process group to ask.
#[cfg(not(windows))]
pub fn newest_descendant(_shell: i32) -> Option<i32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn own_process() {
        let me = std::process::id() as i32;
        assert_eq!(cwd(me), std::env::current_dir().ok());
        assert!(name(me).is_some_and(|n| !n.is_empty()));
        let args = argv(me).unwrap();
        let first = std::env::args().next().unwrap();
        assert_eq!(args[0], first);
        assert_eq!(args.len(), std::env::args().count());
    }

    #[test]
    fn procargs2_layout() {
        let mut buf = 2i32.to_ne_bytes().to_vec();
        buf.extend_from_slice(b"/usr/bin/x\0\0\0claude\0--resume\0HOME=/h\0");
        assert_eq!(
            parse_procargs2(&buf),
            Some(vec!["claude".to_owned(), "--resume".to_owned()])
        );
        assert_eq!(parse_procargs2(&[1, 0]), None);
    }
}
