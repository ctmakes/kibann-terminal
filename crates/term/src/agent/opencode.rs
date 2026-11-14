//! OpenCode, checked against 2.2.7 (`tests/fixtures/opencode/`).
//!
//! Its screen ends in the prompt box, closed by a `╹▀▀▀…` border, with a
//! footer line below it. A permission or question dialog takes the prompt
//! box's place, so:
//! - the border on screen means no dialog is open, and what the user typed
//!   (which could say anything) stays above it;
//! - without the border, the dialog is the `┃` block at the bottom.

use super::State;

/// `opencode`'s subcommands that aren't its interactive screen (2.2.7's
/// `--help`; `mini` is a smaller interactive screen, so it counts).
const NOT_INTERACTIVE: &[&str] = &[
    "upgrade",
    "update",
    "uninstall",
    "acp",
    "api",
    "debug",
    "auth",
    "mcp",
    "plugin",
    "models",
    "stats",
    "run",
    "session",
    "service",
    "reload",
    "pair",
    "serve",
];

/// Whether `opencode` with these arguments (after the program) opens its
/// interactive screen. A subcommand comes first, before any flags.
pub(super) fn interactive(args: &[String]) -> bool {
    !args
        .first()
        .is_some_and(|a| NOT_INTERACTIVE.contains(&a.as_str()))
}

/// The bottom border of the prompt box.
fn is_border(line: &str) -> bool {
    let t = line.trim();
    t.strip_prefix('╹')
        .is_some_and(|rest| rest.chars().count() >= 20 && rest.chars().all(|c| c == '▀'))
}

pub(super) fn detect(_title: &str, screen: &[&str]) -> State {
    let lower: Vec<String> = screen.iter().map(|l| l.to_lowercase()).collect();

    if let Some(i) = lower.iter().rposition(|l| is_border(l)) {
        // Working: the footer offers to interrupt ("esc interrupt", then
        // "esc again to interrupt" after one Esc).
        let footer = &lower[i + 1..];
        let working = footer
            .iter()
            .any(|l| l.contains("esc interrupt") || l.contains("esc again to interrupt"));
        return if working { State::Working } else { State::Idle };
    }

    // The dialog: the `┃` lines at the bottom, without the bar.
    let end = lower
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .map_or(0, |i| i + 1);
    let start = lower[..end]
        .iter()
        .rposition(|l| !l.trim_start().starts_with('┃'))
        .map_or(0, |i| i + 1);
    let dialog: Vec<&str> = lower[start..end]
        .iter()
        .map(|l| l.trim_start().trim_start_matches('┃').trim())
        .collect();
    let has = |s: &str| dialog.iter().any(|l| l.contains(s));

    // A permission: "△ Permission required" and its answers.
    let permission = dialog
        .iter()
        .any(|l| l.starts_with("△ permission required"))
        && dialog
            .iter()
            .any(|l| l.contains("allow once") && l.contains("reject"));
    // A question: numbered answers and "enter submit".
    let question = has("enter submit") && dialog.iter().any(|l| l.starts_with("1. "));
    if permission || question {
        State::Blocked
    } else {
        State::Idle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BORDER: &str = "  ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀";

    #[test]
    fn typed_text_never_blocks() {
        let screen = [
            "  ┃",
            "  ┃  △ Permission required",
            "  ┃   Allow once   Always allow   Reject",
            "  ┃  1. yes   enter submit",
            "  ┃  Build · Big Pickle OpenCode",
            BORDER,
            "  /tmp/x                                 ctrl+p commands",
        ];
        assert_eq!(detect("OpenCode", &screen), State::Idle);
    }

    #[test]
    fn interrupt_only_counts_in_the_footer() {
        let mut screen = vec![
            "  ┃  esc interrupt",
            "  ┃",
            BORDER,
            "  /tmp/x  ctrl+p commands",
        ];
        assert_eq!(detect("OpenCode", &screen), State::Idle);
        screen[3] = "   ⬝⬝⬝⬝⬝⬝⬝⬝ esc interrupt     ctrl+p commands";
        assert_eq!(detect("OpenCode", &screen), State::Working);
    }

    #[test]
    fn a_question_needs_its_answers() {
        let screen = [
            "  ┃",
            "  ┃  Questions",
            "  ┃  ↑↓ select  enter submit  esc dismiss",
        ];
        assert_eq!(detect("OC | x", &screen), State::Idle);
    }
}
