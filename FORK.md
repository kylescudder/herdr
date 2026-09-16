# Fork maintenance

This fork (`kylescudder/herdr`) carries features that upstream does not. This
file exists so re-applying them after an upstream merge takes **minutes**.

## The one rule

**Never re-implement a fork feature from notes or a summary.** Replay the real
commits: merge, or `git cherry-pick` them onto the new upstream. If upstream has
rewritten a file so heavily that a commit cannot apply, export it first
(`git format-patch`) and port it hunk by hunk against the real diff — never from
a description of what it used to do.

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
| `fork_contract::acknowledge_is_registered_for_the_ui_and_the_client_lane` | `request_changes_ui` + `CLIENT_SHELL_METHODS` for acknowledge |
| `fork_contract::acknowledge_keybind_resolves_and_is_listed` | acknowledge keybind table, config fields, keybind help |
| `persist::snapshot::fork_persistence_contract::parent_workspace_id_survives_the_snapshot_round_trip` | `src/persist/snapshot.rs` field + serde attrs |
| `client::shell::tests::…::reorder_moves_an_explicit_parent_group_as_one_block` | one grouping source of truth across sidebar / reorder / drag |
| `client::shell::tests::…::move_worktree_keybind_opens_the_move_workspace_picker` | keybind opens the modal, not a navigation mode |
| `client::shell::tests::…::shift_m_opens_the_move_picker_from_inside_the_workspace_picker` | dispatch from Navigate mode still opens the modal |
| `client::shell::tests::…::move_workspace_picker_renders_a_modal_listing_targets` | the picker actually renders |
| `client::shell::tests::…::sidebar_marks_the_active_and_hovered_rows_at_the_left_edge` | left-edge session indicators in the sidebar |
| `server::client_shell::tests::snapshot_carries_the_explicit_parent_link_as_a_public_workspace_id` | projection emits the public id the client matches |
| `config::tests::published_profile_keeps_the_move_worktree_binding` | binding survives the endpoint keybind profile round trip |

### Hooks with no test guard

`docs/versions/manifest.json` pins each published version to the commit its docs
were seeded from. Upstream ships its own manifest, so **every upstream merge
re-points these at upstream's release commits** and `node scripts/docs/versions.mjs
check` fails with "tag vX moved from A to B". Repoint the affected entries at the
fork's own tags (`git rev-parse vX^{commit}`); `215fe66f` did this for 0.8.0 and
the next merge clobbered 0.8.2 the same way.

Translated docs (`docs/next/website/src/content/docs/{ja,zh-cn}/`) must keep the
same **heading outline** as the English page. Adding an `##` section without
translating it fails `scripts/docs_translation_parity.py`.

Neither is caught by `cargo nextest`. Run the validate job's commands before
pushing:

```bash
python3 scripts/agent_detection_manifest_check.py --require-published
python3 scripts/config_reference_check.py
python3 scripts/docs_translation_parity.py --docs-root docs/next/website/src/content/docs
node scripts/docs/versions.mjs check
node scripts/docs/preview.mjs check
```

Every guard above has been verified to **fail** when its hook is removed. If you
add a fork feature, add a guard for each hook it needs and verify the same way:
delete the hook, watch the named test go red, restore it.

## Fork-owned modules

New files, so an upstream merge cannot conflict with them. Fork logic lives
here; upstream files keep only the hooks.

| File | Holds |
| --- | --- |
| `src/fork_contract.rs` | the hook guards |
| `src/client/shell/workspace_grouping.rs` | the grouping source of truth |
| `src/client/shell/overlays/move_workspace.rs` | the move-to-workspace picker: state, keys, submit, render |
| `src/client/shell/fork_actions.rs` | the acknowledge and move-picker keybind actions, and keyboard reorder |
| `src/client/shell/sidebar_indicators.rs` | the left-edge active/hovered row markers |
| `src/app/api/fork_workspace_grouping.rs` | server-side `workspace.reparent` and `workspace.acknowledge` |
| `src/app/fork_done_markers.rs` | acknowledgement and orphaned-child repair |
| `src/client/shell/tests/fork_grouping.rs` | grouping, picker, reorder and indicator tests |

