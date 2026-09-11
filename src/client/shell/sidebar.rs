use super::*;
use ratatui::{
    text::Line,
    widgets::{Paragraph, Widget},
};

fn collapsed_sidebar_sections(area: Rect) -> (Rect, Option<u16>, Rect) {
    let content = Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height);
    if content.is_empty() {
        return (Rect::default(), None, Rect::default());
    }
    if content.height < 7 {
        return (content, None, Rect::default());
    }
    let workspace_height = content.height.div_ceil(2);
    let divider_y = content.y + workspace_height;
    let detail_height = content.height.saturating_sub(workspace_height + 1);
    (
        Rect::new(content.x, content.y, content.width, workspace_height),
        Some(divider_y),
        Rect::new(content.x, divider_y + 1, content.width, detail_height),
    )
}

pub(crate) fn render_collapsed_sidebar(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    selected_workspace_id: Option<&str>,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    render_sidebar_background(buffer, area, palette);
    let (workspace_area, divider_y, detail_area) = collapsed_sidebar_sections(area);
    for (index, workspace) in snapshot
        .workspaces
        .iter()
        .take(workspace_area.height as usize)
        .enumerate()
    {
        let rect = Rect::new(
            workspace_area.x,
            workspace_area.y + index as u16,
            workspace_area.width,
            1,
        );
        let selected = selected_workspace_id == Some(workspace.workspace_id.as_str());
        let selection_background =
            if workspace.focused && palette.selection_bg == ratatui::style::Color::Reset {
                palette.active_row_bg
            } else {
                palette.selection_bg
            };
        if selected {
            buffer.set_style(rect, Style::default().bg(selection_background));
        } else if workspace.focused {
            buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
        }
        let number_style = if selected {
            Style::default()
                .fg(palette.overlay1)
                .bg(selection_background)
        } else if workspace.focused {
            Style::default().fg(palette.text).bg(palette.active_row_bg)
        } else {
            Style::default().fg(palette.overlay0)
        };
        put_text(
            buffer,
            rect.x,
            rect.y,
            rect.width.min(2),
            &format!("{:<2}", index + 1),
            number_style,
        );
        let status = workspace.agent_status;
        put_text(
            buffer,
            rect.x.saturating_add(2),
            rect.y,
            rect.width.saturating_sub(2),
            status_icon(status, config.status_indicators),
            Style::default().fg(status_color(status, palette)),
        );
        hits.workspaces.push(WorkspaceHit {
            rect,
            workspace_id: workspace.workspace_id.clone(),
            indented: false,
            group_toggle: None,
        });
    }

    if let Some(divider_y) = divider_y {
        put_text(
            buffer,
            workspace_area.x,
            divider_y,
            workspace_area.width,
            &"─".repeat(workspace_area.width as usize),
            Style::default().fg(palette.surface_dim),
        );
    }

    let detail_content = Rect::new(
        detail_area.x,
        detail_area.y,
        detail_area.width,
        detail_area.height.saturating_sub(1),
    );
    for (index, pane_id) in super::ordered_agent_pane_ids(snapshot, config.agent_panel_sort)
        .into_iter()
        .take(detail_content.height as usize)
        .enumerate()
    {
        let Some(agent) = snapshot
            .agents
            .iter()
            .find(|agent| agent.pane_id == pane_id)
        else {
            continue;
        };
        let rect = Rect::new(
            detail_content.x,
            detail_content.y + index as u16,
            detail_content.width,
            1,
        );
        if agent.focused {
            buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
        }
        put_text(
            buffer,
            rect.x,
            rect.y,
            rect.width.min(2),
            &format!("{:<2}", index + 1),
            Style::default().fg(if agent.focused {
                palette.text
            } else {
                palette.overlay0
            }),
        );
        put_text(
            buffer,
            rect.x.saturating_add(2),
            rect.y,
            rect.width.saturating_sub(2),
            status_icon(agent.agent_status, config.status_indicators),
            Style::default().fg(status_color(agent.agent_status, palette)),
        );
        hits.agents.push((rect, pane_id));
    }
    hits.sidebar_toggle = if area.is_empty() || workspace_area.width == 0 {
        Rect::default()
    } else {
        Rect::new(
            workspace_area.x + workspace_area.width / 2,
            area.bottom().saturating_sub(1),
            1,
            1,
        )
    };
    put_text(
        buffer,
        hits.sidebar_toggle.x,
        hits.sidebar_toggle.y,
        hits.sidebar_toggle.width,
        "»",
        if super::super::global_menu::global_menu_attention(snapshot) {
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette.overlay0)
        },
    );
}

