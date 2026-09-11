//! Tests for fork-carried workspace grouping, the move-to-workspace picker,
//! and keyboard/mouse reordering.
//!
//! These live in their own file so upstream edits to the shared client-shell
//! test files never conflict with them. See FORK.md.

use super::*;

#[test]
fn grouped_worktrees_render_parent_branch_and_indented_child() {
    let config = ClientShellConfig::from_config(&Config::default());
    let mut state = ClientShellState::new(config);
    let mut snapshot = snapshot();
    snapshot.workspaces[0].worktree = Some(ClientShellWorktree {
        key: "repo".into(),
        label: "repo".into(),
        is_linked_worktree: false,
    });
    snapshot.workspaces.push(ClientShellWorkspace {
        workspace_id: "ws_2".into(),
        active_tab_id: "tab_ws2".into(),
        new_workspace_cwd: "/repo/feature".into(),
        number: 2,
        label: "repo-feature".into(),
        custom_label: false,
        branch: Some("worktree/feature".into()),
        git_ahead_behind: None,
        tokens: Vec::new(),
        worktree: Some(ClientShellWorktree {
            key: "repo".into(),
            label: "repo".into(),
            is_linked_worktree: true,
        }),
        focused: false,
        agent_status: AgentStatus::Idle,
    });
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    let frame = state.compose(106, 20).expect("composed frame");
    let text = frame
        .cells
        .chunks(frame.width as usize)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("main"));
    assert!(text.contains("└─"));
    assert!(text.contains("feature"));

    let mut replacement = (**state.snapshot.as_ref().expect("snapshot")).clone();
    replacement.revision = 2;
    replacement.focused_workspace_id = Some("ws_2".into());
    replacement.workspaces[0].focused = false;
    replacement.workspaces[1].focused = true;
    replacement.workspaces[1].agent_status = AgentStatus::Blocked;
    let mut replacement_surface = surface();
    replacement_surface.projection_revision = 2;
    // Groups collapse by the parent workspace id (ws_1), not the git repo key.
    state.collapsed_groups.insert("ws_1".into());
    state.set_snapshot(Box::new(replacement));
    state.set_pane_surface(replacement_surface);
    let collapsed = state.compose(106, 20).expect("collapsed worktree group");
    let parent = state.hits.workspaces[0].rect;
    let status_cell = usize::from(parent.y) * usize::from(collapsed.width)
        + usize::from(parent.x.saturating_add(1));
    assert_eq!(
        collapsed.cells[status_cell].fg,
        crate::protocol::color_to_u32(state.config.palette.red)
    );

    let mut next = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::NextWorkspace),
        &mut next,
    );
    assert!(matches!(
        &next.actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(
                &request.method,
                crate::api::schema::Method::WorkspaceFocus(target)
                    if target.workspace_id == "ws_1"
            )
    ));
}

#[test]
fn same_repo_non_linked_workspaces_stay_separate_top_level() {
    // Two projects in one monorepo (same repo key, both non-linked checkouts)
    // must render as separate top-level spaces, not nested into one group.
    let config = ClientShellConfig::from_config(&Config::default());
    let mut state = ClientShellState::new(config);
    let mut snapshot = snapshot();
    snapshot.workspaces[0].label = "odyssey".into();
    snapshot.workspaces[0].worktree = Some(ClientShellWorktree {
        key: "monorepo".into(),
        label: "monorepo".into(),
        is_linked_worktree: false,
    });
    snapshot.workspaces.push(ClientShellWorkspace {
        workspace_id: "ws_2".into(),
        active_tab_id: "tab_ws2".into(),
        new_workspace_cwd: "/repo/bifrost".into(),
        number: 2,
        label: "bifrost".into(),
        custom_label: false,
        branch: Some("main".into()),
        git_ahead_behind: None,
        tokens: Vec::new(),
        worktree: Some(ClientShellWorktree {
            key: "monorepo".into(),
            label: "monorepo".into(),
            is_linked_worktree: false,
        }),
        focused: false,
        agent_status: AgentStatus::Idle,
    });
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    let frame = state.compose(106, 20).expect("composed frame");
    let text = frame
        .cells
        .chunks(frame.width as usize)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("odyssey"), "odyssey rendered");
    assert!(text.contains("bifrost"), "bifrost rendered");
    // No worktree-nesting glyphs: neither is indented under the other.
    assert!(
        !text.contains("└─") && !text.contains("├─"),
        "same-repo non-linked workspaces must not nest:\n{text}"
    );
    // Both are top-level rows.
    assert_eq!(state.hits.workspaces.len(), 2);
}

