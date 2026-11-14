//! Codex CLI, checked against 0.169.0 (`tests/fixtures/codex/`).
//!
//! - Working: the title starts with a braille spinner, and before the reply
//!   streams a status line reads `• Working (2s • esc to interrupt)`.
//! - Blocked: an approval ("Would you like to run the following command?")
//!   or the folder-trust question replaces the prompt, with numbered answers
//!   and a hint on the screen's last line ("Press enter to confirm or esc to
//!   cancel"). While the prompt shows, its footer is the last line, so what
//!   the user typed can't look like a dialog.

use super::State;

/// `codex`'s subcommands that aren't its interactive screen (0.169.0's
/// `--help`; `resume`, `fork` and `agents` open it, so they count).
const NOT_INTERACTIVE: &[&str] = &[
    "exec",
    "e",
    "review",
    "login",
    "logout",
    "mcp",
    "mcp-server",
    "plugin",
    "app-server",
    "remote-control",
    "app",
    "completion",
    "update",
    "doctor",
    "sandbox",
    "debug",
    "apply",
    "a",
    "queue",
    "archive",
    "delete",
    "migrate-rollouts",
    "migrate-nfs-db",
    "unarchive",
    "cloud",
    "exec-server",
    "features",
    "help",
];

/// Whether `codex` with these arguments (after the program) opens its
/// interactive screen.
pub(super) fn interactive(args: &[String]) -> bool {
    !args
        .first()
        .is_some_and(|a| NOT_INTERACTIVE.contains(&a.as_str()))
}

/// How far above the hint a dialog's question and answers may be.
const DIALOG_LINES: usize = 30;

pub(super) fn detect(title: &str, screen: &[&str]) -> State {
    let lower: Vec<String> = screen.iter().map(|l| l.trim().to_lowercase()).collect();
    let last = lower.iter().rposition(|l| !l.is_empty());

    if let Some(end) = last {
        let hint = &lower[end];
        if hint.starts_with("press enter to confirm") || hint.starts_with("enter continue") {
            let dialog = &lower[end.saturating_sub(DIALOG_LINES)..end];
            let question = dialog.iter().any(|l| {
                ((l.starts_with("would you like to") || l.starts_with("do you want to"))
                    && l.ends_with('?'))
                    || l.starts_with("trust this folder?")
            });
            let answers = dialog
                .iter()
                .any(|l| l.trim_start_matches('›').trim_start().starts_with("1. "));
            if question && answers {
                return State::Blocked;
            }
        }
    }

    let spinner = title
        .chars()
        .next()
        .is_some_and(|c| ('\u{2800}'..='\u{28FF}').contains(&c))
        && title.chars().nth(1) == Some(' ');
    let status = lower
        .iter()
        .any(|l| l.starts_with("• ") && l.ends_with("• esc to interrupt)"));
    if spinner || status {
        State::Working
    } else {
        State::Idle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dialog_needs_the_hint_last() {
        let mut screen = vec![
            "  Would you like to run the following command?",
            "› 1. Yes, proceed (y)",
            "  Press enter to confirm or esc to cancel",
        ];
        assert_eq!(detect("cxwork", &screen), State::Blocked);
        // Typed into the prompt: the footer comes after it.
        screen.push("  gemma4 default · /tmp/x");
        assert_eq!(detect("cxwork", &screen), State::Idle);
    }

    #[test]
    fn working_from_the_title_or_the_status_line() {
        let screen = ["› Ask Codex to do anything"];
        assert_eq!(detect("⠏ cxwork", &screen), State::Working);
        assert_eq!(detect("cxwork", &screen), State::Idle);
        let screen = ["• Working (2s • esc to interrupt)", "› Ask Codex"];
        assert_eq!(detect("cxwork", &screen), State::Working);
        let screen = ["› esc to interrupt"];
        assert_eq!(detect("cxwork", &screen), State::Idle);
    }
}