pub(crate) fn render_sidebar(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    state: &mut ShellRenderState<'_>,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    render_sidebar_background(buffer, area, palette);
    hits.sidebar_divider = if area.is_empty() {
        Rect::default()
    } else {
        Rect::new(area.right().saturating_sub(1), area.y, 1, area.height)
    };
    let (workspace_area, detail_area) =
        crate::ui::expanded_sidebar_sections(area, state.sidebar_section_split);
    hits.sidebar_section_divider =
        crate::ui::sidebar_section_divider_rect(area, state.sidebar_section_split);
    put_text(
        buffer,
        workspace_area.x,
        workspace_area.y,
        workspace_area.width,
        " spaces",
        Style::default()
            .fg(palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );

    let entries = workspace_entries(snapshot, state.collapsed_groups);
    let body = Rect::new(
        workspace_area.x,
        workspace_area.y.saturating_add(WORKSPACE_HEADER_ROWS),
        workspace_area.width,
        workspace_area
            .height
            .saturating_sub(WORKSPACE_HEADER_ROWS + 1),
    );
    hits.workspace_body = body;
    let row_heights = entries
        .iter()
        .map(|entry| {
            snapshot
                .workspaces
                .get(entry.index)
                .map(|workspace| {
                    workspace_rows(
                        workspace,
                        displayed_workspace_status(snapshot, workspace, state.collapsed_groups),
                        entry.indented,
                        &config.spaces,
                    )
                    .len()
                    .max(1)
                    .min(u16::MAX as usize) as u16
                })
                .unwrap_or(1)
        })
        .collect::<Vec<_>>();
    let gaps = entries
        .iter()
        .enumerate()
        .map(|(index, _)| {
            entries
                .get(index + 1)
                .map_or(0, |next| u16::from(!next.indented) * config.spaces.row_gap)
        })
        .collect::<Vec<_>>();
    let mut metrics = super::scroll::list_scroll_metrics(
        &row_heights,
        &gaps,
        body.height,
        *state.workspace_scroll,
    );
    if !body.is_empty() && std::mem::take(state.reveal_focused_workspace) {
        if let Some(target) = entries
            .iter()
            .position(|entry| snapshot.workspaces[entry.index].focused)
        {
            *state.workspace_scroll = super::scroll::list_scroll_start_to_reveal(
                &row_heights,
                &gaps,
                body.height,
                *state.workspace_scroll,
                target,
            );
            metrics = super::scroll::list_scroll_metrics(
                &row_heights,
                &gaps,
                body.height,
                *state.workspace_scroll,
            );
        }
    }
    hits.workspace_max_scroll = metrics.max_offset_from_bottom;
    hits.workspace_scroll_metrics = Some(metrics);
    *state.workspace_scroll = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom);
    let show_scrollbar = metrics.max_offset_from_bottom > 0 && body.width > 1;
    let content_width = body.width.saturating_sub(u16::from(show_scrollbar));
    let mut y = body.y;
    for (entry_position, entry) in entries.iter().enumerate().skip(*state.workspace_scroll) {
        let Some(workspace) = snapshot.workspaces.get(entry.index) else {
            continue;
        };
        let status = displayed_workspace_status(snapshot, workspace, state.collapsed_groups);
        let rows = workspace_rows(workspace, status, entry.indented, &config.spaces);
        let row_height = (rows.len().max(1).min(u16::MAX as usize) as u16).min(body.height);
        if y.saturating_add(row_height) > body.bottom() {
            break;
        }
        let rect = Rect::new(body.x, y, content_width, row_height);
        let selected = state.selected_workspace_id == Some(workspace.workspace_id.as_str());
        let dragged = state.dragged_workspace_id == Some(workspace.workspace_id.as_str());
        if selected {
            buffer.set_style(rect, Style::default().bg(palette.selection_bg));
        } else if dragged {
            buffer.set_style(rect, Style::default().bg(palette.surface1));
        } else if workspace.focused {
            buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
        }
        render_workspace_rows(
            buffer,
            rect,
            workspace,
            status,
            config.status_indicators,
            entry,
            rows,
            selected,
            dragged,
            palette,
        );
        let group_toggle = parent_group_key(snapshot, entry.index).map(|key| {
            let rect = Rect::new(rect.right().saturating_sub(1), rect.y, 1, 1);
            put_text(
                buffer,
                rect.x,
                rect.y,
                rect.width,
                if state.collapsed_groups.contains(&key) {
                    "▸"
                } else {
                    "▾"
                },
                Style::default().fg(palette.accent),
            );
            (rect, key)
        });
        hits.workspaces.push(WorkspaceHit {
            rect,
            workspace_id: workspace.workspace_id.clone(),
            indented: entry.indented,
            group_toggle,
        });
        let gap = entries
            .get(entry_position + 1)
            .map_or(0, |next| u16::from(!next.indented) * config.spaces.row_gap);
        y = y.saturating_add(row_height + gap);
    }

    if show_scrollbar {
        let track = Rect::new(body.right().saturating_sub(1), body.y, 1, body.height);
        hits.workspace_scrollbar = track;
        super::scroll::render_list_scrollbar(buffer, track, metrics, palette);
    }

    if let Some(row) = state.workspace_drop_indicator_row.filter(|row| {
        *row >= workspace_area.y.saturating_add(1)
            && *row < workspace_area.bottom().saturating_sub(1)
    }) {
        put_text(
            buffer,
            body.x,
            row,
            body.width,
            &"─".repeat(body.width as usize),
            Style::default().fg(palette.accent),
        );
    }

    let footer_y = workspace_area.bottom().saturating_sub(1);
    if config.mouse_capture {
        hits.new_workspace = Rect::new(
            workspace_area.x,
            footer_y,
            5.min(workspace_area.width),
            u16::from(workspace_area.height > 0),
        );
        put_text(
            buffer,
            workspace_area.x,
            footer_y,
            workspace_area.width,
            " new",
            Style::default().fg(palette.overlay0),
        );
        let attention = super::super::global_menu::global_menu_attention(snapshot);
        let launcher_width = if attention { 8 } else { 6 }.min(workspace_area.width);
        hits.global_launcher = Rect::new(
            workspace_area.right().saturating_sub(launcher_width),
            footer_y,
            launcher_width,
            1,
        );
        if attention {
            let start_x = workspace_area.right().saturating_sub(6);
            put_text(
                buffer,
                start_x,
                footer_y,
                2,
                "● ",
                Style::default()
                    .fg(palette.accent)
                    .add_modifier(Modifier::BOLD),
            );
            put_text(
                buffer,
                start_x.saturating_add(2),
                footer_y,
                4,
                "menu",
                Style::default().fg(palette.overlay0),
            );
        } else {
            put_right_text(
                buffer,
                workspace_area,
                footer_y,
                "menu",
                Style::default().fg(palette.overlay0),
            );
        }
    }

    super::render_agent_panel(
        buffer,
        detail_area,
        snapshot,
        config,
        state.agent_scroll,
        hits,
    );

    hits.sidebar_toggle = Rect::new(
        area.right().saturating_sub(2),
        area.bottom().saturating_sub(1),
        u16::from(area.width > 1),
        u16::from(area.height > 0),
    );
    put_text(
        buffer,
        hits.sidebar_toggle.x,
        hits.sidebar_toggle.y,
        hits.sidebar_toggle.width,
        "«",
        Style::default().fg(palette.overlay0),
    );
}