#[test]
fn move_workspace_keybind_reorders_focused_project() {
    let config = ClientShellConfig::from_config(&Config::default());
    let mut state = ClientShellState::new(config);
    let mut snapshot = snapshot();
    snapshot.workspaces[0].label = "a".into();
    snapshot.workspaces[0].focused = false;
    for (index, label) in [(2, "b"), (3, "c")] {
        let mut workspace = snapshot.workspaces[0].clone();
        workspace.workspace_id = format!("ws_{index}");
        workspace.number = index;
        workspace.label = label.into();
        workspace.focused = index == 2;
        snapshot.workspaces.push(workspace);
    }
    snapshot.focused_workspace_id = Some("ws_2".into());
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());

    // Move up: "b" lands before "a" via a single WorkspaceMove at index 0.
    let mut up = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::MoveWorkspacePrevious),
        &mut up,
    );
    assert!(
        matches!(
            &up.actions[..],
            [ClientShellAction::Endpoint { request, .. }]
                if matches!(
                    &request.method,
                    crate::api::schema::Method::WorkspaceMove(params)
                        if params.workspace_id == "ws_2" && params.insert_index == 0
                )
        ),
        "expected WorkspaceMove(ws_2 -> 0), got {:?}",
        up.actions
    );
}

#[test]
fn move_workspace_keybind_at_top_is_noop() {
    let config = ClientShellConfig::from_config(&Config::default());
    let mut state = ClientShellState::new(config);
    let snapshot = snapshot(); // single workspace ws_1, focused
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    let mut up = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::MoveWorkspacePrevious),
        &mut up,
    );
    assert!(
        !up.actions.iter().any(|action| matches!(
            action,
            ClientShellAction::Endpoint { request, .. }
                if matches!(
                    request.method,
                    crate::api::schema::Method::WorkspaceMove(_)
                        | crate::api::schema::Method::WorkspaceMoveBlock(_)
                )
        )),
        "top project must not emit a move: {:?}",
        up.actions
    );
}

#[test]
fn reorder_moves_an_explicit_parent_group_as_one_block() {
    // Reorder must use the sidebar's grouping. A workspace with an explicitly
    // filed child moves as a block, and the child is not independently
    // reorderable, or shift+j/k silently does nothing once anything is nested.
    let config = ClientShellConfig::from_config(&Config::default());
    let mut state = ClientShellState::new(config);
    let mut snapshot = snapshot();
    for (index, label) in [(2, "middle"), (3, "child")] {
        let mut extra = snapshot.workspaces[0].clone();
        extra.workspace_id = format!("ws_{index}");
        extra.number = index;
        extra.label = label.into();
        extra.focused = false;
        snapshot.workspaces.push(extra);
    }
    // ws_3 is filed under ws_1, so the top-level rows are ws_1 then ws_2.
    snapshot.workspaces[2].tokens.push((
        crate::protocol::PARENT_WORKSPACE_TOKEN.into(),
        "ws_1".into(),
    ));
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());

    // Moving the parent down carries its child as a block.
    let method = state
        .workspace_reorder_method("ws_1", false)
        .expect("parent should reorder");
    assert!(
        matches!(
            &method,
            crate::api::schema::Method::WorkspaceMoveBlock(params)
                if params.workspace_ids == vec!["ws_1".to_owned(), "ws_3".to_owned()]
        ),
        "parent must move with its child: {method:?}"
    );

    // The nested child resolves to its parent rather than moving alone.
    let from_child = state
        .workspace_reorder_method("ws_3", false)
        .expect("child should reorder its parent group");
    assert!(
        matches!(
            &from_child,
            crate::api::schema::Method::WorkspaceMoveBlock(params)
                if params.workspace_ids == vec!["ws_1".to_owned(), "ws_3".to_owned()]
        ),
        "child must move its parent's block: {from_child:?}"
    );

    // An ungrouped top-level workspace still moves on its own.
    let lone = state
        .workspace_reorder_method("ws_2", true)
        .expect("middle workspace should reorder");
    assert!(
        matches!(
            &lone,
            crate::api::schema::Method::WorkspaceMove(params) if params.workspace_id == "ws_2"
        ),
        "lone workspace moves singly: {lone:?}"
    );
}

