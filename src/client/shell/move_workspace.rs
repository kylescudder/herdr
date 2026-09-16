//! The "move workspace to workspace" modal picker.
//!
//! Fork-owned: upstream has no equivalent, so this file cannot conflict on an
//! upstream merge. What it leaves behind in upstream files are the hooks listed
//! in `FORK.md` — the `ClientShellOverlay::MoveWorkspace` variant, the key and
//! mouse routing arms, and the overlay render arm.
//!
//! The original feature (`f6f7cf40`) drew a **modal dialog**, not a sidebar
//! navigation mode. Keep it that way; a port that rebuilt it as a navigate mode
//! is what `FORK.md`'s "one rule" exists to prevent.

use super::*;
use crossterm::event::KeyCode;

/// A candidate destination in the "move to workspace" picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClientMoveWorkspaceTarget {
    /// Target workspace public id, or `None` for "top level" (unfile).
    pub(crate) target_id: Option<String>,
    pub(crate) label: String,
}

/// Picker for filing one workspace under another. The original feature drew a
/// modal list rather than reusing sidebar navigation, so keep it an overlay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClientMoveWorkspaceOverlay {
    /// Public id of the workspace being moved.
    pub(crate) workspace_id: String,
    pub(crate) label: String,
    pub(crate) entries: Vec<ClientMoveWorkspaceTarget>,
    pub(crate) selected: usize,
    pub(crate) error: Option<String>,
    pub(crate) moving: bool,
}

impl ClientMoveWorkspaceOverlay {
    pub(crate) fn selected_target_id(&self) -> Option<String> {
        self.entries
            .get(self.selected)
            .and_then(|target| target.target_id.clone())
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let last = self.entries.len() - 1;
        let next = self.selected as isize + delta;
        self.selected = next.clamp(0, last as isize) as usize;
    }
}

impl ClientShellState {
    /// Opens the "move to workspace" picker for `workspace_id`. Candidates are
    /// "top level" (unfile) plus every other workspace; the server rejects
    /// cycles and the message is surfaced in the overlay.
    pub(crate) fn open_move_workspace_overlay(&mut self, workspace_id: String) -> bool {
        let Some(snapshot) = self.snapshot.as_deref() else {
            return false;
        };
        let Some(moving) = snapshot
            .workspaces
            .iter()
            .find(|workspace| workspace.workspace_id == workspace_id)
        else {
            return false;
        };
        let label = moving.label.clone();
        let current_parent = moving
            .tokens
            .iter()
            .find(|(name, _)| name == crate::protocol::PARENT_WORKSPACE_TOKEN)
            .map(|(_, value)| value.clone());

        let mut entries = vec![ClientMoveWorkspaceTarget {
            target_id: None,
            label: "top level".to_owned(),
        }];
        entries.extend(
            snapshot
                .workspaces
                .iter()
                .filter(|candidate| candidate.workspace_id != workspace_id)
                .map(|candidate| ClientMoveWorkspaceTarget {
                    target_id: Some(candidate.workspace_id.clone()),
                    label: candidate.label.clone(),
                }),
        );
        // Start on the current parent so the picker shows where it is filed now.
        let selected = current_parent
            .as_deref()
            .and_then(|parent| {
                entries
                    .iter()
                    .position(|entry| entry.target_id.as_deref() == Some(parent))
            })
            .unwrap_or(0);

        self.overlay = Some(ClientShellOverlay::MoveWorkspace(
            ClientMoveWorkspaceOverlay {
                workspace_id,
                label,
                entries,
                selected,
                error: None,
                moving: false,
            },
        ));
        true
    }

    pub(crate) fn submit_move_workspace(&mut self, outcome: &mut ClientShellInput) {
        let Some(ClientShellOverlay::MoveWorkspace(move_overlay)) = self.overlay.as_mut() else {
            return;
        };
        if move_overlay.moving {
            return;
        }
        let workspace_id = move_overlay.workspace_id.clone();
        let parent_workspace_id = move_overlay.selected_target_id();
        move_overlay.moving = true;
        move_overlay.error = None;
        if !self.push_endpoint_method_with_kind(
            crate::api::schema::Method::WorkspaceReparent(
                crate::api::schema::WorkspaceReparentParams {
                    workspace_id,
                    parent_workspace_id,
                },
            ),
            PendingEndpointKind::MoveWorkspace,
            outcome,
        ) {
            if let Some(ClientShellOverlay::MoveWorkspace(move_overlay)) = self.overlay.as_mut() {
                move_overlay.moving = false;
            }
        }
        outcome.repaint = true;
    }

