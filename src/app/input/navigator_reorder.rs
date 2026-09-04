//! Keyboard reordering of workspaces (projects) in the workspace list.
//!
//! Pressing `shift+k` / `shift+j` moves the selected project up or down one
//! slot, so frequently used projects can be lifted above ones that happened to
//! be opened later. It works from both surfaces that show the workspace list:
//! the sidebar picker (Navigate mode, `prefix+w`) and the session navigator
//! overlay (`prefix+g`).
//!
//! Ordering and worktree grouping reuse the exact rules the mouse-drag reorder
//! uses ([`AppState::workspace_move_block_params`]): a worktree group moves as
//! one atomic block, an adjacent group is jumped over as a unit, and the move is
//! persisted through the same server API path so other clients stay in sync.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    app::{
        state::{AppState, NavigatorTarget, WorkspaceDropTarget},
        App,
    },
    terminal::TerminalRuntimeRegistry,
};

/// Direction of a navigator workspace reorder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkspaceReorderDirection {
    Up,
    Down,
}

/// A resolved reorder, ready to dispatch through the workspace-move API. Mirrors
/// the mouse path: a plain workspace moves singly, a worktree group moves as a
/// block, so each surface emits the same server event.
#[derive(Debug, Clone)]
pub(crate) enum WorkspaceReorderCommand {
    Single {
        source_ws_idx: usize,
        insert_idx: usize,
    },
    Block(crate::api::schema::WorkspaceMoveBlockParams),
}

/// Maps a key press to a reorder direction, or `None` when it is not a reorder
/// shortcut. `shift+k` moves the selected project up, `shift+j` moves it down.
pub(crate) fn reorder_direction_for_key(key: &KeyEvent) -> Option<WorkspaceReorderDirection> {
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    match key.code {
        // Terminals usually deliver a shifted letter as its uppercase char; some
        // send the lowercase char with the shift modifier set. Accept both.
        KeyCode::Char('K') => Some(WorkspaceReorderDirection::Up),
        KeyCode::Char('J') => Some(WorkspaceReorderDirection::Down),
        KeyCode::Char('k') if shift => Some(WorkspaceReorderDirection::Up),
        KeyCode::Char('j') if shift => Some(WorkspaceReorderDirection::Down),
        _ => None,
    }
}

/// Reorder direction for the session-navigator overlay: like
/// [`reorder_direction_for_key`] but never fires while the search box is
/// focused, so the shifted letters still type into the query.
pub(crate) fn navigator_reorder_direction_for_key(
    state: &AppState,
    key: &KeyEvent,
) -> Option<WorkspaceReorderDirection> {
    if state.navigator.search_focused {
        return None;
    }
    reorder_direction_for_key(key)
}

impl AppState {
    /// Ordered top-level workspace indices in the grouped display order, one per
    /// worktree group (its header). Reordering steps through this list so a
    /// whole group counts as a single slot.
    fn top_level_group_heads(&self) -> Vec<usize> {
        self.visible_workspace_order()
            .into_iter()
            .filter(|&idx| {
                self.workspaces
                    .get(idx)
                    .is_some_and(|ws| crate::ui::workspace_parent_index(self, ws).is_none())
            })
            .collect()
    }

    /// Resolves the currently selected navigator row into the concrete workspace
    /// move needed to shift its project one slot in `direction`, or `None` when
    /// the move is not available (a filter or search is active, the selection is
    /// missing, or the project is already at the end of the list).
    pub(crate) fn plan_navigator_workspace_reorder(
        &self,
        terminal_runtimes: &TerminalRuntimeRegistry,
        direction: WorkspaceReorderDirection,
    ) -> Option<WorkspaceReorderCommand> {
        // Only reorder the full, unfiltered tree. Under a search query or a
        // state filter the rows are a reordered subset, so a positional move
        // would be meaningless.
        if !self.navigator.query.trim().is_empty() || self.navigator.state_filter.is_some() {
            return None;
        }

        let rows = self.navigator_rows_from(terminal_runtimes);
        let selected_ws_idx = rows.get(self.navigator.selected)?.target.ws_idx();
        self.plan_workspace_reorder(selected_ws_idx, direction)
    }

