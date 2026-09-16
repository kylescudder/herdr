//! Done markers as an inbox: acknowledgement and orphan repair.
//!
//! Fork-carried behaviour kept in its own module so an upstream merge cannot
//! conflict with it. The hooks left in upstream files are the absence of
//! `mark_active_tab_seen` on focus paths, the unconditional `seen = false` on
//! completion, and the two calls to `promote_orphaned_workspace_children` after
//! a workspace is removed. See FORK.md.

use super::actions::PaneStateUpdate;
use super::state::AppState;
use crate::layout::PaneId;

impl AppState {
    /// Marks every pane in `ws_idx` as seen ("acknowledged"), turning any
    /// finished "done" agent back to "idle" without changing its agent state or
    /// re-running it. Returns a `PaneStateUpdate` for each pane whose seen flag
    /// actually flipped so the caller can emit `pane.agent_status_changed`.
    pub(crate) fn acknowledge_workspace(&mut self, ws_idx: usize) -> Vec<PaneStateUpdate> {
        let now = std::time::Instant::now();
        let mut updates = Vec::new();
        let Some(ws) = self.workspaces.get(ws_idx) else {
            return updates;
        };
        let targets: Vec<(PaneId, crate::terminal::TerminalId)> = ws
            .tabs
            .iter()
            .flat_map(|tab| tab.panes.iter())
            .filter(|(_, pane)| !pane.seen)
            .map(|(pane_id, pane)| (*pane_id, pane.attached_terminal_id.clone()))
            .collect();
        for (pane_id, terminal_id) in targets {
            let Some(terminal) = self.terminals.get(&terminal_id) else {
                continue;
            };
            let change = terminal.unchanged_effective_state_change_at(now);
            // Drop any queued completion notification for this pane. With a
            // non-zero `toast.delay_seconds` a background completion sits in
            // `pending_agent_notifications` until its deadline, and the drain
            // checks terminal state and agent label but not `seen` — so without
            // this an acknowledged item still fires its toast and sound later.
            self.pending_agent_notifications.remove(&pane_id);
            if let Some(pane) = self.workspaces[ws_idx]
                .tabs
                .iter_mut()
                .find_map(|tab| tab.panes.get_mut(&pane_id))
            {
                pane.seen = true;
            }
            updates.push(PaneStateUpdate {
                pane_id,
                ws_idx,
                previous_agent_label: change.previous_agent_label.clone(),
                previous_known_agent: change.previous_known_agent,
                previous_state: change.previous_state,
                previous_seen: false,
                previous_presentation: change.previous_presentation.clone(),
                agent_label: change.agent_label.clone(),
                known_agent: change.known_agent,
                state: change.state,
                seen: true,
                presentation: change.presentation.clone(),
                agent_name_changed: false,
                agent_released: false,
                agent_release_status: None,
                // Acknowledging only flips `seen`; no completion occurred, so
                // this must not fire a done notification.
                suppress_completion: true,
            });
        }
        updates
    }

    /// Clears explicit grouping links that point at workspaces which no longer
    /// exist, promoting those children back to the top level.
    ///
    /// Closing a parent must not leave a dangling `parent_workspace_id`: it is
    /// persisted, and workspace ids are only reserved against surviving
    /// workspaces, so after a restart a newly created workspace can reuse the
    /// closed id and silently adopt the orphaned child.
    pub(crate) fn promote_orphaned_workspace_children(&mut self) {
        let live: std::collections::HashSet<&str> =
            self.workspaces.iter().map(|ws| ws.id.as_str()).collect();
        let orphaned: Vec<usize> = self
            .workspaces
            .iter()
            .enumerate()
            .filter_map(|(index, ws)| {
                ws.parent_workspace_id
                    .as_deref()
                    .is_some_and(|parent| {
                        parent != crate::protocol::EXPLICIT_TOP_LEVEL_PARENT
                            && !live.contains(parent)
                    })
                    .then_some(index)
            })
            .collect();
        for index in orphaned {
            self.workspaces[index].parent_workspace_id = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::actions::tests::app_with_workspaces;
    use crate::detect::{Agent, AgentState};
    use crate::events::AppEvent;

    /// Acknowledging must also cancel a queued completion notification. With a
    /// non-zero `toast.delay_seconds` the toast sits in
    /// `pending_agent_notifications` until its deadline, and the drain does not
    /// check `seen`, so it would fire after the user dismissed the item.
    #[test]
    fn acknowledge_cancels_a_pending_completion_notification() {
        let mut state = app_with_workspaces(&["active", "background"]);
        state.active = Some(0);
        state.toast_config.delay_seconds = 30;
        let pane_id = *state.workspaces[1].panes.keys().next().unwrap();
        let terminal_id = state.workspaces[1]
            .panes
            .get(&pane_id)
            .unwrap()
            .attached_terminal_id
            .clone();
        state.terminals.get_mut(&terminal_id).unwrap().state = AgentState::Working;
        state.workspaces[1].panes.get_mut(&pane_id).unwrap().seen = true;

        // Background completion queues a delayed notification.
        state.handle_app_event(AppEvent::StateChanged {
            pane_id,
            agent: Some(Agent::Pi),
            state: AgentState::Idle,
            visible_blocker: false,
            visible_working: false,
            process_exited: false,
            observed_at: std::time::Instant::now(),
        });
        assert!(
            state.pending_agent_notifications.contains_key(&pane_id),
            "a delayed completion notification should be queued"
        );

        state.acknowledge_workspace(1);

        assert!(
            !state.pending_agent_notifications.contains_key(&pane_id),
            "acknowledging must cancel the queued notification"
        );
        // Nothing is delivered once the deadline passes.
        let delivered = state.drain_due_agent_notifications(
            std::time::Instant::now() + std::time::Duration::from_secs(120),
        );
        assert!(
            delivered.is_empty(),
            "an acknowledged completion must not notify later: {delivered:?}"
        );
    }

    /// Restored with the sticky done markers (07d0a24e): focus must not clear a
    /// completion; only the agent being addressed again does.
    #[test]
    fn done_marker_survives_focus_until_agent_is_addressed() {
        let mut state = app_with_workspaces(&["active", "background"]);
        state.active = Some(0);
        let pane_id = *state.workspaces[1].panes.keys().next().unwrap();
        let terminal_id = state.workspaces[1]
            .panes
            .get(&pane_id)
            .unwrap()
            .attached_terminal_id
            .clone();
        state.terminals.get_mut(&terminal_id).unwrap().state = AgentState::Working;
        state.workspaces[1].panes.get_mut(&pane_id).unwrap().seen = true;

        // Agent finishes in the background -> done marker (unseen).
        state.handle_app_event(AppEvent::StateChanged {
            pane_id,
            agent: Some(Agent::Pi),
            state: AgentState::Idle,
            visible_blocker: false,
            visible_working: false,
            process_exited: false,
            observed_at: std::time::Instant::now(),
        });
        assert!(!state.workspaces[1].panes.get(&pane_id).unwrap().seen);

        // Focusing/switching to the pane must not clear the marker.
        state.switch_workspace(1);
        assert!(!state.workspaces[1].panes.get(&pane_id).unwrap().seen);

        // Addressing the agent (it starts working again) clears it.
        state.handle_app_event(AppEvent::StateChanged {
            pane_id,
            agent: Some(Agent::Pi),
            state: AgentState::Working,
            visible_blocker: false,
            visible_working: true,
            process_exited: false,
            observed_at: std::time::Instant::now(),
        });
        assert!(state.workspaces[1].panes.get(&pane_id).unwrap().seen);
    }
}
