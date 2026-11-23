//! `builtin_files.rs`: the screens and parts Kibann comes with (`screens/`,
//! `parts/`), embedded.

use std::fs;
use std::path::{Path, PathBuf};

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
    embed(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
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
