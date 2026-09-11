//! Integration-point guards for this fork's carried features.
//!
//! Most of the fork's code lives in its own modules, which never conflict when
//! upstream is merged in. What does break is the handful of one-line *hooks*
//! into upstream files: an enum variant, a match arm, a name in a registration
//! list. Those fail **silently** — nothing stops compiling when a method is
//! missing from `request_changes_ui`, it just stops re-rendering.
//!
//! Every test here asserts one such hook and names the file to restore it in.
//! After an upstream merge, a dropped hook shows up as a red test in seconds
//! instead of hours of debugging. See `FORK.md` for the full map.

#[cfg(test)]
mod tests {
    /// Hook: `src/api/mod.rs` — `request_changes_ui`.
    ///
    /// Grouping is sidebar-visible, so the method must be listed there. The
    /// server only re-projects the client-shell snapshot during a render pass;
    /// without this the reparent persists but the sidebar keeps the old
    /// grouping until restart.
    #[test]
    fn reparent_is_registered_as_a_ui_changing_request() {
        let request = crate::api::schema::Request {
            id: "guard".into(),
            method: crate::api::schema::Method::WorkspaceReparent(
                crate::api::schema::WorkspaceReparentParams {
                    workspace_id: "w1".into(),
                    parent_workspace_id: Some("w2".into()),
                },
            ),
        };
        assert!(
            crate::api::request_changes_ui(&request),
            "workspace.reparent must be listed in request_changes_ui \
             (src/api/mod.rs), or the sidebar never re-renders after a move"
        );
    }

    /// Hook: `src/server/client_commands.rs` — `CLIENT_SHELL_METHODS`.
    ///
    /// The TUI reaches the server over the client-shell command lane, which is
    /// allowlisted by method name. Missing here, the picker's Enter is rejected
    /// with `unsupported_endpoint_command`.
    #[test]
    fn reparent_is_allowed_on_the_client_shell_lane() {
        assert!(
            crate::server::client_commands::supports_client_shell_method_name("workspace.reparent"),
            "workspace.reparent must be in CLIENT_SHELL_METHODS \
             (src/server/client_commands.rs), or the TUI cannot invoke it"
        );
    }

    /// Hook: `src/input/keybindings.rs` — the `resolve_non_indexed_action`
    /// table, plus the `move_worktree` field in `src/config/{model,keybinds}.rs`.
    ///
    /// Exercises the real config path rather than a hand-built `Keybinds`, so a
    /// binding dropped from any of those three files is caught.
    #[test]
    fn move_worktree_keybind_resolves_from_the_default_config() {
        let config = crate::config::Config::default();
        let (live, _diagnostics) = config
            .live_keybinds_with_diagnostics()
            .expect("default keybinds must be valid");

        // A shifted letter can reach the client in either representation.
        for key in [
            crate::input::TerminalKey::new(
                crossterm::event::KeyCode::Char('m'),
                crossterm::event::KeyModifiers::SHIFT,
            ),
            crate::input::TerminalKey::new(
                crossterm::event::KeyCode::Char('M'),
                crossterm::event::KeyModifiers::empty(),
            ),
            crate::input::TerminalKey::new(
                crossterm::event::KeyCode::Char('M'),
                crossterm::event::KeyModifiers::SHIFT,
            ),
        ] {
            assert!(
                matches!(
                    crate::input::resolve_prefix_binding(&live.keybinds, &key),
                    Some(crate::input::KeybindMatch::Action(
                        crate::input::KeybindAction::MoveWorktreeToWorkspace
                    ))
                ),
                "prefix+shift+m must resolve to MoveWorktreeToWorkspace for {key:?}; \
                 check the resolve_non_indexed_action table (src/input/keybindings.rs) \
                 and the move_worktree field in src/config/model.rs + keybinds.rs"
            );
        }
    }

