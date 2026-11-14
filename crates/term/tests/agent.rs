//! Agent state on real screens, which the emulator built from the agents
//! running in a pane, account details removed. Each file is `title: ...` and
//! then the screen.
//! - `fixtures/claude/`: Claude Code 2.1.323.
//! - `fixtures/opencode/`: OpenCode 2.2.7, 80x24.

use term::agent::{Agent, State, detect};

fn state_of(agent: Agent, name: &str) -> State {
    let path = format!(
        "{}/tests/fixtures/{}/{name}.txt",
        env!("CARGO_MANIFEST_DIR"),
        agent.name()
    );
    let text = std::fs::read_to_string(&path).unwrap();
    let (first, screen) = text.split_once('\n').unwrap();
    let title = first.strip_prefix("title: ").unwrap();
    let lines: Vec<&str> = screen.lines().collect();
    detect(agent, title, &lines)
}

/// Every fixture of `agent` is listed, with the state its name says.
fn check(agent: Agent, want: &[(&str, State)]) {
    for (name, state) in want {
        assert_eq!(state_of(agent, name), *state, "{} {name}", agent.name());
    }
    let dir = format!(
        "{}/tests/fixtures/{}",
        env!("CARGO_MANIFEST_DIR"),
        agent.name()
    );
    let mut files: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().replace(".txt", ""))
        .collect();
    files.sort();
    let mut listed: Vec<String> = want.iter().map(|(n, _)| n.to_string()).collect();
    listed.sort();
    assert_eq!(files, listed, "every fixture is checked");
}

#[test]
fn claude_code_screens() {
    check(
        Agent::Claude,
        &[
            ("idle-start", State::Idle),
            ("idle-after-turn", State::Idle),
            ("working", State::Working),
            ("working-again", State::Working),
            ("blocked-bash", State::Blocked),
            ("blocked-write", State::Blocked),
            ("idle-after-cancel", State::Idle),
            ("idle-after-interrupt", State::Idle),
            // A menu with "Esc to cancel" that asks nothing.
            ("model-menu", State::Idle),
            ("idle-after-menu", State::Idle),
        ],
    );
}

#[test]
fn opencode_screens() {
    check(
        Agent::OpenCode,
        &[
            ("idle-start", State::Idle),
            ("working", State::Working),
            ("working-tool", State::Working),
            // One Esc: "esc again to interrupt", still running.
            ("working-esc-once", State::Working),
            ("blocked-command", State::Blocked),
            ("blocked-edit", State::Blocked),
            ("blocked-question", State::Blocked),
            ("idle-after-turn", State::Idle),
            ("idle-after-interrupt", State::Idle),
            ("idle-after-cancel", State::Idle),
            ("idle-after-dismiss", State::Idle),
            // Pickers that ask nothing.
            ("menu", State::Idle),
            ("model-menu", State::Idle),
            // The user typed the dialog's words into the prompt.
            ("typed-question", State::Idle),
        ],
    );
}
