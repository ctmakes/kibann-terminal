//! Directories a session's shell reports. What the OS knows about its
//! programs is the `procinfo` crate.

use std::path::PathBuf;

/// A directory reported by the shell: OSC 7's `file://host/path` with
/// percent escapes, or a bare absolute path (OSC 9;9, OSC 1337). On
/// Windows, with a drive: `file:///C:/x`, `C:\x` and `C:/x` are `C:\x`.
pub fn parse_reported_dir(raw: &[u8]) -> Option<PathBuf> {
    reported_dir(raw, cfg!(windows))
}

fn reported_dir(raw: &[u8], windows: bool) -> Option<PathBuf> {
    let s = std::str::from_utf8(raw).ok()?.trim();
    let drive = |p: &str| {
        let b = p.as_bytes();
        b.len() >= 2
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && (b.len() == 2 || matches!(b[2], b'/' | b'\\'))
    };
    let path = if let Some(rest) = s.strip_prefix("file://") {
        // Skip the host; the path starts at the next slash.
        &rest[rest.find('/')?..]
    } else if s.starts_with('/') || (windows && drive(s)) {
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
    if windows {
        let p = decoded
            .strip_prefix('/')
            .filter(|p| drive(p))
            .unwrap_or(&decoded);
        if drive(p) {
            let mut p = p.replace('/', "\\");
            if p.len() == 2 {
                p.push('\\');
            }
            return Some(PathBuf::from(p));
        }
    }
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

    #[test]
    fn reported_dirs_with_a_drive() {
        let w = |s: &str| reported_dir(s.as_bytes(), true);
        assert_eq!(
            w("file:///C:/Users/a/My%20Dir"),
            Some(r"C:\Users\a\My Dir".into())
        );
        assert_eq!(w("file://host/d:/x"), Some(r"d:\x".into()));
        assert_eq!(w(r"C:\x\y"), Some(r"C:\x\y".into()));
        assert_eq!(w("C:/x"), Some(r"C:\x".into()));
        assert_eq!(w("C:"), Some(r"C:\".into()));
        assert_eq!(w("/tmp"), Some("/tmp".into()));
        assert_eq!(w("CC:/x"), None);
        // Not on Unix, where a drive means nothing.
        assert_eq!(reported_dir(b"C:/x", false), None);
        assert_eq!(reported_dir(b"/C:/x", false), Some("/C:/x".into()));
    }
}
