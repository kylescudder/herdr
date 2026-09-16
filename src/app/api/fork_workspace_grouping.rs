//! Server-side workspace grouping and done-marker acknowledgement.
//!
//! Fork-carried behaviour kept in its own module so an upstream merge cannot
//! conflict with it. The only hooks left in upstream files are the `Method`
//! variants, the dispatch arms, `request_changes_ui` and `CLIENT_SHELL_METHODS`
//! — each covered by a guard in `src/fork_contract.rs`. See FORK.md.

use crate::api::schema::{
    EventData, EventEnvelope, EventKind, ResponseResult, WorkspaceReparentParams, WorkspaceTarget,
};
use crate::app::App;

use super::responses::{encode_error, encode_success};
use super::workspaces::workspace_not_found;

impl App {
    /// The workspace `index` groups under, mirroring the sidebar: an explicit
    /// `parent_workspace_id` wins, otherwise a linked git worktree is inferred
    /// to sit under its repo's primary checkout.
    fn grouping_parent_index(&self, index: usize) -> Option<usize> {
        let workspace = self.state.workspaces.get(index)?;
        if let Some(parent_id) = workspace.parent_workspace_id.as_deref() {
            if parent_id == crate::protocol::EXPLICIT_TOP_LEVEL_PARENT {
                // Explicitly top level: suppress the inferred worktree parent.
                return None;
            }
            return self
                .state
                .workspaces
                .iter()
                .position(|candidate| candidate.id == parent_id);
        }
        let space = workspace.worktree_space.as_ref()?;
        if !space.is_linked_worktree {
            return None;
        }
        self.state.workspaces.iter().position(|candidate| {
            candidate.worktree_space.as_ref().is_some_and(|candidate| {
                !candidate.is_linked_worktree && candidate.key == space.key
            })
        })
    }

    pub(super) fn handle_workspace_reparent(
        &mut self,
        id: String,
        params: WorkspaceReparentParams,
    ) -> String {
        let Some(index) = self.parse_workspace_id(&params.workspace_id) else {
            return workspace_not_found(id, &params.workspace_id);
        };
        let parent_id = match &params.parent_workspace_id {
            // Record the choice rather than clearing it: a linked worktree would
            // otherwise be re-nested under its primary checkout immediately.
            None => Some(crate::protocol::EXPLICIT_TOP_LEVEL_PARENT.to_string()),
            Some(requested) => {
                let Some(parent_index) = self.parse_workspace_id(requested) else {
                    return workspace_not_found(id, requested);
                };
                if parent_index == index {
                    return encode_error(
                        id,
                        "workspace_reparent_failed",
                        "a workspace cannot be filed under itself".to_string(),
                    );
                }
                // Reject cycles against the same combined graph the sidebar
                // groups by: an explicit parent, else the primary checkout a
                // linked worktree is inferred to sit under. Walking only
                // explicit links would accept filing a repo under its own
                // worktree, and the resulting two-node cycle drops both rows
                // out of the sidebar entirely.
                let child_id = self.state.workspaces[index].id.clone();
                let mut cursor = Some(parent_index);
                for _ in 0..self.state.workspaces.len() {
                    let Some(current) = cursor else { break };
                    if self.state.workspaces[current].id == child_id {
                        return encode_error(
                            id,
                            "workspace_reparent_failed",
                            "reparenting would create a grouping cycle".to_string(),
                        );
                    }
                    cursor = self.grouping_parent_index(current);
                }
                Some(self.state.workspaces[parent_index].id.clone())
            }
        };
        self.state.workspaces[index].parent_workspace_id = parent_id;
        self.schedule_session_save();
        // Grouping is a shared session fact, not TUI presentation state, so it
        // is announced on the public event path as well as returned here.
        self.emit_event(EventEnvelope {
            event: EventKind::WorkspaceUpdated,
            data: EventData::WorkspaceUpdated {
                workspace: self.workspace_info(index),
            },
        });
        let workspaces = self.workspace_list_info();
        // Grouping is sidebar-visible state. `api::request_changes_ui` must list
        // this method, or the server never runs a render pass and the diff-based
        // client-shell projection is never pushed: the reparent would persist
        // silently while the sidebar kept the old grouping until a restart.
        encode_success(id, ResponseResult::WorkspaceList { workspaces })
    }

