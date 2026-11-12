//! Agent status per pane: what the session detects on screen, what an agent
//! reports itself, and "done", which only the daemon knows (an agent
//! finished while its pane wasn't seen).

use std::collections::BTreeMap;

use proto::layout::PaneId;
use proto::status::{AgentState, PaneStatus};

#[derive(Debug, Default)]
struct Pane {
    /// From the screen: agent and state (never done).
    detected: Option<(String, AgentState)>,
    /// From the agent itself; beats `detected`.
    report: Option<(String, AgentState)>,
    seq: Option<u64>,
    done: bool,
}

impl Pane {
    fn current(&self) -> Option<(&str, AgentState)> {
        self.report
            .as_ref()
            .or(self.detected.as_ref())
            .map(|(a, s)| (a.as_str(), *s))
    }
}

#[derive(Debug, Default)]
pub struct Statuses {
    panes: BTreeMap<PaneId, Pane>,
    /// The map watchers last got.
    sent: BTreeMap<PaneId, PaneStatus>,
}

impl Statuses {
    /// Change `pane`, then work out done: working → idle unseen is done;
    /// anything but idle clears it.
    fn update(&mut self, pane: PaneId, seen: bool, f: impl FnOnce(&mut Pane)) {
        let p = self.panes.entry(pane).or_default();
        let before = p.current().map(|(_, s)| s);
        f(p);
        match p.current().map(|(_, s)| s) {
            Some(AgentState::Idle) => {
                if before == Some(AgentState::Working) && !seen {
                    p.done = true;
                }
            }
            _ => p.done = false,
        }
        if p.current().is_none() && p.seq.is_none() {
            self.panes.remove(&pane);
        }
    }

    /// What the session saw: the agent and its state, or none; `at_shell`
    /// when the pane's shell is back in the foreground, which ends a report.
    pub fn detected(
        &mut self,
        pane: PaneId,
        agent: Option<(String, AgentState)>,
        at_shell: bool,
        seen: bool,
    ) {
        self.update(pane, seen, |p| {
            p.detected = agent;
            if at_shell {
                p.report = None;
                p.seq = None;
            }
        });
    }

    /// An agent's own report. False (and ignored) when `seq` isn't above
    /// the last one.
    pub fn report(
        &mut self,
        pane: PaneId,
        agent: String,
        state: AgentState,
        seq: Option<u64>,
        seen: bool,
    ) -> bool {
        let last = self.panes.get(&pane).and_then(|p| p.seq);
        if let (Some(last), Some(seq)) = (last, seq)
            && seq <= last
        {
            return false;
        }
        self.update(pane, seen, |p| {
            p.report = Some((agent, state));
            if seq.is_some() {
                p.seq = seq;
            }
        });
        true
    }

    pub fn release(&mut self, pane: PaneId, seen: bool) {
        self.update(pane, seen, |p| {
            p.report = None;
            p.seq = None;
        });
    }

    /// The user looks at `pane`: done becomes idle.
    pub fn seen(&mut self, pane: PaneId) {
        if let Some(p) = self.panes.get_mut(&pane) {
            p.done = false;
        }
    }

    pub fn remove(&mut self, pane: PaneId) {
        self.panes.remove(&pane);
    }

    pub fn get(&self, pane: PaneId) -> Option<PaneStatus> {
        let p = self.panes.get(&pane)?;
        let (agent, state) = p.current()?;
        Some(PaneStatus {
            agent: agent.to_owned(),
            state: if p.done { AgentState::Done } else { state },
        })
    }

    pub fn map(&self) -> BTreeMap<PaneId, PaneStatus> {
        self.panes
            .keys()
            .filter_map(|&id| Some((id, self.get(id)?)))
            .collect()
    }

    /// The map, if it changed since the last call.
    pub fn changed(&mut self) -> Option<BTreeMap<PaneId, PaneStatus>> {
        let now = self.map();
        if now == self.sent {
            return None;
        }
        self.sent = now.clone();
        Some(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use AgentState::*;

    fn claude(s: AgentState) -> Option<(String, AgentState)> {
        Some(("claude".into(), s))
    }

    fn state(st: &Statuses, p: PaneId) -> Option<AgentState> {
        st.get(p).map(|s| s.state)
    }

    #[test]
    fn done_only_when_unseen() {
        let mut st = Statuses::default();
        st.detected(1, claude(Working), false, false);
        st.detected(1, claude(Idle), false, false);
        assert_eq!(state(&st, 1), Some(Done));
        st.seen(1);
        assert_eq!(state(&st, 1), Some(Idle));

        st.detected(2, claude(Working), false, true);
        st.detected(2, claude(Idle), false, true);
        assert_eq!(state(&st, 2), Some(Idle));
    }

    #[test]
    fn done_ends_when_work_starts_again() {
        let mut st = Statuses::default();
        st.detected(1, claude(Working), false, false);
        st.detected(1, claude(Idle), false, false);
        st.detected(1, claude(Working), false, false);
        assert_eq!(state(&st, 1), Some(Working));
        st.detected(1, claude(Blocked), false, false);
        st.detected(1, claude(Idle), false, false);
        assert_eq!(state(&st, 1), Some(Idle), "a cancelled prompt isn't done");
    }

    #[test]
    fn reports_beat_detection_until_released() {
        let mut st = Statuses::default();
        st.detected(1, claude(Idle), false, false);
        assert!(st.report(1, "bot".into(), Working, Some(1), false));
        assert_eq!(st.get(1).unwrap().agent, "bot");
        assert!(
            !st.report(1, "bot".into(), Blocked, Some(1), false),
            "stale"
        );
        assert_eq!(state(&st, 1), Some(Working));
        st.detected(1, claude(Blocked), false, false);
        assert_eq!(state(&st, 1), Some(Working));
        st.release(1, false);
        assert_eq!(st.get(1).unwrap().agent, "claude");
        assert_eq!(state(&st, 1), Some(Blocked));
        assert!(
            st.report(1, "bot".into(), Idle, Some(1), false),
            "seq resets"
        );
    }

    #[test]
    fn the_shell_coming_back_ends_a_report() {
        let mut st = Statuses::default();
        st.report(1, "bot".into(), Working, Some(5), false);
        st.detected(1, None, false, false);
        assert_eq!(state(&st, 1), Some(Working));
        st.detected(1, None, true, false);
        assert_eq!(st.get(1), None);
        assert!(st.map().is_empty());
    }

    #[test]
    fn changes_are_sent_once() {
        let mut st = Statuses::default();
        assert_eq!(st.changed(), None);
        st.detected(1, claude(Working), false, false);
        assert_eq!(st.changed().unwrap().len(), 1);
        st.detected(1, claude(Working), false, false);
        assert_eq!(st.changed(), None);
        st.remove(1);
        assert_eq!(st.changed(), Some(BTreeMap::new()));
    }
}