    /// Resolves the workspace owning `ws_idx` into the concrete move needed to
    /// shift its project one slot in `direction`, or `None` when unavailable
    /// (the project is already at the end, or it is not a reorderable top-level
    /// group root). Shared by the navigator overlay and the sidebar picker.
    pub(crate) fn plan_workspace_reorder(
        &self,
        ws_idx: usize,
        direction: WorkspaceReorderDirection,
    ) -> Option<WorkspaceReorderCommand> {
        let source = self.workspaces.get(ws_idx)?;
        // Act on the whole group: a worktree child moves its parent group.
        let head_idx = crate::ui::workspace_parent_index(self, source).unwrap_or(ws_idx);

        let heads = self.top_level_group_heads();
        let pos = heads.iter().position(|&idx| idx == head_idx)?;

        let drop_target = match direction {
            WorkspaceReorderDirection::Up => {
                let previous = *heads.get(pos.checked_sub(1)?)?;
                WorkspaceDropTarget::Before(previous)
            }
            WorkspaceReorderDirection::Down => {
                // Already last: nothing to move past.
                heads.get(pos + 1)?;
                // Land ahead of the group that follows the next one, i.e. jump
                // the immediate neighbour group as a single unit. If the next
                // group is the last, fall off the end.
                match heads.get(pos + 2) {
                    Some(&after_next) => WorkspaceDropTarget::Before(after_next),
                    None => WorkspaceDropTarget::End,
                }
            }
        };

        let params = self.workspace_move_block_params(head_idx, drop_target)?;

        // Move as an atomic block whenever the group holds more than one
        // workspace, however the grouping was established: a git-repo worktree
        // group, or a worktree explicitly filed under an ordinary workspace via
        // `parent_workspace_id` (whose header carries no `worktree_space`).
        // Inferring "is a group" from the header's worktree metadata alone would
        // pick Single for the latter and leave the filed child behind, so
        // `workspace.list`, persisted order, and agent sorting would disagree
        // with the grouped display order. A lone workspace moves singly.
        if params.workspace_ids.len() > 1 {
            Some(WorkspaceReorderCommand::Block(params))
        } else {
            let insert_idx = params
                .before_workspace_id
                .as_ref()
                .and_then(|id| self.workspaces.iter().position(|ws| ws.id == *id))
                .unwrap_or(self.workspaces.len());
            Some(WorkspaceReorderCommand::Single {
                source_ws_idx: head_idx,
                insert_idx,
            })
        }
    }

    /// Finds the row index of the row that logically matches `target` for the
    /// workspace whose id is `ws_id`, after the workspace list has been
    /// reordered. Used to keep the same row selected across a move.
    fn navigator_row_index_for(
        &self,
        terminal_runtimes: &TerminalRuntimeRegistry,
        ws_id: &str,
        target: &NavigatorTarget,
    ) -> Option<usize> {
        let rows = self.navigator_rows_from(terminal_runtimes);
        rows.iter().position(|row| {
            self.workspaces
                .get(row.target.ws_idx())
                .is_some_and(|ws| ws.id == ws_id)
                && match (&row.target, target) {
                    (NavigatorTarget::Workspace { .. }, NavigatorTarget::Workspace { .. }) => true,
                    (
                        NavigatorTarget::Tab { tab_idx: a, .. },
                        NavigatorTarget::Tab { tab_idx: b, .. },
                    ) => a == b,
                    (
                        NavigatorTarget::Pane { pane_id: a, .. },
                        NavigatorTarget::Pane { pane_id: b, .. },
                    ) => a == b,
                    _ => false,
                }
        })
    }
}

impl App {
    /// Navigator key entry point: handle a workspace reorder shortcut in the
    /// [`App`] context (so it can dispatch through the move API), otherwise fall
    /// back to the pure-state navigator key handling.
    pub(crate) fn handle_navigator_key_with_reorder(&mut self, key: KeyEvent) {
        if let Some(direction) = navigator_reorder_direction_for_key(&self.state, &key) {
            self.reorder_selected_navigator_workspace(direction);
        } else {
            super::modal::handle_navigator_key(&mut self.state, &self.terminal_runtimes, key);
        }
    }

