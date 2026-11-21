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
    "workspaces",
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
    embed(&root);
    let digest = hash.finalize();
    let hex: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    println!("cargo:rustc-env=KIBANN_DAEMON_BUILD={hex}");
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