    /// Key routing while the picker is open. Returns once the key is consumed —
    /// every key is, because a modal swallows input.
    pub(crate) fn route_move_workspace_key(
        &mut self,
        code: KeyCode,
        outcome: &mut ClientShellInput,
    ) {
        let moving = matches!(
            self.overlay,
            Some(ClientShellOverlay::MoveWorkspace(
                ClientMoveWorkspaceOverlay { moving: true, .. }
            ))
        );
        match code {
            KeyCode::Esc if !moving => {
                self.overlay = None;
                outcome.repaint = true;
            }
            KeyCode::Enter => self.submit_move_workspace(outcome),
            // j/k alongside the arrows, matching the original picker.
            KeyCode::Up | KeyCode::Char('k') if !moving => {
                if let Some(ClientShellOverlay::MoveWorkspace(move_overlay)) = self.overlay.as_mut()
                {
                    move_overlay.move_selection(-1);
                }
                outcome.repaint = true;
            }
            KeyCode::Down | KeyCode::Char('j') if !moving => {
                if let Some(ClientShellOverlay::MoveWorkspace(move_overlay)) = self.overlay.as_mut()
                {
                    move_overlay.move_selection(1);
                }
                outcome.repaint = true;
            }
            _ => {}
        }
    }

    /// Settles a `workspace.reparent` response. Success closes the picker; a
    /// rejection (a cycle, say) keeps it open and shows the server's message.
    pub(crate) fn handle_move_workspace_result(&mut self, error: Option<String>) {
        match error {
            None => self.overlay = None,
            Some(message) => {
                if let Some(ClientShellOverlay::MoveWorkspace(move_overlay)) = self.overlay.as_mut()
                {
                    move_overlay.moving = false;
                    move_overlay.error = Some(message);
                }
            }
        }
    }
}

pub(super) fn render_move_workspace_overlay(
    b: &mut Buffer,
    move_overlay: &ClientMoveWorkspaceOverlay,
    p: &Palette,
) -> Option<OverlayRender> {
    const MAX_ROWS: usize = 8;
    let visible = move_overlay.entries.len().clamp(1, MAX_ROWS);
    let popup = popup(b.area, 64, visible as u16 + 8)?;
    let inner = panel(b, popup, p.accent, p.panel_bg)?;
    put_text(
        b,
        inner.x,
        inner.y,
        inner.width,
        &format!(" move {} to…", move_overlay.label),
        Style::default()
            .fg(p.text)
            .bg(p.panel_bg)
            .add_modifier(Modifier::BOLD),
    );
    put_text(
        b,
        inner.x,
        inner.y + 1,
        inner.width,
        " Pick the workspace to file it under.",
        Style::default().fg(p.subtext0).bg(p.panel_bg),
    );

    // Scroll so the selected row stays visible for long workspace lists.
    let first = move_overlay
        .selected
        .saturating_sub(visible.saturating_sub(1));
    let mut row_hits = Vec::new();
    for (offset, index) in (first..move_overlay.entries.len())
        .take(visible)
        .enumerate()
    {
        let entry = &move_overlay.entries[index];
        let selected = index == move_overlay.selected;
        row_hits.push((
            Rect::new(inner.x, inner.y + 3 + offset as u16, inner.width, 1),
            index,
        ));
        let style = if selected {
            Style::default()
                .fg(contrast(p))
                .bg(p.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.text).bg(p.panel_bg)
        };
        let marker = if selected { "▸" } else { " " };
        put_text(
            b,
            inner.x,
            inner.y + 3 + offset as u16,
            inner.width,
            &format!(" {marker} {}", entry.label),
            style,
        );
    }

    let status_y = inner.y + 3 + visible as u16;
    if move_overlay.moving {
        put_text(
            b,
            inner.x,
            status_y,
            inner.width,
            " moving…",
            Style::default().fg(p.accent).bg(p.panel_bg),
        );
    } else if let Some(error) = move_overlay.error.as_deref() {
        put_text(
            b,
            inner.x,
            status_y,
            inner.width,
            &format!(" {error}"),
            Style::default().fg(p.red).bg(p.panel_bg),
        );
    } else {
        put_text(
            b,
            inner.x,
            status_y,
            inner.width,
            " ↑/↓ or j/k to choose",
            Style::default().fg(p.subtext0).bg(p.panel_bg),
        );
    }

    let buttons = row(inner, &[14, 12], 2, inner.height.saturating_sub(1));
    let [primary, cancel] = buttons.as_slice() else {
        return None;
    };
    button(
        b,
        *primary,
        " ↵ move ",
        Style::default()
            .fg(contrast(p))
            .bg(p.accent)
            .add_modifier(Modifier::BOLD),
    );
    button(
        b,
        *cancel,
        " esc cancel ",
        Style::default()
            .fg(p.text)
            .bg(p.surface0)
            .add_modifier(Modifier::BOLD),
    );
    Some(OverlayRender {
        primary: *primary,
        cancel: *cancel,
        clear: Rect::default(),
        // Reuses the worktree row hit channel; the mouse router dispatches on
        // the active overlay, so the rows cannot be confused with another's.
        worktree_rows: row_hits,
        cursor: None,
        ..OverlayRender::default()
    })
}