pub(crate) fn render_sidebar_background(buffer: &mut Buffer, area: Rect, palette: &Palette) {
    buffer.set_style(area, Style::default().bg(palette.sidebar_bg));
    let separator_x = area.right().saturating_sub(1);
    for y in area.y..area.bottom() {
        if let Some(cell) = buffer.cell_mut((separator_x, y)) {
            cell.set_symbol("│");
            cell.set_style(Style::default().fg(palette.surface_dim));
        }
    }
}

/// A workspace's explicitly chosen parent workspace id, carried invisibly in the
/// snapshot tokens (see `protocol::PARENT_WORKSPACE_TOKEN`).
fn explicit_parent_id(workspace: &ClientShellWorkspace) -> Option<&str> {
    workspace
        .tokens
        .iter()
        .find(|(name, _)| name == crate::protocol::PARENT_WORKSPACE_TOKEN)
        .map(|(_, value)| value.as_str())
}

pub(crate) fn workspace_entries(
    snapshot: &ClientShellSnapshot,
    collapsed_groups: &HashSet<String>,
) -> Vec<WorkspaceEntry> {
    let (parent_index, children_of) = workspace_group_layout(snapshot);
    let mut entries = Vec::new();
    for (index, parent) in parent_index.iter().enumerate() {
        // Children are emitted beneath their parent below, not at top level.
        if parent.is_some() {
            continue;
        }
        entries.push(WorkspaceEntry {
            index,
            indented: false,
            last_child: false,
        });
        let Some(children) = children_of.get(&index) else {
            continue;
        };
        if collapsed_groups.contains(&snapshot.workspaces[index].workspace_id) {
            if let Some(active) = children
                .iter()
                .copied()
                .find(|child| snapshot.workspaces[*child].focused)
            {
                entries.push(WorkspaceEntry {
                    index: active,
                    indented: true,
                    last_child: true,
                });
            }
            continue;
        }
        for (child_index, child) in children.iter().enumerate() {
            entries.push(WorkspaceEntry {
                index: *child,
                indented: true,
                last_child: child_index + 1 == children.len(),
            });
        }
    }
    entries
}