`move_workspace.rs` is declared inside `overlays.rs` rather than `shell.rs` so
`use super::*` reaches that file's private drawing helpers (`popup`, `panel`,
`put_text`, `button`, `row`, `contrast`) without widening six upstream
signatures. Keep it there; the alternative reintroduces six conflict points.

What deliberately stays in upstream files, and cannot be extracted:

- **Hooks** — enum variants, match arms, registration lists, keybind and config
  tables. Each is guarded by a named test; see the table above.
- **Deletions of upstream code the fork replaced.** `sidebar.rs` loses
  `workspace_entries` / `parent_group_key` and `mouse.rs` rewrites
  `workspace_move_method`, because grouping must have exactly **one** source of
  truth. Leaving upstream's version in place as dead code is the bug that broke
  `shift+j/k` for every nested workspace. `tests/agents_worktrees_notifications.rs`
  loses two tests that assert the pre-fork grouping; replacements of the same
  name live in `tests/fork_grouping.rs`.
- **In-place semantic changes** to upstream functions, such as the completion
  path in `src/app/actions.rs`.

Extraction cut the fork's footprint in upstream files from 1911 lines to 1046,
of which 167 are those deletions.

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

### Sticky done markers + acknowledge

A finished agent's "done" marker is an inbox item, not a notification: focusing
a pane, switching tab or workspace, and refocusing the OS window do **not**
clear it. A pane becomes seen only when its agent is addressed again, or when
the workspace is explicitly acknowledged with `prefix+shift+a`
(`workspace.acknowledge`, also `herdr workspace acknowledge <id>`).

Originally `07d0a24e` (sticky markers) and `5219269b` (acknowledge). Both were
reverted by the v0.9.0 sync, which reset `src/` to upstream — the commits stayed
in history while their code did not, so `git log` showed the feature present
when it was gone. Restored by porting the real diffs.

Easy to get wrong when re-porting:

- Four paths cleared markers on focus; `Workspace::switch_tab` had its own
  seen-marking loop separate from the `mark_active_tab_seen` call sites.
- A completion must set `seen = false` unconditionally. Upstream's version sets
  it to `suppress_active_tab_notifications`, which silently restores the old
  behaviour for a focused pane.
- `acknowledge_workspace` builds `PaneStateUpdate` with
  `suppress_completion: true` — it only flips `seen`, and must not fire a done
  notification.

### Sidebar session indicators

Left-edge markers in the workspace list: an accent bar (`▎`) spans the
active workspace and an arrow (`❯`) marks the hovered/navigate-selected
row. The arrow wins on a row that is both. Originally `72ea88e3`, lost in the
v0.9.0 sync because it lived in `src/ui/sidebar.rs`, which upstream deleted.

Easy to get wrong when re-porting:

- The markers must be drawn **after** the row text, or the text overwrites
  them. They occupy the row's first cell, which the row template leaves blank.
- The bar spans every line of a multi-line row; the arrow is only on the first.
- The original also brightened the hovered row, which upstream now themes via
  `selection_bg`. That palette entry can be `Color::Reset`, leaving the hovered
  row invisible, so the port falls back to `surface1` only in that case rather
  than overriding a theme that sets a colour.

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

These tests fail in the Linux dev container for environmental reasons, not code:

- `live_handoff` binary — devpts
- `inactive_owner_cancels_idle_stream_and_dispatches_close` — concurrency timing
- `unreadable_loose_ref_dir_is_unavailable_not_absent` — container runs as root,
  so `chmod 0o000` is bypassed
Re-run a suspected flake on its own before believing it, but do not assume a
flake is environmental: `actor_delays_enter_from_completed_prompt_write` looked
like container noise and turned out to be a real race in the test, which also
failed on CI. It asserted that a closed PTY always reports a write error, while
the actor may instead drop the completion channel during shutdown. Fixed by
accepting either outcome.

Exclude with:

```
-E 'not binary(live_handoff)
    and not test(inactive_owner_cancels_idle_stream_and_dispatches_close)
    and not test(unreadable_loose_ref_dir_is_unavailable_not_absent)'
```

```
```

The maintenance scripts need Python 3.11+ (`tomllib`); macOS system Python 3.9
fails to import them. Run them in the container.
