//! Session → ATIF trajectory projection helpers.

#[path = "trajectory/map.rs"]
pub mod map;
#[path = "trajectory/types.rs"]
pub mod types;
#[path = "trajectory/validate.rs"]
pub mod validate;

pub use map::*;
pub use types::*;
pub use validate::*;