/// Hybrid grouping. An explicit `parent_workspace_id` (carried in tokens) wins,
/// so a worktree can nest under a chosen top-level workspace (monorepo-friendly).
/// Otherwise a linked git worktree nests under its repo's primary checkout. Two
/// non-linked workspaces that merely share a repo stay separate top-level
/// entries. Returns each workspace's parent index (None = top level) and, per
/// top-level parent, its ordered child indices. Nesting is capped at two levels;
/// a group is identified by its parent's `workspace_id`.
fn workspace_group_layout(
    snapshot: &ClientShellSnapshot,
) -> (
    Vec<Option<usize>>,
    std::collections::BTreeMap<usize, Vec<usize>>,
) {
    let workspaces = &snapshot.workspaces;
    let id_to_index: HashMap<&str, usize> = workspaces
        .iter()
        .enumerate()
        .map(|(index, workspace)| (workspace.workspace_id.as_str(), index))
        .collect();

    // Linked-worktree primaries: the first non-linked workspace of a repo key
    // that has at least one linked worktree.
    let mut keys_with_linked = HashSet::<&str>::new();
    for workspace in workspaces {
        if let Some(worktree) = &workspace.worktree {
            if worktree.is_linked_worktree {
                keys_with_linked.insert(worktree.key.as_str());
            }
        }
    }
    let mut primary_of_key = HashMap::<&str, usize>::new();
    for (index, workspace) in workspaces.iter().enumerate() {
        if let Some(worktree) = &workspace.worktree {
            if !worktree.is_linked_worktree && keys_with_linked.contains(worktree.key.as_str()) {
                primary_of_key.entry(worktree.key.as_str()).or_insert(index);
            }
        }
    }

    // Direct parent per workspace: explicit wins, else the linked primary.
    let mut direct_parent = vec![None; workspaces.len()];
    for (index, workspace) in workspaces.iter().enumerate() {
        if let Some(parent_id) = explicit_parent_id(workspace) {
            if let Some(&parent) = id_to_index.get(parent_id) {
                if parent != index {
                    direct_parent[index] = Some(parent);
                    continue;
                }
            }
        }
        if let Some(worktree) = &workspace.worktree {
            if worktree.is_linked_worktree {
                if let Some(&parent) = primary_of_key.get(worktree.key.as_str()) {
                    if parent != index {
                        direct_parent[index] = Some(parent);
                    }
                }
            }
        }
    }

    // Flatten to two levels: resolve each parent to its top-most ancestor.
    let top_ancestor = |start: usize| -> usize {
        let mut node = start;
        for _ in 0..workspaces.len() {
            match direct_parent[node] {
                Some(parent) => node = parent,
                None => break,
            }
        }
        node
    };
    let mut parent_index = vec![None; workspaces.len()];
    let mut children_of = std::collections::BTreeMap::<usize, Vec<usize>>::new();
    let directs: Vec<Option<usize>> = direct_parent.clone();
    for (index, direct) in directs.into_iter().enumerate() {
        if let Some(direct) = direct {
            let root = top_ancestor(direct);
            if root != index {
                parent_index[index] = Some(root);
                children_of.entry(root).or_default().push(index);
            }
        }
    }
    (parent_index, children_of)
}

