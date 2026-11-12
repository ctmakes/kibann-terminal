//! Agent state on real Claude Code screens (`fixtures/claude/`: screens the
//! emulator built from recordings of Claude Code 2.1.323, account details
//! removed). Each file is `title: ...` and then the screen.

use term::agent::{Agent, State, detect};

fn state_of(name: &str) -> State {
    let path = format!(
        "{}/tests/fixtures/claude/{name}.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap();
    let (first, screen) = text.split_once('\n').unwrap();
    let title = first.strip_prefix("title: ").unwrap();
    let lines: Vec<&str> = screen.lines().collect();
    detect(Agent::Claude, title, &lines)
}

#[test]
fn claude_code_screens() {
    for (name, want) in [
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
    ] {
        assert_eq!(state_of(name), want, "{name}");
    }
}
