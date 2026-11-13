//! What a clicked link does. Links come from program output, so anything that
//! could run code when opened is shown in Finder instead.

use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub enum Open {
    /// Hand to the OS: browser, mail app, and so on.
    Url(String),
    /// Open with the file's default app (Finder for a directory).
    Path(PathBuf),
    /// Select it in Finder (or the platform's file manager) without opening it.
    Reveal(PathBuf),
    Ignore,
}

const URL_SCHEMES: &[&str] = &[
    "http://", "https://", "mailto:", "ftp://", "ssh://", "git://",
];

/// Bundles and documents that run something when opened.
const RISKY_EXTENSIONS: &[&str] = &[
    "app",
    "command",
    "tool",
    "terminal",
    "workflow",
    "action",
    "pkg",
    "mpkg",
    "scpt",
    "scptd",
    "applescript",
    "jar",
    "webloc",
    "inetloc",
    "fileloc",
    "url",
    "desktop",
    "exe",
    "bat",
    "cmd",
    "com",
    "msi",
    "ps1",
    "vbs",
    "lnk",
];

fn risky(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if ext.is_some_and(|e| RISKY_EXTENSIONS.contains(&e.as_str())) {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(m) = std::fs::metadata(path)
            && m.is_file()
            && m.permissions().mode() & 0o111 != 0
        {
            return true;
        }
    }
    false
}

/// Decide what opening `target` (a URL or an absolute path) does.
pub fn plan(target: &str) -> Open {
    let lower = target.get(..10).unwrap_or(target).to_ascii_lowercase();
    if URL_SCHEMES.iter().any(|s| lower.starts_with(s)) {
        return Open::Url(target.to_owned());
    }
    let path = if lower.starts_with("file://") {
        match term::process::parse_reported_dir(target.as_bytes()) {
            Some(p) => p,
            None => return Open::Ignore,
        }
    } else if Path::new(target).is_absolute() {
        PathBuf::from(target)
    } else {
        return Open::Ignore;
    };
    if !path.exists() {
        Open::Ignore
    } else if risky(&path) {
        Open::Reveal(path)
    } else {
        Open::Path(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_open_only_with_known_schemes() {
        assert_eq!(plan("https://x.com/a"), Open::Url("https://x.com/a".into()));
        assert_eq!(plan("MAILTO:a@b.c"), Open::Url("MAILTO:a@b.c".into()));
        assert_eq!(plan("javascript:alert(1)"), Open::Ignore);
        assert_eq!(plan("x-apple.systempreferences:foo"), Open::Ignore);
        assert_eq!(plan("relative/path"), Open::Ignore);
    }

    #[test]
    fn files_open_unless_they_could_run_something() {
        let dir = std::env::temp_dir().join(format!("kibann-open-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Thing.app")).unwrap();
        std::fs::write(dir.join("notes.md"), "").unwrap();
        std::fs::write(dir.join("run.command"), "").unwrap();
        std::fs::write(dir.join("script"), "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.join("script"), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        let p = |name: &str| dir.join(name);
        let s = |name: &str| p(name).to_string_lossy().into_owned();

        assert_eq!(plan(&s("notes.md")), Open::Path(p("notes.md")));
        assert_eq!(plan(dir.to_str().unwrap()), Open::Path(dir.clone()));
        assert_eq!(plan(&s("Thing.app")), Open::Reveal(p("Thing.app")));
        assert_eq!(plan(&s("run.command")), Open::Reveal(p("run.command")));
        #[cfg(unix)]
        assert_eq!(plan(&s("script")), Open::Reveal(p("script")));
        assert_eq!(plan(&s("missing")), Open::Ignore);
        // A Unix path makes a file URL as is; Windows ones (`C:\...`) don't
        // yet.
        #[cfg(unix)]
        {
            let url = format!("file://localhost{}", s("notes.md").replace(' ', "%20"));
            assert_eq!(plan(&url), Open::Path(p("notes.md")));
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
