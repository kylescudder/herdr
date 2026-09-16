//! Keybind actions the fork adds to the client shell.
//!
//! Fork-owned: upstream has no equivalent, so this file cannot conflict on an
//! upstream merge. The hook it leaves behind is the pair of dispatch arms in
//! `src/client/shell/actions.rs`, guarded by the keybind tests in
//! `src/fork_contract.rs`.

use super::state::ClientShellState;
use super::ClientShellInput;

impl ClientShellState {
    /// The workspace a workspace-scoped keybind acts on: the sidebar picker's
    /// selection when it is open, else the focused workspace.
    fn keybind_target_workspace(&self) -> Option<String> {
        self.navigate_workspace_id.clone().or_else(|| {
            self.snapshot
                .as_deref()
                .and_then(|snapshot| snapshot.focused_workspace_id.clone())
        })
    }

    /// Marks a finished workspace as read: its "done" agents go back to idle
    /// without re-running them.
    pub(super) fn acknowledge_target_workspace(&mut self, outcome: &mut ClientShellInput) {
        let Some(workspace_id) = self.keybind_target_workspace() else {
            return;
        };
        self.push_endpoint_method(
            crate::api::schema::Method::WorkspaceAcknowledge(crate::api::schema::WorkspaceTarget {
                workspace_id,
            }),
            outcome,
        );
        outcome.repaint = true;
    }

    /// Opens the modal "move to workspace" picker, matching the original
    /// feature. Leaving Navigate mode here is deliberate — the modal owns input
    /// from this point, and staying in Navigate would eat the picker's keys.
    pub(super) fn open_move_picker_for_target(&mut self, outcome: &mut ClientShellInput) {
        let Some(source) = self.keybind_target_workspace() else {
            return;
        };
        if self.open_move_workspace_overlay(source) {
            self.mode = self.copy_or_terminal_mode();
            self.navigate_workspace_id = None;
            outcome.repaint = true;
        }
    }

    /// Builds the move for reordering the project owning `source_workspace_id`
    /// one slot among the top-level (non-linked) roots. `up` moves it toward the
    /// front. A worktree child moves its whole group via its primary. Reuses the
    /// sidebar drag's move builder so grouping and block moves stay consistent.
    /// No-op (`None`) at the list ends.
    pub(super) fn workspace_reorder_method(
        &self,
        source_workspace_id: &str,
        up: bool,
    ) -> Option<crate::api::schema::Method> {
        let snapshot = self.snapshot.as_deref()?;
        let grouping = super::workspace_grouping::WorkspaceGrouping::compute(snapshot);
        let index = snapshot
            .workspaces
            .iter()
            .position(|workspace| workspace.workspace_id == source_workspace_id)?;

        // A nested workspace reorders among its siblings, inside the group.
        // Moving its parent's block instead would leave the selected row
        // exactly where it was, which reads as the keybind doing nothing.
        if let Some(root) = grouping.parent_of(index) {
            let siblings = grouping.children(root);
            let position = siblings.iter().position(|sibling| *sibling == index)?;
            // `move_workspace` inserts before the element currently at
            // `insert_index`, so swapping with the neighbour above is that
            // neighbour's index, and with the one below is just past it.
            let insert_index = if up {
                *siblings.get(position.checked_sub(1)?)?
            } else {
                siblings.get(position + 1)?.saturating_add(1)
            };
            return Some(crate::api::schema::Method::WorkspaceMove(
                crate::api::schema::WorkspaceMoveParams {
                    workspace_id: source_workspace_id.to_owned(),
                    insert_index,
                },
            ));
        }

        // Reorder the top-level row this workspace belongs to, using the same
        // grouping the sidebar renders.
        let (source_root, _) = super::render::workspace_group_block(snapshot, source_workspace_id)?;
        let roots = super::render::top_level_workspace_ids(snapshot);
        let position = roots.iter().position(|id| *id == source_root)?;
        let before = if up {
            // Land before the previous root. No-op at the top.
            Some(roots.get(position.checked_sub(1)?)?.clone())
        } else {
            // Jump the next root; land before the one after it, or append when
            // the next root is last. No-op at the bottom.
            roots.get(position + 1)?;
            roots.get(position + 2).cloned()
        };
        self.workspace_move_method(&source_root, before.as_deref())
    }
}
