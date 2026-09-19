pub mod atomic;
pub mod bwrap;
pub mod columnar;
pub mod command;
pub mod control;
pub mod index;
pub mod jsonl;
pub mod layout;
pub mod manuals;
pub mod message;
pub mod plain;
pub mod proc;
pub mod process;
pub mod queue;
pub mod quota;
pub mod receipt;
pub mod schema;
pub mod session;
pub mod stream;
pub mod terminal;
pub mod toolpath;
pub mod trajectory;

pub use trajectory::*;

pub(crate) fn unix_timestamp_text() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    format!("{seconds}\n")
}
