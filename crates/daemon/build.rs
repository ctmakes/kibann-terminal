//! `KIBANN_DAEMON_BUILD`: a fingerprint of the source the daemon is built
//! from. The app restarts a running daemon only when its fingerprint
//! differs, so an update that leaves the terminal code alone keeps the
//! shells (see `daemon::BUILD`).

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// What the daemon is built from, relative to the workspace root: the
/// crates it's made of and the files they include. `Cargo.lock` covers
/// dependency versions (an update to an app-only one counts too).
const INPUTS: &[&str] = &[
    "crates/daemon/src",
    "crates/daemon/Cargo.toml",
    "crates/term/src",
    "crates/term/Cargo.toml",
    "crates/proto/src",
    "crates/proto/Cargo.toml",
    "crates/git/src",
    "crates/git/Cargo.toml",
    "layouts",
    "skills",
    "Cargo.lock",
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
    let mut hash = Sha256::new();
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
    let digest = hash.finalize();
    let hex: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    println!("cargo:rustc-env=KIBANN_DAEMON_BUILD={hex}");
}