/// Two workspaces: focused "ws_1" plus a "parent" candidate to file it under.
fn move_picker_state() -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    let mut snapshot = snapshot();
    let mut second = snapshot.workspaces[0].clone();
    second.workspace_id = "ws_2".into();
    second.number = 2;
    second.label = "parent".into();
    second.focused = false;
    snapshot.workspaces.push(second);
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    state
}

#[test]
fn move_worktree_keybind_opens_the_move_workspace_picker() {
    // The original feature drew a modal picker; the keybind must open it rather
    // than silently repurposing sidebar navigation.
    let mut state = move_picker_state();

    let mut out = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::MoveWorktreeToWorkspace),
        &mut out,
    );

    let Some(ClientShellOverlay::MoveWorkspace(picker)) = state.overlay.as_ref() else {
        panic!(
            "expected the move-to-workspace picker, got {:?}",
            state.overlay
        );
    };
    assert_eq!(picker.workspace_id, "ws_1");
    // "top level" plus every other workspace.
    assert_eq!(
        picker
            .entries
            .iter()
            .map(|entry| entry.label.as_str())
            .collect::<Vec<_>>(),
        vec!["top level", "parent"]
    );
}

#[test]
fn move_workspace_picker_renders_a_modal_listing_targets() {
    let mut state = move_picker_state();
    let mut out = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::MoveWorktreeToWorkspace),
        &mut out,
    );

    let frame = state.compose(106, 24).expect("composed frame");
    let text = frame
        .cells
        .chunks(frame.width as usize)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    for expected in ["move", "top level", "parent", "esc cancel"] {
        assert!(
            text.contains(expected),
            "picker must render {expected:?}; frame was:\n{text}"
        );
    }
}

#[test]
fn move_workspace_picker_files_under_the_selected_target() {
    let mut state = move_picker_state();
    let mut out = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::MoveWorktreeToWorkspace),
        &mut out,
    );

    // j moves off "top level" onto the parent workspace, enter commits.
    state.handle_input_bytes(b"j");
    let filed = state.handle_input_bytes(b"\r");
    let [ClientShellAction::Endpoint { request, .. }] = &filed.actions[..] else {
        panic!("expected a reparent endpoint, got {:?}", filed.actions);
    };
    assert!(
        matches!(
            &request.method,
            crate::api::schema::Method::WorkspaceReparent(params)
                if params.workspace_id == "ws_1"
                    && params.parent_workspace_id.as_deref() == Some("ws_2")
        ),
        "got {:?}",
        request.method
    );
}

#[test]
fn move_workspace_picker_top_level_entry_unfiles() {
    let mut state = move_picker_state();
    let mut out = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::MoveWorktreeToWorkspace),
        &mut out,
    );

    // "top level" is selected first, so enter unfiles to the top level.
    let filed = state.handle_input_bytes(b"\r");
    let [ClientShellAction::Endpoint { request, .. }] = &filed.actions[..] else {
        panic!("expected a reparent endpoint, got {:?}", filed.actions);
    };
    assert!(
        matches!(
            &request.method,
            crate::api::schema::Method::WorkspaceReparent(params)
                if params.workspace_id == "ws_1" && params.parent_workspace_id.is_none()
        ),
        "got {:?}",
        request.method
    );
}

