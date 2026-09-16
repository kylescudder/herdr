//! Left-edge session indicators for the sidebar workspace list.
//!
//! Fork-carried behaviour kept in its own module so an upstream merge cannot
//! conflict with it. The only hook is the pair of calls in
//! `sidebar.rs::render_sidebar`; `sidebar_marks_the_active_and_hovered_rows_at_the_left_edge`
//! guards that they still happen. See FORK.md.

use ratatui::{buffer::Buffer, layout::Rect, style::Color};

use crate::app::state::Palette;

/// Arrow marking the hovered / navigate-selected row.
const HOVERED: &str = "\u{276f}";
/// Bar spanning the active workspace.
const ACTIVE: &str = "\u{258e}";

/// Background for the hovered row.
///
/// A theme that leaves `selection_bg` unset renders the hovered row as a faint
/// tint or nothing at all, so fall back to the brightest surface (what drag
/// uses) to keep it unmistakable, rather than overriding a theme that sets one.
pub(super) fn hovered_row_background(palette: &Palette) -> Color {
    if palette.selection_bg == Color::Reset {
        palette.surface1
    } else {
        palette.selection_bg
    }
}

/// Draws the left-edge markers for one workspace row.
///
/// Must be called *after* the row text is rendered, or the text overwrites the
/// markers: they occupy the row's first cell, which the row template leaves
/// blank. The bar spans every line of a multi-line row; the arrow marks only
/// the first, and wins on a row that is both active and hovered.
pub(super) fn draw_row_markers(
    buffer: &mut Buffer,
    rect: Rect,
    row_height: u16,
    list_bottom: u16,
    selected: bool,
    active: bool,
    palette: &Palette,
) {
    for row_index in 0..row_height {
        let y = rect.y.saturating_add(row_index);
        if y >= list_bottom {
            break;
        }
        if selected && row_index == 0 {
            buffer[(rect.x, y)]
                .set_symbol(HOVERED)
                .set_fg(palette.accent);
        } else if active {
            buffer[(rect.x, y)]
                .set_symbol(ACTIVE)
                .set_fg(palette.accent);
        }
    }
}