    /// Hook: `src/input/keybind_help.rs`.
    ///
    /// Cosmetic, but it was silently dropped once already: the binding worked
    /// while `prefix+?` never listed it.
    #[test]
    fn move_worktree_is_listed_in_keybind_help() {
        let config = crate::config::Config::default();
        let (live, _diagnostics) = config
            .live_keybinds_with_diagnostics()
            .expect("default keybinds must be valid");
        let groups = crate::input::keybind_help_groups(&live.keybinds, live.prefix);
        let listed =
            groups
                .iter()
                .flat_map(|(_, entries)| entries.iter())
                .any(|(binding, label)| {
                    label.contains("move worktree to workspace") && binding != "unset"
                });
        assert!(
            listed,
            "the move_worktree binding must appear in keybind help with a real \
             binding (src/input/keybind_help.rs); groups were {groups:?}"
        );
    }

    /// Hook: `src/protocol/wire.rs` — `PARENT_WORKSPACE_TOKEN`.
    ///
    /// The generation-1 `ClientShellWorkspace` codec is frozen, so the grouping
    /// link rides in the existing `tokens` field. The name must stay stable and
    /// must not look like a rendered token template, or it becomes visible.
    #[test]
    fn parent_link_token_name_is_stable_and_namespaced() {
        assert_eq!(
            crate::protocol::PARENT_WORKSPACE_TOKEN,
            "herdr:parent",
            "the parent-link token name is a wire value shared by the server \
             projection and the client sidebar; changing it silently disables \
             nesting (src/protocol/wire.rs)"
        );
    }

    /// Hooks: `src/api/mod.rs` (`request_changes_ui`) and
    /// `src/server/client_commands.rs` (`CLIENT_SHELL_METHODS`) for the
    /// acknowledge action.
    ///
    /// Acknowledging changes sidebar-visible status, so without the first the
    /// server never re-renders and a "done" workspace keeps its marker; without
    /// the second the TUI's keybind is rejected outright.
    #[test]
    fn acknowledge_is_registered_for_the_ui_and_the_client_lane() {
        let request = crate::api::schema::Request {
            id: "guard".into(),
            method: crate::api::schema::Method::WorkspaceAcknowledge(
                crate::api::schema::WorkspaceTarget {
                    workspace_id: "w1".into(),
                },
            ),
        };
        assert!(
            crate::api::request_changes_ui(&request),
            "workspace.acknowledge must be listed in request_changes_ui \
             (src/api/mod.rs), or acknowledging never re-renders the sidebar"
        );
        assert!(
            crate::server::client_commands::supports_client_shell_method_name(
                "workspace.acknowledge"
            ),
            "workspace.acknowledge must be in CLIENT_SHELL_METHODS \
             (src/server/client_commands.rs), or the TUI cannot invoke it"
        );
    }

    /// Hooks: the `resolve_non_indexed_action` table plus the `acknowledge`
    /// field in `src/config/{model,keybinds}.rs`, and `keybind_help.rs`.
    #[test]
    fn acknowledge_keybind_resolves_and_is_listed() {
        let config = crate::config::Config::default();
        let (live, _diagnostics) = config
            .live_keybinds_with_diagnostics()
            .expect("default keybinds must be valid");

        for key in [
            crate::input::TerminalKey::new(
                crossterm::event::KeyCode::Char('a'),
                crossterm::event::KeyModifiers::SHIFT,
            ),
            crate::input::TerminalKey::new(
                crossterm::event::KeyCode::Char('A'),
                crossterm::event::KeyModifiers::empty(),
            ),
        ] {
            assert!(
                matches!(
                    crate::input::resolve_prefix_binding(&live.keybinds, &key),
                    Some(crate::input::KeybindMatch::Action(
                        crate::input::KeybindAction::AcknowledgeWorkspace
                    ))
                ),
                "prefix+shift+a must resolve to AcknowledgeWorkspace for {key:?}; \
                 check the resolve_non_indexed_action table and the acknowledge \
                 field in src/config/model.rs + keybinds.rs"
            );
        }

        let groups = crate::input::keybind_help_groups(&live.keybinds, live.prefix);
        assert!(
            groups
                .iter()
                .flat_map(|(_, entries)| entries.iter())
                .any(|(binding, label)| label.contains("acknowledge") && binding != "unset"),
            "the acknowledge binding must appear in keybind help \
             (src/input/keybind_help.rs)"
        );
    }

    // The persistence hook (`WorkspaceSnapshot.parent_workspace_id`) is guarded
    // by `parent_workspace_id_survives_the_snapshot_round_trip` in
    // src/persist/snapshot.rs, which can reach that private module.
}
