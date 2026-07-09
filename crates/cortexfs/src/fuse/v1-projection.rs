use crate::*;

#[path = "projection/core.rs"]
pub mod core;
#[path = "projection/model-controls.rs"]
pub mod model_controls;
#[path = "projection/plain-io.rs"]
pub mod plain_io;
#[path = "projection/virtual-model.rs"]
pub mod virtual_model;

pub(crate) use model_controls::*;
pub(crate) use plain_io::*;
