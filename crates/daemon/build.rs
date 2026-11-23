//! `KIBANN_DAEMON_BUILD`: a fingerprint of what the daemon is built from.
//! The app restarts a running daemon only when its fingerprint differs, so
//! an update that leaves the terminal code alone keeps the shells (see
//! `daemon::BUILD`).
//!
//! What counts is found from `Cargo.lock`: this crate and every package it
//! depends on. For a package from a registry or git, its version and
//! checksum; for one of ours (a crate under `crates/`), its source. The
//! app, the CLI and the user's files (workspaces, screens, parts, skills)
//! aren't among them, so changing those keeps the daemon.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

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
    let lock = root.join("Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock.display());
    let (locked, ours) = closure(&fs::read_to_string(&lock).expect("Cargo.lock"), "daemon");
    let mut hash = Sha256::new();
    for line in &locked {
        hash.update(line.as_bytes());
        hash.update(b"\n");
    }
    let mut all = Vec::new();
    for name in &ours {
        let dir = root.join("crates").join(name);
        assert!(
            dir.is_dir(),
            "{name} is ours (no source in Cargo.lock) but isn't in crates/"
        );
        for input in ["src", "Cargo.toml", "build.rs"] {
            let path = dir.join(input);
            println!("cargo:rerun-if-changed={}", path.display());
            files(&path, &mut all);
        }
    }
    all.sort();
    for f in &all {
        let rel = f.strip_prefix(&root).unwrap_or(f);
        hash.update(rel.to_string_lossy().replace('\\', "/").as_bytes());
        hash.update([0]);
        // Line endings depend on the checkout (Git on Windows), not the code.
        let bytes = fs::read(f).unwrap_or_default();
        let bytes: Vec<u8> = bytes.into_iter().filter(|&b| b != b'\r').collect();
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(&bytes);
    }
    let digest = hash.finalize();
    let hex: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    println!("cargo:rustc-env=KIBANN_DAEMON_BUILD={hex}");
}

/// `root` and everything it depends on in `Cargo.lock`: one line for each
/// package from elsewhere (`name version source checksum`), and the names
/// of ours (no source). A `dependencies` item is `name`, or `name version`
/// when the lock has several versions of it.
fn closure(lock: &str, root: &str) -> (BTreeSet<String>, BTreeSet<String>) {
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
    let (mut locked, mut ours) = (BTreeSet::new(), BTreeSet::new());
    let mut todo = vec![root.to_owned()];
    while let Some(dep) = todo.pop() {
        let Some(p) = find(&dep) else { continue };
        let new = if p.get("source").is_none() {
            ours.insert(field(p, "name"))
        } else {
            locked.insert(
                ["name", "version", "source", "checksum"]
                    .map(|k| field(p, k))
                    .join(" "),
            )
        };
        if !new {
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
    assert!(ours.contains(root), "{root} isn't in Cargo.lock");
    (locked, ours)
}
