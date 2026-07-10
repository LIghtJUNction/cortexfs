// Support modules: single-token stems when possible; legacy multi-word files
// keep explicit #[path] only when forced by existing filenames.

#[path = "support/control-text.rs"]
pub mod control_text;
#[path = "support/host-path.rs"]
pub mod host_path;
#[path = "support/jsonl-line.rs"]
pub mod jsonl_line;
#[path = "support/layout-path.rs"]
pub mod layout_path;
pub mod manuals;
#[path = "support/message-stream.rs"]
pub mod message_stream;
#[path = "support/plain-fs.rs"]
pub mod plain_fs;
#[path = "support/process-helpers.rs"]
pub mod process_helpers;
#[path = "support/session-index.rs"]
pub mod session_index;
#[path = "support/session-layout.rs"]
pub mod session_layout;
#[path = "support/shared-queue.rs"]
pub mod shared_queue;
pub mod stream;
#[path = "support/tool-path.rs"]
pub mod tool_path;
#[path = "support/tool-schema.rs"]
pub mod tool_schema;
#[path = "support/trajectory.rs"]
pub mod trajectory;

pub use trajectory::*;