    pub(super) fn handle_workspace_acknowledge(
        &mut self,
        id: String,
        target: WorkspaceTarget,
    ) -> String {
        let Some(index) = self.parse_workspace_id(&target.workspace_id) else {
            return workspace_not_found(id, &target.workspace_id);
        };
        if self.state.workspaces.get(index).is_none() {
            return workspace_not_found(id, &target.workspace_id);
        }
        self.acknowledge_workspace_seen(index);
        encode_success(id, ResponseResult::Ok {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::schema::SuccessResponse;
    use crate::config::Config;
    use crate::workspace::Workspace;

    #[test]
    fn api_workspace_reparent_files_unfiles_and_rejects_cycles() {
        let event_hub = crate::api::EventHub::default();
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            api_rx,
            event_hub,
        );
        app.state.workspaces = vec![Workspace::test_new("child"), Workspace::test_new("parent")];
        let child = app.public_workspace_id(0);
        let parent = app.public_workspace_id(1);

        // File child under parent.
        let response = app.handle_workspace_reparent(
            "r1".into(),
            WorkspaceReparentParams {
                workspace_id: child.clone(),
                parent_workspace_id: Some(parent.clone()),
            },
        );
        let success: SuccessResponse = serde_json::from_str(&response).unwrap();
        assert!(matches!(
            success.result,
            ResponseResult::WorkspaceList { .. }
        ));
        assert_eq!(
            app.state.workspaces[0].parent_workspace_id.as_deref(),
            Some(parent.as_str())
        );

        // Filing parent under child would form a cycle: rejected, state intact.
        let response = app.handle_workspace_reparent(
            "r2".into(),
            WorkspaceReparentParams {
                workspace_id: parent.clone(),
                parent_workspace_id: Some(child.clone()),
            },
        );
        assert!(
            response.contains("workspace_reparent_failed"),
            "cycle should be rejected: {response}"
        );
        assert!(app.state.workspaces[1].parent_workspace_id.is_none());

        // Unfile child. The choice is recorded as an explicit "top level"
        // rather than cleared, so a linked worktree is not re-nested by the
        // git fallback.
        app.handle_workspace_reparent(
            "r3".into(),
            WorkspaceReparentParams {
                workspace_id: child.clone(),
                parent_workspace_id: None,
            },
        );
        assert_eq!(
            app.state.workspaces[0].parent_workspace_id.as_deref(),
            Some(crate::protocol::EXPLICIT_TOP_LEVEL_PARENT)
        );
        assert!(app.grouping_parent_index(0).is_none());
    }

    /// Unfiling a linked worktree must stick. Clearing the field would let the
    /// git fallback immediately re-infer the primary checkout as its parent,
    /// making the advertised operation a silent no-op.
    #[test]
    fn api_workspace_reparent_keeps_a_linked_worktree_at_top_level() {
        let event_hub = crate::api::EventHub::default();
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            api_rx,
            event_hub,
        );
        let space = |linked: bool| crate::workspace::WorktreeSpaceMembership {
            key: "repo".into(),
            label: "repo".into(),
            repo_root: std::path::PathBuf::from("/repo"),
            checkout_path: std::path::PathBuf::from("/repo"),
            is_linked_worktree: linked,
        };
        let mut primary = Workspace::test_new("repo");
        primary.worktree_space = Some(space(false));
        let mut linked = Workspace::test_new("repo-feature");
        linked.worktree_space = Some(space(true));
        app.state.workspaces = vec![primary, linked];
        let linked_id = app.public_workspace_id(1);

        // Inferred under its primary checkout before any explicit choice.
        assert_eq!(app.grouping_parent_index(1), Some(0));

        app.handle_workspace_reparent(
            "r1".into(),
            WorkspaceReparentParams {
                workspace_id: linked_id,
                parent_workspace_id: None,
            },
        );
        assert!(
            app.grouping_parent_index(1).is_none(),
            "an explicitly unfiled worktree must stay at top level"
        );
    }

    /// A repo cannot be filed under its own linked worktree. The worktree has
    /// no explicit parent, so walking only explicit links accepts it; the
    /// inferred worktree edge then closes a cycle and both rows drop out of the
    /// sidebar.
    #[test]
    fn api_workspace_reparent_rejects_a_cycle_through_an_inferred_worktree_parent() {
        let event_hub = crate::api::EventHub::default();
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            api_rx,
            event_hub,
        );
        let space = |linked: bool| crate::workspace::WorktreeSpaceMembership {
            key: "repo".into(),
            label: "repo".into(),
            repo_root: std::path::PathBuf::from("/repo"),
            checkout_path: std::path::PathBuf::from("/repo"),
            is_linked_worktree: linked,
        };
        let mut primary = Workspace::test_new("repo");
        primary.worktree_space = Some(space(false));
        let mut linked = Workspace::test_new("repo-feature");
        linked.worktree_space = Some(space(true));
        app.state.workspaces = vec![primary, linked];
        let primary_id = app.public_workspace_id(0);
        let linked_id = app.public_workspace_id(1);

        let response = app.handle_workspace_reparent(
            "r1".into(),
            WorkspaceReparentParams {
                workspace_id: primary_id,
                parent_workspace_id: Some(linked_id),
            },
        );
        assert!(
            response.contains("workspace_reparent_failed"),
            "filing a repo under its own worktree must be rejected: {response}"
        );
        assert!(app.state.workspaces[0].parent_workspace_id.is_none());
    }

    /// Closing a parent must not leave a child pointing at a dead id: ids are
    /// reserved only against surviving workspaces, so a later workspace could
    /// reuse it and silently adopt the orphan.
    #[test]
    fn closing_a_parent_promotes_its_explicit_children_to_top_level() {
        let mut state = crate::app::AppState::test_new();
        state.workspaces = vec![
            Workspace::test_new("child"),
            Workspace::test_new("parent"),
            Workspace::test_new("other"),
        ];
        let parent_id = state.workspaces[1].id.clone();
        state.workspaces[0].parent_workspace_id = Some(parent_id.clone());

        state.workspaces.remove(1);
        state.promote_orphaned_workspace_children();

        assert!(
            state.workspaces[0].parent_workspace_id.is_none(),
            "the child must be promoted once its parent is gone"
        );
        assert!(
            !state
                .workspaces
                .iter()
                .any(|ws| ws.parent_workspace_id.as_deref() == Some(parent_id.as_str())),
            "no workspace may still reference the closed parent"
        );
    }
}
