# Fork maintenance

This fork (`kylescudder/herdr`) carries features that upstream does not. This
file exists so re-applying them after an upstream merge takes **minutes**.

## The one rule

**Never re-implement a fork feature from notes or a summary.** Replay the real
commits (merge, cherry-pick, or a patch from `fork/patches/`).

A past sync used "reset `src/` to upstream, then re-port from notes". It lost
the original design — the move-to-workspace **modal picker** came back as a
sidebar-navigation mode, several registration hooks were silently dropped, and
workspace grouping ended up with two conflicting sources of truth. Debugging
that cost hours. The code is the spec; prose is not.

## Why merges hurt, and what to check

Fork logic is mostly self-contained, but it must reach into upstream files at a
few **hooks**: an enum variant, a match arm, a name in a registration list.

Hooks fail **silently**. Nothing stops compiling when a method is missing from
`request_changes_ui` — the feature just quietly stops working. That is the
entire class of bug this file and the guard tests defend against.

## After an upstream merge

```bash
git merge origin/master          # resolve conflicts
just test                        # or the container recipe in the notes below
```

Any dropped hook surfaces as a **named failing test** whose message says which
file to restore it in. Work the failures top to bottom; do not hand-audit.

## Guard tests

`src/fork_contract.rs` is the single place to look. Each test guards one hook
and names its file. Two guards live next to their code because the module they
assert on is private:

| Guard | Hook it protects |
| --- | --- |
| `fork_contract::reparent_is_registered_as_a_ui_changing_request` | `src/api/mod.rs` — `request_changes_ui` |
| `fork_contract::reparent_is_allowed_on_the_client_shell_lane` | `src/server/client_commands.rs` — `CLIENT_SHELL_METHODS` |
| `fork_contract::move_worktree_keybind_resolves_from_the_default_config` | `src/input/keybindings.rs` table + `src/config/{model,keybinds}.rs` |
| `fork_contract::move_worktree_is_listed_in_keybind_help` | `src/input/keybind_help.rs` |
| `fork_contract::parent_link_token_name_is_stable_and_namespaced` | `src/protocol/wire.rs` — `PARENT_WORKSPACE_TOKEN` |
| `persist::snapshot::fork_persistence_contract::parent_workspace_id_survives_the_snapshot_round_trip` | `src/persist/snapshot.rs` field + serde attrs |
| `client::shell::tests::…::reorder_moves_an_explicit_parent_group_as_one_block` | one grouping source of truth across sidebar / reorder / drag |
| `client::shell::tests::…::move_worktree_keybind_opens_the_move_workspace_picker` | keybind opens the modal, not a navigation mode |
| `client::shell::tests::…::shift_m_opens_the_move_picker_from_inside_the_workspace_picker` | dispatch from Navigate mode still opens the modal |
| `client::shell::tests::…::move_workspace_picker_renders_a_modal_listing_targets` | the picker actually renders |
| `server::client_shell::tests::snapshot_carries_the_explicit_parent_link_as_a_public_workspace_id` | projection emits the public id the client matches |
| `config::tests::published_profile_keeps_the_move_worktree_binding` | binding survives the endpoint keybind profile round trip |

Every guard above has been verified to **fail** when its hook is removed. If you
add a fork feature, add a guard for each hook it needs and verify the same way:
delete the hook, watch the named test go red, restore it.

## Carried features

### Explicit workspace grouping + move-to-workspace picker

Lets a workspace be filed under another so related projects nest even when git
cannot infer the relationship (two workspaces of one monorepo). Originally
`f6f7cf40` (plus `16d1c397` for j/k in the picker), rebuilt for the
`src/client/shell/**` architecture.

Design facts that are easy to get wrong:

- It is a **modal overlay**, not a sidebar navigation mode. Upstream's original
  drew it in `ui/dialogs.rs`; the port lives in the client overlay system.
- The parent link rides in `ClientShellWorkspace.tokens` as `herdr:parent`
  because the generation-1 client codec is **frozen** — no new struct fields.
- The token value is the **public** `workspace_id`, which is what the sidebar
  matches against. `Workspace::id` is that same public id.
- `workspace.reparent` is an **appended** `Method` variant; enum order is frozen.
- Sidebar, keyboard reorder and mouse drag must share one grouping source
  (`top_level_workspace_ids` / `workspace_group_block`). When they diverged,
  `shift+j/k` silently did nothing for any nested workspace.

Fork-owned modules (new files, so upstream merges cannot conflict with them):

| File | Holds |
| --- | --- |
| `src/client/shell/workspace_grouping.rs` | the grouping source of truth |
| `src/client/shell/tests/fork_grouping.rs` | grouping, picker and reorder tests |
| `src/fork_contract.rs` | the hook guards |

Everything else the feature needs is a hook listed above.

### Monorepo top-level spaces

Two non-linked workspaces sharing a git-common-dir key must stay **separate
top-level** spaces; upstream's git-key grouping would nest them. Guarded by
`same_repo_non_linked_workspaces_stay_separate_top_level`.

### Keyboard workspace reorder

`shift+j` / `shift+k` inside the workspace picker. Config keys
`keys.move_workspace_previous` / `move_workspace_next`, unset by default.

## Local build and test notes

Native macOS builds cannot run `zig build` on recent SDKs, so `build.rs` honours
`HERDR_SKIP_ZIG=1` against a prebuilt archive. Keep that escape hatch when
merging upstream `build.rs`.

Three tests fail in the Linux dev container for environmental reasons, not code:

- `live_handoff` binary — devpts
- `inactive_owner_cancels_idle_stream_and_dispatches_close` — concurrency timing
- `unreadable_loose_ref_dir_is_unavailable_not_absent` — container runs as root,
  so `chmod 0o000` is bypassed
- `pty::actor::unix::tests::actor_delays_enter_from_completed_prompt_write` —
  intermittent under full-suite concurrency; passes in isolation

Re-run a suspected flake on its own before believing it. All four pass on CI.

Exclude with:

```
-E 'not binary(live_handoff)
    and not test(inactive_owner_cancels_idle_stream_and_dispatches_close)
    and not test(unreadable_loose_ref_dir_is_unavailable_not_absent)'
```

The maintenance scripts need Python 3.11+ (`tomllib`); macOS system Python 3.9
fails to import them. Run them in the container.