#[test]
fn move_workspace_picker_escape_cancels_without_moving() {
    let mut state = move_picker_state();
    let mut out = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::MoveWorktreeToWorkspace),
        &mut out,
    );

    let cancelled = state.handle_input_bytes(b"\x1b");
    assert!(state.overlay.is_none(), "esc must close the picker");
    assert!(
        cancelled.actions.is_empty(),
        "esc must not move anything: {:?}",
        cancelled.actions
    );
}

#[test]
fn shift_m_opens_the_move_picker_from_inside_the_workspace_picker() {
    // Real user flow: prefix+w to list workspaces, then shift+m on the
    // selection. Dispatching from Navigate mode must still open the modal.
    let mut state = move_picker_state();
    state.compose(106, 24).expect("composed frame");

    let (prefix_key, prefix_modifiers) = state.config.keybinds.prefix;
    state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        prefix_key,
        prefix_modifiers,
    ))]);
    state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        KeyCode::Char('w'),
        KeyModifiers::empty(),
    ))]);
    assert_eq!(state.mode, ClientShellMode::Navigate);

    state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        KeyCode::Char('M'),
        KeyModifiers::SHIFT,
    ))]);
    let Some(ClientShellOverlay::MoveWorkspace(picker)) = state.overlay.as_ref() else {
        panic!(
            "shift+m inside the workspace picker must open the move picker, got {:?}",
            state.overlay
        );
    };
    assert_eq!(picker.workspace_id, "ws_1");
}

#[test]
fn move_workspace_picker_starts_on_the_current_parent() {
    let mut state = move_picker_state();
    // Pretend ws_1 is already filed under ws_2.
    let mut updated = (**state.snapshot.as_ref().expect("snapshot")).clone();
    updated.revision = 2;
    updated.workspaces[0].tokens.push((
        crate::protocol::PARENT_WORKSPACE_TOKEN.into(),
        "ws_2".into(),
    ));
    let mut surface = surface();
    surface.projection_revision = 2;
    state.set_snapshot(Box::new(updated));
    state.set_pane_surface(surface);

    let mut out = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::MoveWorktreeToWorkspace),
        &mut out,
    );
    let Some(ClientShellOverlay::MoveWorkspace(picker)) = state.overlay.as_ref() else {
        panic!("expected the move picker, got {:?}", state.overlay);
    };
    assert_eq!(
        picker.selected_target_id().as_deref(),
        Some("ws_2"),
        "the picker should open on the current parent"
    );
}

#[test]
fn explicit_parent_token_nests_under_chosen_workspace() {
    // odyssey + bifrost share one monorepo (same repo key, non-linked) so they
    // stay separate top-level. A worktree explicitly filed under bifrost nests
    // under bifrost even though the git key alone could not disambiguate.
    let config = ClientShellConfig::from_config(&Config::default());
    let mut state = ClientShellState::new(config);
    let mut snapshot = snapshot();
    snapshot.workspaces[0].label = "odyssey".into();
    snapshot.workspaces[0].workspace_id = "ws_1".into();
    snapshot.workspaces[0].worktree = Some(ClientShellWorktree {
        key: "monorepo".into(),
        label: "monorepo".into(),
        is_linked_worktree: false,
    });
    for (index, label, linked, parent) in [
        (2, "bifrost", false, None),
        (3, "feature", true, Some("ws_2")),
    ] {
        let mut ws = snapshot.workspaces[0].clone();
        ws.workspace_id = format!("ws_{index}");
        ws.number = index;
        ws.label = label.into();
        ws.focused = false;
        ws.worktree = Some(ClientShellWorktree {
            key: "monorepo".into(),
            label: "monorepo".into(),
            is_linked_worktree: linked,
        });
        if let Some(parent) = parent {
            ws.tokens.push((
                crate::protocol::PARENT_WORKSPACE_TOKEN.into(),
                parent.into(),
            ));
        }
        snapshot.workspaces.push(ws);
    }
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    state.compose(106, 20).expect("composed frame");

    let hit = |id: &str| {
        state
            .hits
            .workspaces
            .iter()
            .find(|hit| hit.workspace_id == id)
            .unwrap_or_else(|| panic!("hit for {id}"))
            .indented
    };
    assert!(!hit("ws_1"), "odyssey stays top-level");
    assert!(!hit("ws_2"), "bifrost stays top-level");
    assert!(
        hit("ws_3"),
        "the worktree nests under its explicit parent bifrost"
    );
}

