//! Which coding agent runs in a pane, and whether it is working, waiting for
//! the user (blocked) or idle, read from its title and the bottom of its
//! screen.
//!
//! Screen and title rather than the agent's hooks: hooks miss transitions
//! (Esc mid-turn and a cancelled permission prompt fire none). herdr learned
//! this first and reads Claude Code the same way. Each agent's module names
//! the version its signals were checked against, from real screens saved in
//! `tests/fixtures/<agent>/`.
//!
//! Every detector is conservative: blocked needs a question and its answers
//! in the agent's dialog area, never in what the user typed. A missed dot is
//! better than a false "waiting for you".

use std::path::Path;

mod claude;
mod opencode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    Claude,
    OpenCode,
}

impl Agent {
    pub fn name(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::OpenCode => "opencode",
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

/// The agent a process is, from its name and arguments.
///
/// - Claude Code's native binary is named after its version (`2.1.323`), so
///   `argv[0]` decides; the npm package runs as `node .../claude-code/cli.js`.
/// - OpenCode is the `opencode` binary, or its npm launcher
///   (`node .../opencode-ai/bin/opencode`). Its subcommands that aren't the
///   interactive screen (`opencode serve`, `opencode run`, …) don't count.
pub fn identify(name: &str, argv: &[String]) -> Option<Agent> {
    let named = |s: &str| match base(s) {
        "claude" => Some(Agent::Claude),
        "opencode" => Some(Agent::OpenCode),
        _ => None,
    };
    let runtime = argv.first().map(|a| base(a)).unwrap_or(name);
    let (agent, rest) = if let Some(a) = named(name).or_else(|| argv.first().and_then(|a| named(a)))
    {
        (a, argv.get(1..).unwrap_or_default())
    } else if matches!(runtime, "node" | "bun" | "deno") {
        let i = argv.iter().skip(1).position(|a| !a.starts_with('-'))? + 1;
        let script = &argv[i];
        let agent = named(script).or_else(|| {
            if script.contains("claude-code") {
                Some(Agent::Claude)
            } else if script.contains("/opencode-ai/") {
                Some(Agent::OpenCode)
            } else {
                None
            }
        })?;
        (agent, &argv[i + 1..])
    } else {
        return None;
    };
    if agent == Agent::OpenCode && !opencode::interactive(rest) {
        return None;
    }
    Some(agent)
}

/// The state of `agent`, given its title and the screen's lines (the active
/// screen, top to bottom).
pub fn detect(agent: Agent, title: &str, screen: &[&str]) -> State {
    match agent {
        Agent::Claude => claude::detect(title, screen),
        Agent::OpenCode => opencode::detect(title, screen),
    }
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
    fn identifying_opencode() {
        // As seen in a pane: OpenCode 2.2.7's binary, started from the shell.
        assert_eq!(
            identify("opencode", &args(&["opencode", "--standalone"])),
            Some(Agent::OpenCode)
        );
        assert_eq!(
            identify(
                "opencode",
                &args(&["/opt/homebrew/bin/opencode", "-c", "~/src/app"])
            ),
            Some(Agent::OpenCode)
        );
        assert_eq!(
            identify("opencode", &args(&["opencode", "mini"])),
            Some(Agent::OpenCode)
        );
        assert_eq!(
            identify(
                "node",
                &args(&["node", "/opt/lib/node_modules/opencode-ai/bin/opencode"])
            ),
            Some(Agent::OpenCode)
        );
        assert_eq!(
            identify("bun", &args(&["bun", "/x/bin/opencode", "--continue"])),
            Some(Agent::OpenCode)
        );
        // Not its interactive screen.
        for sub in ["serve", "run", "auth", "models", "session", "service"] {
            assert_eq!(
                identify("opencode", &args(&["opencode", sub])),
                None,
                "{sub}"
            );
        }
        assert_eq!(
            identify(
                "node",
                &args(&[
                    "node",
                    "/opt/lib/node_modules/opencode-ai/bin/opencode",
                    "serve"
                ])
            ),
            None
        );
        assert_eq!(
            identify("node", &args(&["node", "opencode-server.js"])),
            None
        );
        assert_eq!(identify("vim", &args(&["vim", "opencode.json"])), None);
    }
}
