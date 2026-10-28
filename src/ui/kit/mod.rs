//! The component kit: the interactive and structural pieces every screen is
//! built from. Each component reads its colors from the palette and its
//! sizes, type, depth and motion from `ui::theme`, so a screen never styles
//! a button, card or menu by hand.
//!
//! Smaller presentational helpers (pills, avatars, selectable text) live in
//! `ui::components`. See DESIGN.md for when to use what.

mod button;
mod card;
mod dropdown;
pub mod interact;
mod list_row;
mod overlay;
mod segmented;
mod sort_header;
mod stat;
mod stat_strip;

pub use button::{Button, ButtonKind, ButtonSize};
pub use card::Card;
pub use dropdown::{Dropdown, DropdownItem};
pub use list_row::ListRow;
pub use overlay::{Overlay, OverlayAlign};
pub use segmented::Segmented;
pub use sort_header::{Sort, SortHeader};
pub use stat::Stat;
pub use stat_strip::StatStrip;

use std::rc::Rc;

use gpui_kit::*;

/// What every clickable component calls. The same shape as a GPUI click
/// listener, so `cx.listener(...)` plugs straight in.
pub type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