#[test]
fn workspace_drag_moves_parent_worktree_as_one_block_and_rejects_child() {
    let mut projected = snapshot();
    projected.workspaces[0].worktree = Some(ClientShellWorktree {
        key: "repo".into(),
        label: "repo".into(),
        is_linked_worktree: false,
    });
    let mut child = projected.workspaces[0].clone();
    child.workspace_id = "ws_child".into();
    child.number = 2;
    child.label = "feature".into();
    child.focused = false;
    child.worktree = Some(ClientShellWorktree {
        key: "repo".into(),
        label: "repo".into(),
        is_linked_worktree: true,
    });
    let mut other = projected.workspaces[0].clone();
    other.workspace_id = "ws_other".into();
    other.number = 3;
    other.label = "other".into();
    other.focused = false;
    other.worktree = None;
    projected.workspaces.extend([child, other]);

    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    state.compose(106, 24).expect("worktree workspaces");
    assert!(state.hits.workspaces[1].indented);
    let parent = state.hits.workspaces[0].rect;
    let child = state.hits.workspaces[1].rect;
    let other = state.hits.workspaces[2].rect;

    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: parent.x + 2,
        row: parent.y,
        modifiers: KeyModifiers::empty(),
    })]);
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: other.x + 2,
        row: other.bottom(),
        modifiers: KeyModifiers::empty(),
    })]);
    let moved = state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: other.x + 2,
        row: other.bottom(),
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(matches!(
        &moved.actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(
                &request.method,
                crate::api::schema::Method::WorkspaceMoveBlock(params)
                    if params.workspace_ids == ["ws_1", "ws_child"]
                        && params.before_workspace_id.is_none()
            )
    ));

    state.compose(106, 24).expect("worktree child");
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: child.x + 2,
        row: child.y,
        modifiers: KeyModifiers::empty(),
    })]);
    let dragging_child =
        state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: other.x + 2,
            row: other.bottom(),
            modifiers: KeyModifiers::empty(),
        })]);
    assert!(dragging_child.actions.is_empty());
    assert!(state.chrome_drag.is_none());
}

#[test]
fn move_workspace_picker_files_a_clicked_target() {
    // Herdr is mouse-first: clicking a target row must select and commit it,
    // not dismiss the picker.
    let mut state = move_picker_state();
    let mut out = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::MoveWorktreeToWorkspace),
        &mut out,
    );
    state.compose(106, 24).expect("composed frame");

    // Row 1 is the "parent" workspace ("top level" is row 0).
    let (rect, _) = state
        .hits
        .worktree_rows
        .iter()
        .copied()
        .find(|(_, index)| *index == 1)
        .expect("picker should publish target row hits");

    let clicked =
        state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x + 1,
            row: rect.y,
            modifiers: KeyModifiers::empty(),
        })]);
    let [ClientShellAction::Endpoint { request, .. }] = &clicked.actions[..] else {
        panic!("clicking a target must file it, got {:?}", clicked.actions);
    };
    assert!(
        matches!(
            &request.method,
            crate::api::schema::Method::WorkspaceReparent(params)
                if params.workspace_id == "ws_1"
                    && params.parent_workspace_id.as_deref() == Some("ws_2")
        ),
        "got {:?}",
        request.method
    );
}
