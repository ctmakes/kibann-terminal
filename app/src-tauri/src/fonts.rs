//! The installed monospaced font families, for Settings → Appearance:
//! Core Text on macOS, fontconfig (`fc-list`) on Linux. Elsewhere, or when
//! the list is empty, Settings takes a typed name.

#[cfg(target_os = "macos")]
use objc2_core_foundation::{CFArray, CFRetained, CFString};
#[cfg(target_os = "macos")]
use objc2_core_text::{CTFont, CTFontManagerCopyAvailableFontFamilyNames, CTFontSymbolicTraits};

/// Families whose regular face is monospaced, sorted, without the hidden
/// system ones (names starting with ".").
#[cfg(target_os = "macos")]
pub fn monospaced() -> Vec<String> {
    // SAFETY: Core Text's font manager and font creation are thread-safe;
    // the array holds CFStrings (family names), as documented.
    let names = unsafe {
        CFRetained::cast_unchecked::<CFArray<CFString>>(CTFontManagerCopyAvailableFontFamilyNames())
    };
    let mut out: Vec<String> = names
        .to_vec()
        .into_iter()
        .filter_map(|name| {
            let family = name.to_string();
            if family.starts_with('.') {
                return None;
            }
            // SAFETY: a null matrix means the identity.
            let font = unsafe { CTFont::with_name(&name, 12.0, std::ptr::null()) };
            let traits = unsafe { font.symbolic_traits() };
            traits
                .contains(CTFontSymbolicTraits::MonoSpaceTrait)
                .then_some(family)
        })
        .collect();
    out.sort_by_key(|f| f.to_lowercase());
    out.dedup();
    out
}

/// fontconfig's monospaced families (`spacing=mono`; charcell fonts are
/// `spacing=charcell` and rare).
#[cfg(target_os = "linux")]
pub fn monospaced() -> Vec<String> {
    let out = std::process::Command::new("fc-list")
        .args([":spacing=mono", "family"])
        .output();
    match out {
        Ok(o) if o.status.success() => families(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    }
}

/// `fc-list ... family` output: one font per line, its family names
/// comma-separated (the first is the main one; the rest are aliases or
/// translations).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn families(text: &str) -> Vec<String> {
    let mut out: Vec<String> = text
        .lines()
        .filter_map(|l| {
            let first = l.split(',').next()?.trim().to_owned();
            (!first.is_empty() && !first.starts_with('.')).then_some(first)
        })
        .collect();
    out.sort_by_key(|f| f.to_lowercase());
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn reading_fc_list() {
        let text = "DejaVu Sans Mono,DejaVu Sans Mono Book\nJetBrains Mono\nDejaVu Sans Mono\n\nNoto Sans Mono CJK JP,Noto Sans Mono CJK JP Regular\n";
        assert_eq!(
            super::families(text),
            [
                "DejaVu Sans Mono",
                "JetBrains Mono",
                "Noto Sans Mono CJK JP"
            ]
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn finds_a_monospaced_font_on_linux() {
        // Every desktop has a monospaced font; CI's runner has DejaVu.
        let all = super::monospaced();
        assert!(!all.is_empty());
        assert!(!all.iter().any(|f| f == "DejaVu Sans"), "{all:?}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn finds_a_monospaced_font_and_no_proportional_one() {
        let all = super::monospaced();
        assert!(all.iter().any(|f| f == "Menlo"), "{all:?}");
        assert!(!all.iter().any(|f| f == "Helvetica"), "{all:?}");
    }
}
