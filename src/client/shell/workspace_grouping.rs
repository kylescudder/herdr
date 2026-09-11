//! Workspace grouping for the sidebar: which rows are top level, and which
//! nest beneath them.
//!
//! Fork-carried behaviour, kept in its own module so upstream merges do not
//! conflict with it. An explicit `parent_workspace_id` (carried to the client
//! as the `herdr:parent` token) wins over git-key grouping, so two workspaces
//! of one monorepo stay separate top-level rows. This is the single source of
//! truth shared by the sidebar, keyboard reorder and mouse drag. See FORK.md.

use super::*;

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
pub(crate) fn workspace_group_layout(
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

/// A workspace's explicitly chosen parent workspace id, carried invisibly in the
/// snapshot tokens (see `protocol::PARENT_WORKSPACE_TOKEN`).
/// The collapse-group key for a top-level parent that has children — its own
/// `workspace_id`. Non-parent or nested workspaces return `None`.
pub(crate) fn parent_group_key(snapshot: &ClientShellSnapshot, index: usize) -> Option<String> {
    let (_, children_of) = workspace_group_layout(snapshot);
    if !children_of.contains_key(&index) {
        return None;
    }
    snapshot
        .workspaces
        .get(index)
        .map(|workspace| workspace.workspace_id.clone())
}