/// Ordered `workspace_id`s of the top-level sidebar rows, i.e. those that are
/// not nested under another workspace. Reordering acts on these, so it must use
/// the same grouping the sidebar renders.
pub(crate) fn top_level_workspace_ids(snapshot: &ClientShellSnapshot) -> Vec<String> {
    let (parent_index, _) = workspace_group_layout(snapshot);
    snapshot
        .workspaces
        .iter()
        .enumerate()
        .filter(|(index, _)| parent_index.get(*index).copied().flatten().is_none())
        .map(|(_, workspace)| workspace.workspace_id.clone())
        .collect()
}

/// The top-level row that `workspace_id` belongs to, plus the whole block that
/// moves with it (the root followed by its children). A nested workspace
/// resolves to its parent, because only top-level rows reorder.
pub(crate) fn workspace_group_block(
    snapshot: &ClientShellSnapshot,
    workspace_id: &str,
) -> Option<(String, Vec<String>)> {
    let (parent_index, children_of) = workspace_group_layout(snapshot);
    let index = snapshot
        .workspaces
        .iter()
        .position(|workspace| workspace.workspace_id == workspace_id)?;
    let root = parent_index.get(index).copied().flatten().unwrap_or(index);
    let root_id = snapshot.workspaces.get(root)?.workspace_id.clone();
    let mut block = vec![root_id.clone()];
    if let Some(children) = children_of.get(&root) {
        block.extend(
            children
                .iter()
                .filter_map(|child| snapshot.workspaces.get(*child))
                .map(|workspace| workspace.workspace_id.clone()),
        );
    }
    Some((root_id, block))
}

/// The collapse-group key (the parent's own `workspace_id`) for the workspace
/// with `workspace_id`, if it is a top-level parent that has children.
pub(crate) fn group_collapse_key(
    snapshot: &ClientShellSnapshot,
    workspace_id: &str,
) -> Option<String> {
    let index = snapshot
        .workspaces
        .iter()
        .position(|workspace| workspace.workspace_id == workspace_id)?;
    parent_group_key(snapshot, index)
}

/// The collapse-group key for a top-level parent that has children — its own
/// `workspace_id`. Non-parent or nested workspaces return `None`.
fn parent_group_key(snapshot: &ClientShellSnapshot, index: usize) -> Option<String> {
    let (_, children_of) = workspace_group_layout(snapshot);
    if !children_of.contains_key(&index) {
        return None;
    }
    snapshot
        .workspaces
        .get(index)
        .map(|workspace| workspace.workspace_id.clone())
}

