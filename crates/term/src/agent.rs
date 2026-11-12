//! Which coding agent runs in a pane, and whether it is working, waiting for
//! the user (blocked) or idle, read from its title and the bottom of its
//! screen.
//!
//! Screen and title rather than the agent's hooks: hooks miss transitions
//! (Esc mid-turn and a cancelled permission prompt fire none). herdr learned
//! this first and reads Claude Code the same way; the signals below are
//! checked against Claude Code 2.1.323 (`tests/fixtures/claude/`).

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    Claude,
}

impl Agent {
    pub fn name(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Working,
    /// Waiting for the user to answer a prompt.
    Blocked,
}

fn base(s: &str) -> &str {
    Path::new(s)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(s)
}

/// The agent a process is, from its name and arguments. Claude Code's native
/// binary is named after its version (`2.1.323`), so `argv[0]` decides; the
/// npm package runs as `node .../claude-code/cli.js`.
pub fn identify(name: &str, argv: &[String]) -> Option<Agent> {
    let is_claude = |s: &str| base(s) == "claude";
    if is_claude(name) || argv.first().is_some_and(|a| is_claude(a)) {
        return Some(Agent::Claude);
    }
    let runtime = argv.first().map(|a| base(a)).unwrap_or(name);
    if matches!(runtime, "node" | "bun" | "deno")
        && argv
            .iter()
            .skip(1)
            .find(|a| !a.starts_with('-'))
            .is_some_and(|script| is_claude(script) || script.contains("claude-code"))
    {
        return Some(Agent::Claude);
    }
    None
}

/// The state of `agent`, given its title and the screen's lines (the active
/// screen, top to bottom).
pub fn detect(agent: Agent, title: &str, screen: &[&str]) -> State {
    match agent {
        Agent::Claude => claude(title, screen),
    }
}

/// A full-width rule Claude draws around its prompt box and above dialogs.
fn is_rule(line: &str) -> bool {
    let t = line.trim();
    t.chars().count() >= 20 && t.chars().all(|c| c == '─')
}

fn claude(title: &str, screen: &[&str]) -> State {
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

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn identifying_claude() {
        assert_eq!(identify("2.1.323", &args(&["claude"])), Some(Agent::Claude));
        assert_eq!(
            identify(
                "2.1.323",
                &args(&["/Users/x/.local/bin/claude", "--resume"])
            ),
            Some(Agent::Claude)
        );
        assert_eq!(identify("claude", &[]), Some(Agent::Claude));
        assert_eq!(
            identify(
                "node",
                &args(&[
                    "node",
                    "--no-warnings",
                    "/opt/lib/node_modules/@anthropic-ai/claude-code/cli.js"
                ])
            ),
            Some(Agent::Claude)
        );
        assert_eq!(identify("zsh", &args(&["-zsh"])), None);
        assert_eq!(identify("node", &args(&["node", "server.js"])), None);
        assert_eq!(identify("vim", &args(&["vim", "claude.md"])), None);
    }

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
        assert_eq!(detect(Agent::Claude, "✳ Claude Code", &screen), State::Idle);
    }

    #[test]
    fn a_spinner_only_counts_in_the_title() {
        let screen = ["  ⏸ manual mode on · ? for shortcuts   ◐ medium · /effort"];
        assert_eq!(detect(Agent::Claude, "✳ Claude Code", &screen), State::Idle);
        assert_eq!(detect(Agent::Claude, "⠋ Working", &screen), State::Working);
        assert_eq!(detect(Agent::Claude, "◐x", &screen), State::Idle);
    }
}
