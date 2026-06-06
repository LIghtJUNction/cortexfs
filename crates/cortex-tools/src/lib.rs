#![forbid(unsafe_code)]

/// Workspace scope for tool registry, invocation, and tool-loop primitives.
pub const SCOPE: &str = "tools";

/// Shell command execution tool id.
pub const SHELL_EXEC_TOOL: &str = "shell.exec";
/// Filesystem read tool id.
pub const FILESYSTEM_READ_TOOL: &str = "filesystem.read";
/// Unified projection of the local-fs MCP `read_file` tool.
pub const MCP_LOCAL_FS_READ_TOOL: &str = "mcp.local-fs.read_file";

/// Permission required for shell command execution.
pub const HOST_SHELL_EXEC_PERMISSION: &str = "host.shell.exec";
/// Permission required for local filesystem reads.
pub const HOST_FS_READ_PERMISSION: &str = "host.fs.read";
/// Permission required for local-fs MCP `read_file` invocation.
pub const MCP_LOCAL_FS_READ_FILE_PERMISSION: &str = "mcp.local-fs.read_file";

/// Newline-terminated permission text for shell command execution.
pub const HOST_SHELL_EXEC_PERMISSION_TEXT: &str = "host.shell.exec\n";
/// Newline-terminated permission text for local filesystem reads.
pub const HOST_FS_READ_PERMISSION_TEXT: &str = "host.fs.read\n";
/// Newline-terminated permission text for local-fs MCP `read_file` invocation.
pub const MCP_LOCAL_FS_READ_TOOL_PERMISSIONS_TEXT: &str = "mcp.local-fs.read_file\nhost.fs.read\n";

/// Global tools exposed by the FUSE ABI.
pub const GLOBAL_TOOLS: &[&str] = &[
    SHELL_EXEC_TOOL,
    FILESYSTEM_READ_TOOL,
    MCP_LOCAL_FS_READ_TOOL,
];

/// Default tools enabled for the local user space and helper agent.
pub const DEFAULT_ALLOWED_TOOLS: &[&str] = &[FILESYSTEM_READ_TOOL, MCP_LOCAL_FS_READ_TOOL];

#[cfg(test)]
mod tests {
    #[test]
    fn scope_names_design_component() {
        assert_eq!(super::SCOPE, "tools");
    }

    #[test]
    fn default_allowed_tools_exclude_shell_exec() {
        assert_eq!(
            super::DEFAULT_ALLOWED_TOOLS,
            &[super::FILESYSTEM_READ_TOOL, super::MCP_LOCAL_FS_READ_TOOL]
        );
        assert!(!super::DEFAULT_ALLOWED_TOOLS.contains(&super::SHELL_EXEC_TOOL));
    }

    #[test]
    fn permission_texts_are_newline_terminated() {
        assert!(super::HOST_SHELL_EXEC_PERMISSION_TEXT.ends_with('\n'));
        assert!(super::HOST_FS_READ_PERMISSION_TEXT.ends_with('\n'));
        assert!(
            super::MCP_LOCAL_FS_READ_TOOL_PERMISSIONS_TEXT
                .contains(super::MCP_LOCAL_FS_READ_FILE_PERMISSION)
        );
        assert!(
            super::MCP_LOCAL_FS_READ_TOOL_PERMISSIONS_TEXT.contains(super::HOST_FS_READ_PERMISSION)
        );
    }
}