fn displayed_workspace_status(
    snapshot: &ClientShellSnapshot,
    workspace: &ClientShellWorkspace,
    collapsed_groups: &HashSet<String>,
) -> crate::api::schema::AgentStatus {
    if !collapsed_groups.contains(&workspace.workspace_id) {
        return workspace.agent_status;
    }
    let Some(index) = snapshot
        .workspaces
        .iter()
        .position(|candidate| candidate.workspace_id == workspace.workspace_id)
    else {
        return workspace.agent_status;
    };
    let (_, children_of) = workspace_group_layout(snapshot);
    let Some(children) = children_of.get(&index) else {
        return workspace.agent_status;
    };
    children
        .iter()
        .map(|child| snapshot.workspaces[*child].agent_status)
        .chain(std::iter::once(workspace.agent_status))
        .max_by_key(|status| status_priority(*status))
        .unwrap_or(workspace.agent_status)
}

fn workspace_rows(
    workspace: &ClientShellWorkspace,
    status: crate::api::schema::AgentStatus,
    indented: bool,
    config: &SpacesSidebarConfig,
) -> Vec<Vec<crate::ui::ResolvedToken>> {
    let label = if indented && !workspace.custom_label {
        workspace
            .branch
            .as_deref()
            .and_then(|branch| branch.strip_prefix("worktree/").or(Some(branch)))
            .unwrap_or(&workspace.label)
    } else {
        &workspace.label
    };
    let token_values = workspace.tokens.iter().cloned().collect::<HashMap<_, _>>();
    crate::ui::sidebar_space_rows(
        config,
        crate::ui::SpaceTokenContext {
            workspace: label,
            branch: workspace.branch.as_deref(),
            state_text: status_text(status),
            ahead_behind: workspace.git_ahead_behind,
            tokens: &token_values,
            suppress_git_details: indented,
        },
    )
}

fn render_workspace_rows(
    buffer: &mut Buffer,
    area: Rect,
    workspace: &ClientShellWorkspace,
    status: crate::api::schema::AgentStatus,
    indicators: crate::config::StatusIndicatorStyle,
    entry: &WorkspaceEntry,
    rows: Vec<Vec<crate::ui::ResolvedToken>>,
    selected: bool,
    dragged: bool,
    palette: &Palette,
) {
    for (row_index, row) in rows.iter().enumerate() {
        let y = area.y + row_index as u16;
        if y >= area.bottom() {
            break;
        }
        let mut x = area.x;
        if entry.indented {
            let prefix = if row_index == 0 {
                if entry.last_child {
                    "   └─ "
                } else {
                    "   ├─ "
                }
            } else if entry.last_child {
                "        "
            } else {
                "   │    "
            };
            x = put_segment(
                buffer,
                x,
                y,
                area.right(),
                prefix,
                Style::default().fg(palette.overlay0),
            );
        } else if row_index == 0 {
            x = x.saturating_add(1);
        } else {
            x = x.saturating_add(3);
        }
        let highlighted = workspace.focused || dragged;
        let workspace_style = Style::default()
            .fg(if highlighted {
                palette.text
            } else {
                palette.subtext0
            })
            .add_modifier(if highlighted {
                Modifier::BOLD
            } else {
                Modifier::empty()
            });
        let secondary_style = Style::default().fg(if workspace.focused {
            palette.mauve
        } else {
            palette.overlay0
        });
        let spans = crate::ui::resolved_token_spans(
            row,
            (
                status_icon(status, indicators),
                Style::default().fg(status_color(status, palette)),
            ),
            Style::default()
                .fg(status_color(status, palette))
                .add_modifier(Modifier::DIM),
            workspace_style,
            secondary_style,
            Style::default().fg(palette.overlay1),
            palette,
            area.right().saturating_sub(2).saturating_sub(x) as usize,
        );
        Paragraph::new(Line::from(spans)).render(
            Rect::new(x, y, area.right().saturating_sub(2).saturating_sub(x), 1),
            buffer,
        );
    }

    let background = if selected {
        Some(palette.selection_bg)
    } else if dragged {
        Some(palette.surface1)
    } else if workspace.focused {
        Some(palette.active_row_bg)
    } else {
        None
    };
    if let Some(background) = background {
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                buffer[(x, y)].set_bg(background);
            }
        }
    }
}
