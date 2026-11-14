//! The installed monospaced font families (macOS, Core Text), for
//! Settings → Appearance. Elsewhere Settings takes a typed name.

use objc2_core_foundation::{CFArray, CFRetained, CFString};
use objc2_core_text::{CTFont, CTFontManagerCopyAvailableFontFamilyNames, CTFontSymbolicTraits};

/// Families whose regular face is monospaced, sorted, without the hidden
/// system ones (names starting with ".").
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

#[cfg(test)]
mod tests {
    #[test]
    fn finds_a_monospaced_font_and_no_proportional_one() {
        let all = super::monospaced();
        assert!(all.iter().any(|f| f == "Menlo"), "{all:?}");
        assert!(!all.iter().any(|f| f == "Helvetica"), "{all:?}");
    }
}
