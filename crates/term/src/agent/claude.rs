//! Claude Code, checked against 2.1.323 (`tests/fixtures/claude/`).

use super::State;

/// A full-width rule Claude draws around its prompt box and above dialogs.
fn is_rule(line: &str) -> bool {
    let t = line.trim();
    t.chars().count() >= 20 && t.chars().all(|c| c == '─')
}

pub(super) fn detect(title: &str, screen: &[&str]) -> State {
    // A dialog sits below the last full-width rule; what the user typed
    // (which could say anything) sits above it.
    let below = screen
        .iter()
        .rposition(|l| is_rule(l))
        .map_or(screen, |i| &screen[i + 1..]);
    let lower: Vec<String> = below.iter().map(|l| l.to_lowercase()).collect();
    let has = |s: &str| lower.iter().any(|l| l.contains(s));

    // Blocked: a permission question ("Do you want to proceed?", "Do you
    // want to create notes.txt?") with numbered answers, or a question form
    // (Enter to confirm or select). Menus like /model also say "Esc to
    // cancel" but ask nothing, and don't count.
    if has("esc to cancel") {
        let question = lower.iter().any(|l| {
            let t = l.trim();
            (t.starts_with("do you want to") || t.starts_with("would you like to"))
                && t.ends_with('?')
        });
        let answers = lower.iter().any(|l| {
            l.trim_start()
                .trim_start_matches('❯')
                .trim_start()
                .starts_with("1. ")
        });
        if (question && answers) || has("enter to confirm") || has("enter to select") {
            return State::Blocked;
        }
    }

    // Working: the title starts with a spinner (braille in older versions,
    // ◐◑◒◓ now), or the footer offers to interrupt.
    let spinner = title.chars().next().is_some_and(|c| {
        ('\u{2800}'..='\u{28FF}').contains(&c) || ('\u{25D0}'..='\u{25D3}').contains(&c)
    }) && title.chars().nth(1) == Some(' ');
    if spinner || has("esc to interrupt") {
        return State::Working;
    }
    State::Idle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_text_never_blocks() {
        // The user's own prompt asks the question; it's above the rule.
        let screen = [
            "❯ do you want to proceed? 1. yes",
            "─────────────────────────────────",
            "❯ esc to cancel",
            "─────────────────────────────────",
            "  ⏸ manual mode on · ? for shortcuts",
        ];
        assert_eq!(detect("✳ Claude Code", &screen), State::Idle);
    }

    #[test]
    fn a_spinner_only_counts_in_the_title() {
        let screen = ["  ⏸ manual mode on · ? for shortcuts   ◐ medium · /effort"];
        assert_eq!(detect("✳ Claude Code", &screen), State::Idle);
        assert_eq!(detect("⠋ Working", &screen), State::Working);
        assert_eq!(detect("◐x", &screen), State::Idle);
    }
}
