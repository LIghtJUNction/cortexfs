use crate::{
    LOCAL_USER_THREAD_DISPLAY_PATH, LOCAL_USER_THREAD_DISPLAY_TEXT, RuntimeState, json_string,
    validation::validate_control_write,
};

#[derive(Clone, Copy)]
pub struct McpServerControlEffect<'a> {
    pub server_id: &'a str,
    pub command_name: &'a str,
    pub next_status: &'a str,
    pub next_pid: &'a str,
}

impl RuntimeState {
    pub fn write_simple_control(
        &mut self,
        command_name: &str,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<u32> {
        validate_control_write(offset, data)?;
        self.update_dynamic_file(self.last_control_inode, format!("{command_name}\n"));
        self.append_audit("control", command_name, "control");
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    pub fn write_space_control(
        &mut self,
        audit_format: &str,
        display_prefix: &str,
        command_name: &str,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<u32> {
        validate_control_write(offset, data)?;
        self.update_dynamic_file(
            self.last_control_inode,
            format!("{display_prefix}/{command_name}\n"),
        );
        self.append_audit(audit_format, command_name, "control");
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    pub fn write_thread_control(
        &mut self,
        command_name: &str,
        next_state: &str,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<u32> {
        validate_control_write(offset, data)?;
        if let Some(state_inode) = self.thread_state_inode {
            self.update_dynamic_file(state_inode, format!("{next_state}\n"));
        }
        self.update_dynamic_file(
            self.last_control_inode,
            format!("{LOCAL_USER_THREAD_DISPLAY_PATH}/{command_name}\n"),
        );
        self.append_audit("thread.demo.control", command_name, next_state);
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    pub fn write_tool_loop_control(
        &mut self,
        command_name: &str,
        next_state: &str,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<u32> {
        validate_control_write(offset, data)?;
        if let Some(state_inode) = self.tool_loop_state_inode {
            self.update_dynamic_file(state_inode, format!("{next_state}\n"));
        }
        self.append_tool_loop_control_step(command_name, next_state);
        self.update_dynamic_file(
            self.last_control_inode,
            format!("{LOCAL_USER_THREAD_DISPLAY_PATH}/tool-loop/{command_name}\n"),
        );
        self.append_audit("tool-loop.demo.control", command_name, next_state);
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    pub fn write_tool_loop_limit(
        &mut self,
        inode: fuse3::Inode,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<Option<u32>> {
        if Some(inode) == self.tool_loop_max_steps_inode {
            return self
                .write_tool_loop_limit_value(inode, "max_steps", offset, data, parse_positive_u64)
                .map(Some);
        }
        if Some(inode) == self.tool_loop_max_time_ms_inode {
            return self
                .write_tool_loop_limit_value(inode, "max_time_ms", offset, data, parse_positive_u64)
                .map(Some);
        }
        if Some(inode) == self.tool_loop_max_cost_usd_inode {
            return self
                .write_tool_loop_limit_value(
                    inode,
                    "max_cost_usd",
                    offset,
                    data,
                    parse_non_negative_decimal,
                )
                .map(Some);
        }
        Ok(None)
    }

    fn write_tool_loop_limit_value(
        &mut self,
        inode: fuse3::Inode,
        name: &str,
        offset: u64,
        data: &[u8],
        validate: fn(&str) -> fuse3::Result<()>,
    ) -> fuse3::Result<u32> {
        if offset != 0 {
            return Err(libc::EINVAL.into());
        }
        let value = std::str::from_utf8(data).map_err(|_error| libc::EINVAL)?;
        let value = value.trim();
        validate(value)?;
        self.update_dynamic_file(inode, format!("{value}\n"));
        self.append_audit("tool-loop.demo.limits", name, "configured");
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    pub fn write_agent_control(
        &mut self,
        command_name: &str,
        next_state: &str,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<u32> {
        validate_control_write(offset, data)?;
        if let Some(state_inode) = self.agent_helper_runtime_state_inode {
            self.update_dynamic_file(state_inode, format!("{next_state}\n"));
        }
        if let Some(pid_inode) = self.agent_helper_runtime_pid_inode {
            let pid = if matches!(command_name, "start" | "restart") {
                "1234\n".to_owned()
            } else if command_name == "stop" {
                "\n".to_owned()
            } else {
                self.nodes
                    .get(&pid_inode)
                    .and_then(crate::Node::content)
                    .unwrap_or("\n")
                    .to_owned()
            };
            self.update_dynamic_file(pid_inode, pid);
        }
        if let Some(heartbeat_inode) = self.agent_helper_runtime_heartbeat_inode {
            let heartbeat = if matches!(command_name, "start" | "restart") {
                "1\n".to_owned()
            } else if command_name == "stop" {
                "\n".to_owned()
            } else {
                self.nodes
                    .get(&heartbeat_inode)
                    .and_then(crate::Node::content)
                    .unwrap_or("\n")
                    .to_owned()
            };
            self.update_dynamic_file(heartbeat_inode, heartbeat);
        }
        if matches!(command_name, "start" | "restart")
            && let Some(thread_inode) = self.agent_helper_runtime_current_thread_inode
        {
            self.update_dynamic_file(thread_inode, LOCAL_USER_THREAD_DISPLAY_TEXT);
        }
        if command_name == "stop"
            && let Some(thread_inode) = self.agent_helper_runtime_current_thread_inode
        {
            self.update_dynamic_file(thread_inode, "\n");
        }
        if matches!(command_name, "start" | "stop" | "restart")
            && let Some(task_inode) = self.agent_helper_runtime_current_task_inode
        {
            self.update_dynamic_file(task_inode, "\n");
        }
        self.update_dynamic_file(
            self.last_control_inode,
            format!("agent/helper/{command_name}\n"),
        );
        self.append_audit("agent.helper.control", command_name, next_state);
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    pub fn write_mcp_server_control(
        &mut self,
        effect: McpServerControlEffect<'_>,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<u32> {
        validate_control_write(offset, data)?;
        if let Some(status_inode) = self.mcp_local_fs_status_inode {
            self.update_dynamic_file(status_inode, format!("{}\n", effect.next_status));
        }
        if let Some(pid_inode) = self.mcp_local_fs_pid_inode {
            self.update_dynamic_file(pid_inode, effect.next_pid);
        }
        if let Some(state_inode) = self.mcp_session_state_inode {
            self.update_dynamic_file(state_inode, format!("{}\n", effect.next_status));
        }
        self.append_mcp_session_transcript(format!(
            "{{\"type\":\"server_control\",\"server\":\"{}\",\"command\":\"{}\",\"state\":\"{}\"}}\n",
            effect.server_id, effect.command_name, effect.next_status
        ));
        self.update_dynamic_file(
            self.last_control_inode,
            format!("mcp/server/{}/{}\n", effect.server_id, effect.command_name),
        );
        let audit_format = format!("mcp.server.{}.control", effect.server_id);
        self.append_audit(&audit_format, effect.command_name, effect.next_status);
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    pub fn write_mcp_resource_refresh(&mut self, offset: u64, data: &[u8]) -> fuse3::Result<u32> {
        validate_control_write(offset, data)?;
        if let Some(content_inode) = self.mcp_workspace_content_inode {
            self.update_dynamic_file(
                content_inode,
                "workspace=available\nentries=1\nrefreshed=1\n",
            );
        }
        if let Some(state_inode) = self.mcp_session_state_inode {
            self.update_dynamic_file(state_inode, "refreshed\n");
        }
        self.append_mcp_session_transcript(
            "{\"type\":\"resource_refresh\",\"resource\":\"local-fs/workspace\",\"state\":\"refreshed\"}\n",
        );
        self.update_dynamic_file(
            self.last_control_inode,
            "mcp/resource/local-fs/workspace/refresh\n",
        );
        self.append_audit("mcp.resource.local-fs.workspace", "refresh", "refreshed");
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    pub fn append_mcp_session_transcript(&mut self, line: impl AsRef<str>) {
        let Some(inode) = self.mcp_session_transcript_inode else {
            return;
        };
        let mut transcript = self
            .nodes
            .get(&inode)
            .and_then(crate::Node::content)
            .unwrap_or_default()
            .to_owned();
        transcript.push_str(line.as_ref());
        self.update_mcp_session_summary(&transcript);
        self.update_dynamic_file(inode, transcript);
    }

    fn update_mcp_session_summary(&mut self, transcript: &str) {
        let Some(inode) = self.mcp_session_summary_inode else {
            return;
        };
        let line_count = transcript.lines().count();
        let last_entry = transcript.lines().last().unwrap_or_default();
        self.update_dynamic_file(
            inode,
            format!("lines={line_count}\nlast_entry={last_entry}\n"),
        );
    }

    pub fn write_mcp_session_search(&mut self, offset: u64, data: &[u8]) -> fuse3::Result<u32> {
        if offset != 0 {
            return Err(libc::EINVAL.into());
        }
        let query = std::str::from_utf8(data).map_err(|_error| libc::EINVAL)?;
        let query = query.trim();
        if let Some(inode) = self.mcp_session_search_query_inode {
            self.update_dynamic_file(inode, format!("{query}\n"));
        }
        if let Some(inode) = self.mcp_session_search_results_inode {
            self.update_dynamic_file(inode, self.mcp_session_search_results(query));
        }
        self.update_dynamic_file(
            self.last_control_inode,
            "mcp/session/local-fs.demo/search/query\n",
        );
        self.append_audit("mcp.session.search", "query", "searched");
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    fn mcp_session_search_results(&self, query: &str) -> String {
        use std::fmt::Write as _;

        if query.is_empty() {
            return String::new();
        }
        let Some(transcript) = self
            .mcp_session_transcript_inode
            .and_then(|inode| self.nodes.get(&inode))
            .and_then(crate::Node::content)
        else {
            return String::new();
        };

        let mut results = String::new();
        for (index, line) in transcript.lines().enumerate() {
            if !line.contains(query) {
                continue;
            }
            let _ = writeln!(
                results,
                "{{\"source\":\"mcp/session/local-fs.demo/transcript.jsonl\",\"session\":\"mcp/session/local-fs.demo\",\"line\":{},\"score\":1.0,\"text\":{}}}",
                index.saturating_add(1),
                json_string(line),
            );
        }
        results
    }

    pub fn write_cluster_control(
        &mut self,
        command_name: &str,
        next_state: &str,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<u32> {
        validate_control_write(offset, data)?;
        if let Some(state_inode) = self.cluster_state_inode {
            self.update_dynamic_file(state_inode, format!("{next_state}\n"));
        }
        self.update_dynamic_file(
            self.last_control_inode,
            format!("cluster/local/{command_name}\n"),
        );
        self.append_audit("cluster.local.control", command_name, next_state);
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }
}

fn parse_positive_u64(value: &str) -> fuse3::Result<()> {
    if value.parse::<u64>().is_ok_and(|number| number > 0) {
        Ok(())
    } else {
        Err(libc::EINVAL.into())
    }
}

fn parse_non_negative_decimal(value: &str) -> fuse3::Result<()> {
    if value.is_empty() {
        return Err(libc::EINVAL.into());
    }
    let mut parts = value.split('.');
    let Some(whole) = parts.next() else {
        return Err(libc::EINVAL.into());
    };
    if whole.is_empty() || !whole.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(libc::EINVAL.into());
    }
    if let Some(fractional) = parts.next()
        && (fractional.is_empty() || !fractional.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(libc::EINVAL.into());
    }
    if parts.next().is_some() {
        return Err(libc::EINVAL.into());
    }
    Ok(())
}