    /// Moves the selected project one slot in `direction`, keeping the same row
    /// selected at its new position. No-op when no move is available.
    pub(crate) fn reorder_selected_navigator_workspace(
        &mut self,
        direction: WorkspaceReorderDirection,
    ) {
        let Some(command) = self
            .state
            .plan_navigator_workspace_reorder(&self.terminal_runtimes, direction)
        else {
            return;
        };

        // Capture the selected row's stable identity before the move so the
        // selection can follow the project to its new position.
        let restore = self
            .state
            .navigator_rows_from(&self.terminal_runtimes)
            .get(self.state.navigator.selected)
            .and_then(|row| {
                let target = row.target.clone();
                self.state
                    .workspaces
                    .get(target.ws_idx())
                    .map(|ws| (ws.id.clone(), target))
            });

        match command {
            WorkspaceReorderCommand::Single {
                source_ws_idx,
                insert_idx,
            } => {
                self.move_workspace_via_api(source_ws_idx, insert_idx);
            }
            WorkspaceReorderCommand::Block(params) => {
                self.move_workspace_block_via_api(params);
            }
        }

        if let Some((ws_id, target)) = restore {
            if let Some(idx) =
                self.state
                    .navigator_row_index_for(&self.terminal_runtimes, &ws_id, &target)
            {
                self.state.navigator.selected = idx;
            }
            self.state
                .clamp_navigator_selection_from(&self.terminal_runtimes);
        }
    }

