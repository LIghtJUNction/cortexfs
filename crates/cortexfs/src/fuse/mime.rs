use crate::FuseFileType;

/// Returns the stable MIME type for a projected `CortexFS` node.
#[must_use]
pub fn mime_type(path: &str, file_type: FuseFileType) -> &'static str {
    match file_type {
        FuseFileType::Directory => "inode/directory",
        FuseFileType::Socket => "inode/socket",
        FuseFileType::Symlink => "inode/symlink",
        FuseFileType::Other => "application/octet-stream",
        FuseFileType::Regular => regular_mime_type(path),
    }
}

fn regular_mime_type(path: &str) -> &'static str {
    match path.rsplit('/').next().unwrap_or_default() {
        "messages.jsonl" | "events.jsonl" | "raw" | "facts.jsonl" | "decisions.jsonl"
        | "refs.jsonl" | "pack.json" => "application/x-ndjson",
        "meta.json" | "state.json" | "metadata.json" | "schema" => "application/json",
        "latest.md" | "pack.md" | "summary.md" | "todo.md" | "system.md" | "prompt.template.md" => {
            "text/markdown"
        }
        _ => "text/plain",
    }
}
