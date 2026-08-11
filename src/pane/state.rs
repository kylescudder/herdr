use crate::terminal::TerminalId;

/// Viewport state for a pane.
///
/// Terminal identity, cwd, labels, and agent metadata live in TerminalState.
pub struct PaneState {
    pub attached_terminal_id: TerminalId,
    /// Whether the agent in this pane has been addressed since it last finished.
    /// False = "Done": the agent completed and is waiting to be addressed. This
    /// stays false through focusing/reading the pane (it behaves like an inbox
    /// item) and only flips back to true when the agent is next addressed, i.e.
    /// it starts working again.
    pub seen: bool,
}

impl PaneState {
    pub fn new(attached_terminal_id: TerminalId) -> Self {
        Self {
            attached_terminal_id,
            seen: true,
        }
    }
}