    /// Sidebar-picker (Navigate mode) reorder: move the highlighted project one
    /// slot in `direction`. `move_workspace` / `move_workspace_block` keep
    /// `selected` tracking the same workspace by id, so the highlight follows
    /// the project to its new position. No-op when no move is available.
    pub(crate) fn reorder_selected_workspace(&mut self, direction: WorkspaceReorderDirection) {
        let Some(command) = self
            .state
            .plan_workspace_reorder(self.state.selected, direction)
        else {
            return;
        };
        match command {
            WorkspaceReorderCommand::Single {
                source_ws_idx,
                insert_idx,
            } => {
                self.move_workspace_via_api(source_ws_idx, insert_idx);
            }
            WorkspaceReorderCommand::Block(params) => {
                self.move_workspace_block_via_api(params);
            }
        }
        self.state.ensure_workspace_visible(self.state.selected);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::{Mode, NavigatorStateFilter};
    use crate::workspace::{Workspace, WorktreeSpaceMembership};

    fn rt() -> TerminalRuntimeRegistry {
        TerminalRuntimeRegistry::new()
    }

    fn navigator_state(names: &[&str]) -> AppState {
        let mut state = AppState::test_new();
        state.workspaces = names.iter().map(|name| Workspace::test_new(name)).collect();
        state.active = Some(0);
        state.selected = 0;
        state.mode = Mode::Navigator;
        for ws in &state.workspaces {
            state.navigator.expanded_workspaces.insert(ws.id.clone());
        }
        state
    }

    fn names(state: &AppState) -> Vec<String> {
        state
            .workspaces
            .iter()
            .map(|ws| ws.display_name())
            .collect()
    }

    /// Selects the first navigator row that belongs to the named workspace.
    fn select_workspace(state: &mut AppState, name: &str) {
        let rows = state.navigator_rows_from(&rt());
        let idx = rows
            .iter()
            .position(|row| {
                matches!(row.target, NavigatorTarget::Workspace { .. })
                    && state.workspaces[row.target.ws_idx()].display_name() == name
            })
            .expect("workspace row present");
        state.navigator.selected = idx;
    }

    /// Applies a planned command directly to state (no App/API round-trip) so the
    /// pure planning logic can be exercised in isolation.
    fn apply(state: &mut AppState, command: WorkspaceReorderCommand) {
        match command {
            WorkspaceReorderCommand::Single {
                source_ws_idx,
                insert_idx,
            } => {
                state.move_workspace(source_ws_idx, insert_idx);
            }
            WorkspaceReorderCommand::Block(params) => {
                state.move_workspace_block(
                    &params.workspace_ids,
                    params.before_workspace_id.as_deref(),
                );
            }
        }
    }

    #[test]
    fn move_up_swaps_with_previous_project() {
        let mut state = navigator_state(&["a", "b", "c"]);
        select_workspace(&mut state, "b");
        let command = state
            .plan_navigator_workspace_reorder(&rt(), WorkspaceReorderDirection::Up)
            .expect("move available");
        apply(&mut state, command);
        assert_eq!(names(&state), ["b", "a", "c"]);
    }

    #[test]
    fn move_down_swaps_with_next_project() {
        let mut state = navigator_state(&["a", "b", "c"]);
        select_workspace(&mut state, "a");
        let command = state
            .plan_navigator_workspace_reorder(&rt(), WorkspaceReorderDirection::Down)
            .expect("move available");
        apply(&mut state, command);
        assert_eq!(names(&state), ["b", "a", "c"]);
    }

    #[test]
    fn move_down_from_second_last_reaches_the_end() {
        let mut state = navigator_state(&["a", "b", "c"]);
        select_workspace(&mut state, "b");
        let command = state
            .plan_navigator_workspace_reorder(&rt(), WorkspaceReorderDirection::Down)
            .expect("move available");
        apply(&mut state, command);
        assert_eq!(names(&state), ["a", "c", "b"]);
    }

    #[test]
    fn move_up_at_top_is_noop() {
        let state = navigator_state(&["a", "b", "c"]);
        assert!(state
            .plan_navigator_workspace_reorder(&rt(), WorkspaceReorderDirection::Up)
            .is_none());
    }

    #[test]
    fn move_down_at_bottom_is_noop() {
        let mut state = navigator_state(&["a", "b", "c"]);
        select_workspace(&mut state, "c");
        assert!(state
            .plan_navigator_workspace_reorder(&rt(), WorkspaceReorderDirection::Down)
            .is_none());
    }

    #[test]
    fn reorder_disabled_while_filtering() {
        let mut state = navigator_state(&["a", "b", "c"]);
        select_workspace(&mut state, "b");
        state.navigator.state_filter = Some(NavigatorStateFilter::Working);
        assert!(state
            .plan_navigator_workspace_reorder(&rt(), WorkspaceReorderDirection::Up)
            .is_none());
        state.navigator.state_filter = None;
        state.navigator.query = "a".into();
        assert!(state
            .plan_navigator_workspace_reorder(&rt(), WorkspaceReorderDirection::Up)
            .is_none());
    }

    fn worktree_member(name: &str, key: &str, is_linked_worktree: bool) -> Workspace {
        let mut ws = Workspace::test_new(name);
        ws.worktree_space = Some(WorktreeSpaceMembership {
            key: key.into(),
            label: "herdr".into(),
            repo_root: "/repo/herdr".into(),
            checkout_path: format!("/repo/{name}").into(),
            is_linked_worktree,
        });
        ws
    }

    /// A worktree group (`main` header + a linked child) moves as one block and
    /// jumps a plain neighbour as a unit.
    #[test]
    fn move_up_carries_the_whole_worktree_group() {
        let mut state = AppState::test_new();
        state.workspaces = vec![
            Workspace::test_new("solo"),
            worktree_member("main", "repo", false),
            worktree_member("feature", "repo", true),
        ];
        state.active = Some(0);
        state.selected = 0;
        state.mode = Mode::Navigator;
        for ws in &state.workspaces {
            state.navigator.expanded_workspaces.insert(ws.id.clone());
        }
        select_workspace(&mut state, "main");

        let command = state
            .plan_navigator_workspace_reorder(&rt(), WorkspaceReorderDirection::Up)
            .expect("move available");
        assert!(matches!(command, WorkspaceReorderCommand::Block(_)));
        apply(&mut state, command);

        assert_eq!(names(&state), ["main", "feature", "solo"]);
    }

    /// A worktree explicitly filed under an ordinary workspace (via
    /// `parent_workspace_id`, so the header carries no `worktree_space`) must
    /// still move as a block, not leave the child stranded at its old index.
    #[test]
    fn move_carries_explicitly_filed_child_without_worktree_metadata() {
        let mut state = AppState::test_new();
        let solo = Workspace::test_new("solo");
        let parent = Workspace::test_new("parent");
        let mut child = Workspace::test_new("child");
        child.parent_workspace_id = Some(parent.id.clone());
        state.workspaces = vec![solo, parent, child];
        state.active = Some(0);
        state.selected = 0;
        state.mode = Mode::Navigator;
        for ws in &state.workspaces {
            state.navigator.expanded_workspaces.insert(ws.id.clone());
        }
        select_workspace(&mut state, "parent");

        let command = state
            .plan_navigator_workspace_reorder(&rt(), WorkspaceReorderDirection::Up)
            .expect("move available");
        assert!(matches!(command, WorkspaceReorderCommand::Block(_)));
        apply(&mut state, command);

        // The filed child rides along with its parent to the front.
        assert_eq!(names(&state), ["parent", "child", "solo"]);
    }

    #[test]
    fn direction_from_key_ignores_plain_letters() {
        let state = navigator_state(&["a", "b"]);
        let plain_j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::empty());
        assert!(navigator_reorder_direction_for_key(&state, &plain_j).is_none());

        let shift_j = KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT);
        assert_eq!(
            navigator_reorder_direction_for_key(&state, &shift_j),
            Some(WorkspaceReorderDirection::Down)
        );
        let shift_k = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::SHIFT);
        assert_eq!(
            navigator_reorder_direction_for_key(&state, &shift_k),
            Some(WorkspaceReorderDirection::Up)
        );
    }

    #[test]
    fn direction_from_key_ignores_reorder_while_searching() {
        let mut state = navigator_state(&["a", "b"]);
        state.navigator.search_focused = true;
        let shift_j = KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT);
        assert!(navigator_reorder_direction_for_key(&state, &shift_j).is_none());
    }

    /// The sidebar-picker plan is driven by an explicit `ws_idx` and has no
    /// search/filter guard.
    #[test]
    fn plan_workspace_reorder_moves_by_index() {
        let mut state = navigator_state(&["a", "b", "c"]);
        let command = state
            .plan_workspace_reorder(1, WorkspaceReorderDirection::Up)
            .expect("move available");
        apply(&mut state, command);
        assert_eq!(names(&state), ["b", "a", "c"]);
    }

    /// End-to-end through [`App`] in Navigate mode: dispatch persists the move
    /// and the highlight follows the project by id.
    #[test]
    fn app_navigate_mode_reorder_moves_and_follows_selection() {
        let mut app = super::super::app_for_mouse_test();
        app.state.workspaces = ["a", "b", "c"]
            .iter()
            .map(|name| Workspace::test_new(name))
            .collect();
        app.state.active = Some(0);
        app.state.selected = 1;
        app.state.mode = Mode::Navigate;

        app.reorder_selected_workspace(WorkspaceReorderDirection::Up);

        assert_eq!(names(&app.state), ["b", "a", "c"]);
        // The highlight tracks the same workspace ("b") to its new slot.
        assert_eq!(app.state.workspaces[app.state.selected].display_name(), "b");
    }

    /// `shift+k` / `shift+j` in Navigate mode reorder and consume the key
    /// instead of falling through to the exit path.
    #[test]
    fn navigate_key_shift_j_reorders_without_leaving_navigate_mode() {
        let mut app = super::super::app_for_mouse_test();
        app.state.workspaces = ["a", "b", "c"]
            .iter()
            .map(|name| Workspace::test_new(name))
            .collect();
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Navigate;

        let shift_j = crate::input::TerminalKey::new(KeyCode::Char('J'), KeyModifiers::SHIFT);
        app.handle_navigate_key(shift_j);

        assert_eq!(names(&app.state), ["b", "a", "c"]);
        assert_eq!(app.state.mode, Mode::Navigate);
        assert_eq!(app.state.workspaces[app.state.selected].display_name(), "a");
    }

    /// End-to-end through [`App`]: dispatch persists the move via the API and the
    /// same project stays selected at its new position.
    #[test]
    fn app_reorder_moves_and_follows_selection() {
        let mut app = super::super::app_for_mouse_test();
        app.state.workspaces = ["a", "b", "c"]
            .iter()
            .map(|name| Workspace::test_new(name))
            .collect();
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.open_navigator();
        select_workspace(&mut app.state, "b");

        app.reorder_selected_navigator_workspace(WorkspaceReorderDirection::Up);

        assert_eq!(names(&app.state), ["b", "a", "c"]);
        let rows = app.state.navigator_rows_from(&app.terminal_runtimes);
        let selected = &rows[app.state.navigator.selected];
        assert_eq!(
            app.state.workspaces[selected.target.ws_idx()].display_name(),
            "b"
        );
    }
}
