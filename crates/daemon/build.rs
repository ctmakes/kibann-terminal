//! `KIBANN_DAEMON_BUILD`: a fingerprint of the source the daemon is built
//! from. The app restarts a running daemon only when its fingerprint
//! differs, so an update that leaves the terminal code alone keeps the
//! shells (see `daemon::BUILD`).
//!
//! Also `builtin_files.rs`: the screens and parts Kibann comes with
//! (`screens/`, `parts/`), embedded. They're the app's and the CLI's, not
//! the running daemon's, so they don't count toward the fingerprint.

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// What the daemon is built from, relative to the workspace root: the
/// crates it's made of. Dependency versions come from `Cargo.lock`, but only
/// the packages this crate depends on (see [`locked`]): the app's version and
/// its own dependencies change with every release and say nothing about the
/// terminal. The built-in workspaces and skills are left out too; only the
/// CLI and the app use them, and those always run the new binary.
///
/// Still counted, though the running daemon doesn't use them: the CLI and
/// workspace-file code in `crates/daemon/src`, until they're crates of
/// their own.
const INPUTS: &[&str] = &[
    "crates/daemon/src",
    "crates/daemon/Cargo.toml",
    "crates/term/src",
    "crates/term/Cargo.toml",
    "crates/proto/src",
    "crates/proto/Cargo.toml",
    "crates/git/src",
    "crates/git/Cargo.toml",
];

fn files(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for e in entries.flatten() {
            files(&e.path(), out);
        }
    } else if path.is_file() {
        out.push(path.to_owned());
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut all = Vec::new();
    for input in INPUTS {
        let path = root.join(input);
        println!("cargo:rerun-if-changed={}", path.display());
        files(&path, &mut all);
    }
    all.sort();
    let lock = root.join("Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock.display());
    let mut hash = Sha256::new();
    hash.update(locked(
        &fs::read_to_string(&lock).expect("Cargo.lock"),
        "daemon",
    ));
    for f in &all {
        let rel = f.strip_prefix(&root).unwrap_or(f);
        hash.update(rel.to_string_lossy().as_bytes());
        hash.update([0]);
        // Line endings depend on the checkout (Git on Windows), not the code.
        let bytes = fs::read(f).unwrap_or_default();
        let bytes: Vec<u8> = bytes.into_iter().filter(|&b| b != b'\r').collect();
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(&bytes);
    }
    embed(&root);
    let digest = hash.finalize();
    let hex: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    println!("cargo:rustc-env=KIBANN_DAEMON_BUILD={hex}");
}

/// The `Cargo.lock` entries of `root` and everything it depends on, one
/// line each (`name version source checksum`), sorted. A `dependencies` item
/// is `name`, or `name version` when the lock has several versions of it.
fn locked(lock: &str, root: &str) -> Vec<u8> {
    let table: toml::Table = lock.parse().expect("Cargo.lock is TOML");
    let packages = table
        .get("package")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();
    let field =
        |p: &toml::Value, k: &str| p.get(k).and_then(|v| v.as_str()).unwrap_or("").to_owned();
    let find = |dep: &str| {
        let mut words = dep.split(' ');
        let name = words.next().unwrap_or("");
        let version = words.next();
        packages
            .iter()
            .find(|p| field(p, "name") == name && version.is_none_or(|v| field(p, "version") == v))
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut todo = vec![root.to_owned()];
    while let Some(dep) = todo.pop() {
        let Some(p) = find(&dep) else { continue };
        let line = ["name", "version", "source", "checksum"]
            .map(|k| field(p, k))
            .join(" ");
        if !seen.insert(line) {
            continue;
        }
        for d in p
            .get("dependencies")
            .and_then(|d| d.as_array())
            .into_iter()
            .flatten()
        {
            if let Some(d) = d.as_str() {
                todo.push(d.to_owned());
            }
        }
    }
    assert!(!seen.is_empty(), "{root} isn't in Cargo.lock");
    seen.into_iter()
        .flat_map(|l| [l.into_bytes(), b"\n".to_vec()])
        .flatten()
        .collect()
}

/// Write `$OUT_DIR/builtin_files.rs`: `FILES`, every file under `screens/`
/// and `parts/` by its path from the root, with its bytes.
fn embed(root: &Path) {
    let mut all = Vec::new();
    for dir in ["screens", "parts"] {
        let path = root.join(dir);
        println!("cargo:rerun-if-changed={}", path.display());
        files(&path, &mut all);
    }
    all.sort();
    let mut out = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    for f in &all {
        let rel = f.strip_prefix(root).unwrap_or(f);
        let rel = rel.to_string_lossy().replace('\\', "/");
        if rel.split('/').any(|p| p.starts_with('.')) {
            continue;
        }
        let abs = fs::canonicalize(f).unwrap_or_else(|_| f.clone());
        out.push_str(&format!(
            "    ({rel:?}, include_bytes!({:?})),\n",
            abs.to_string_lossy()
        ));
    }
    out.push_str("];\n");
    let dest = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("builtin_files.rs");
    fs::write(dest, out).unwrap();
}
