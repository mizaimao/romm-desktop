//! moose-patch: the parts worth testing without a window around them.
//!
//! `model` is the whole of the app's behaviour and has no SDL in it, which is
//! why its tests run anywhere. `rows` is the two lists. `ui` needs a real
//! context and is only the drawing.

pub mod model;
pub mod rows;
pub mod ui;
