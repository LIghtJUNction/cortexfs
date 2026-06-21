use crate::runtime_types::{PendingResponse, ThreadUpdate};
use crate::text::{
    assistant_content, external_display_name, external_subject, json_string, user_content,
};
use crate::{
    LOCAL_THREAD_SOCKET_PATH, LOCAL_USER_MEMORY_SCOPE_TEXT, LOCAL_USER_THREAD_CONTEXT_TEXT,
    LOCAL_USER_THREAD_DISPLAY_PATH, Node, NodeContent, RuntimeState,
};
use cortex_core::ThreadId;
use std::time::{SystemTime, UNIX_EPOCH};

impl RuntimeState {
    pub fn update_thread_files(&mut self, update: ThreadUpdate<'_>) {
        match update {
            ThreadUpdate::Queued(fingerprint) => {
                if let Some(inode) = self.thread_state_inode {
                    self.update_dynamic_file(inode, "queued\n");
                }
                if let Some(inode) = self.thread_fingerprint_inode {
                    self.update_dynamic_file(inode, format!("{fingerprint}\n"));
                }
            }
            ThreadUpdate::Drained(fingerprint) => {
                if let Some(inode) = self.thread_state_inode {
                    self.update_dynamic_file(inode, "idle\n");
                }
                if let Some(inode) = self.thread_fingerprint_inode {
                    self.update_dynamic_file(inode, format!("{fingerprint}\n"));
                }
            }
        }
    }

    pub fn update_external_thread_files(&mut self, update: ThreadUpdate<'_>) {
        match update {
            ThreadUpdate::Queued(fingerprint) => {
                if let Some(inode) = self.external_thread_state_inode {
                    self.update_dynamic_file(inode, "queued\n");
                }
                if let Some(inode) = self.external_thread_fingerprint_inode {
                    self.update_dynamic_file(inode, format!("{fingerprint}\n"));
                }
            }
            ThreadUpdate::Drained(fingerprint) => {
                if let Some(inode) = self.external_thread_state_inode {
                    self.update_dynamic_file(inode, "idle\n");
                }
                if let Some(inode) = self.external_thread_fingerprint_inode {
                    self.update_dynamic_file(inode, format!("{fingerprint}\n"));
                }
            }
        }
    }

    pub fn increment_external_subject_quota_requests(&mut self) {
        let Some(inode) = self.external_subject_quota_requests_inode else {
            return;
        };
        let current = self
            .nodes
            .get(&inode)
            .and_then(Node::content)
            .and_then(|content| content.trim().parse::<usize>().ok())
            .unwrap_or_default()
            .saturating_add(1);
        self.update_dynamic_file(inode, format!("{current}\n"));
    }

    pub fn append_tool_loop_control_step(&mut self, command_name: &str, next_state: &str) {
        let line = format!(
            "{{\"type\":\"control\",\"command\":\"{command_name}\",\"state\":\"{next_state}\"}}"
        );
        self.append_tool_loop_step(&line);
    }

    pub fn append_thread_messages(&mut self, pending: &PendingResponse, response_body: &str) {
        use std::fmt::Write as _;
        let Some(messages_inode) = self.thread_messages_inode else {
            return;
        };
        let user_text = user_content(&pending.request_body);
        let user_content = json_string(&user_text);
        let assistant_text = assistant_content(response_body);
        let assistant_content = json_string(&assistant_text);
        let Some(content) = self
            .nodes
            .get_mut(&messages_inode)
            .and_then(|node| node.content.as_mut())
            .and_then(NodeContent::as_dynamic_mut)
        else {
            return;
        };
        let _ = writeln!(content, "{{\"role\":\"user\",\"content\":{user_content}}}");
        let _ = writeln!(
            content,
            "{{\"role\":\"assistant\",\"content\":{assistant_content}}}"
        );
        if let Some(latest_inode) = self.thread_latest_inode {
            self.update_dynamic_file(latest_inode, format!("{assistant_text}\n"));
        }
        self.append_thread_episodic_memory(pending, &user_text);
        self.append_thread_episodic_memory(pending, &assistant_text);
        self.append_tool_loop_model_step(&assistant_text);
    }

    fn append_thread_episodic_memory(&mut self, pending: &PendingResponse, text: &str) {
        let id = format!(
            "thread-demo-{}",
            self.episodic_item_count().saturating_add(1)
        );
        let content = format!(
            "thread={LOCAL_USER_THREAD_DISPLAY_PATH}\nformat={}\ntext={text}",
            pending.format
        );
        self.append_memory_layer_item("episodic", &id, &content, &pending.fingerprint);
    }

    fn append_tool_loop_model_step(&mut self, message: &str) {
        let line = format!(
            "{{\"type\":\"model\",\"message\":{}}}",
            json_string(message)
        );
        self.append_tool_loop_step(&line);
    }

    fn append_tool_loop_step(&mut self, step_without_number: &str) {
        use std::fmt::Write as _;
        let Some(steps_inode) = self.tool_loop_steps_inode else {
            return;
        };
        if self.tool_loop_max_steps_exceeded(steps_inode) {
            if let Some(state_inode) = self.tool_loop_state_inode {
                self.update_dynamic_file(state_inode, "limit_exceeded\n");
            }
            self.append_audit("tool-loop.demo.limits", "max_steps", "exceeded");
            return;
        }
        if self.tool_loop_max_time_exceeded() {
            if let Some(state_inode) = self.tool_loop_state_inode {
                self.update_dynamic_file(state_inode, "limit_exceeded\n");
            }
            self.append_audit("tool-loop.demo.limits", "max_time_ms", "exceeded");
            return;
        }
        if self.tool_loop_max_cost_exceeded() {
            if let Some(state_inode) = self.tool_loop_state_inode {
                self.update_dynamic_file(state_inode, "limit_exceeded\n");
            }
            self.append_audit("tool-loop.demo.limits", "max_cost_usd", "exceeded");
            return;
        }
        let Some(content) = self.tool_loop_steps_content_mut(steps_inode) else {
            return;
        };
        let next_step = content.lines().count().saturating_add(1);
        let Some(rest) = step_without_number.strip_prefix('{') else {
            return;
        };
        let _ = writeln!(content, "{{\"step\":{next_step},{rest}");
        if self.tool_loop_started_at.is_none() {
            self.tool_loop_started_at = Some(std::time::Instant::now());
        }
        self.tool_loop_cost_micros = self.tool_loop_cost_micros.saturating_add(1);
    }

    fn tool_loop_max_steps_exceeded(&self, steps_inode: fuse3::Inode) -> bool {
        let current_steps = self
            .nodes
            .get(&steps_inode)
            .and_then(Node::content)
            .map_or(0, |content| content.lines().count());
        let Some(max_steps) = self
            .tool_loop_max_steps_inode
            .and_then(|inode| self.nodes.get(&inode))
            .and_then(Node::content)
            .and_then(|content| content.trim().parse::<usize>().ok())
        else {
            return false;
        };
        current_steps >= max_steps
    }

    fn tool_loop_max_time_exceeded(&self) -> bool {
        let Some(started_at) = self.tool_loop_started_at else {
            return false;
        };
        let Some(max_time_ms) = self
            .tool_loop_max_time_ms_inode
            .and_then(|inode| self.nodes.get(&inode))
            .and_then(Node::content)
            .and_then(|content| content.trim().parse::<u128>().ok())
        else {
            return false;
        };
        started_at.elapsed().as_millis() >= max_time_ms
    }

    fn tool_loop_max_cost_exceeded(&self) -> bool {
        let Some(max_cost_micros) = self
            .tool_loop_max_cost_usd_inode
            .and_then(|inode| self.nodes.get(&inode))
            .and_then(Node::content)
            .and_then(parse_usd_micros)
        else {
            return false;
        };
        self.tool_loop_cost_micros >= max_cost_micros
    }

    fn tool_loop_steps_content_mut(&mut self, steps_inode: fuse3::Inode) -> Option<&mut String> {
        self.nodes
            .get_mut(&steps_inode)?
            .content
            .as_mut()?
            .as_dynamic_mut()
    }

    pub fn append_external_thread_messages(
        &mut self,
        pending: &PendingResponse,
        response_body: &str,
    ) {
        use std::fmt::Write as _;
        let Some(messages_inode) = self.external_thread_messages_inode else {
            return;
        };
        let subject =
            external_subject(&pending.request_body).unwrap_or_else(|| "qq:user:123456".to_owned());
        let display_name =
            external_display_name(&pending.request_body).unwrap_or_else(|| "Alice".to_owned());
        let user_text = user_content(&pending.request_body);
        let user_content = json_string(&user_text);
        let assistant_text = assistant_content(response_body);
        let assistant_content = json_string(&assistant_text);
        let Some(content) = self
            .nodes
            .get_mut(&messages_inode)
            .and_then(|node| node.content.as_mut())
            .and_then(NodeContent::as_dynamic_mut)
        else {
            return;
        };
        let _ = writeln!(
            content,
            "{{\"role\":\"user\",\"content\":{user_content},\"subject\":{},\"display_name\":{}}}",
            json_string(&subject),
            json_string(&display_name),
        );
        let _ = writeln!(
            content,
            "{{\"role\":\"assistant\",\"content\":{assistant_content}}}"
        );
        if let Some(latest_inode) = self.external_thread_latest_inode {
            self.update_dynamic_file(latest_inode, format!("{assistant_text}\n"));
        }
        self.append_external_thread_episodic_memory(pending, &subject, &display_name, &user_text);
        self.append_external_thread_episodic_memory(
            pending,
            &subject,
            &display_name,
            &assistant_text,
        );
    }

    fn append_external_thread_episodic_memory(
        &mut self,
        pending: &PendingResponse,
        subject: &str,
        display_name: &str,
        text: &str,
    ) {
        let id = format!(
            "external-qq-demo-{}",
            self.episodic_item_count().saturating_add(1)
        );
        let content = format!(
            "thread=ext/qq/group/888888/thread/demo\nsubject={subject}\ndisplay_name={display_name}\nformat={}\ntext={text}",
            pending.format
        );
        self.append_memory_layer_item("episodic", &id, &content, &pending.fingerprint);
    }

    pub fn mark_thread_socket_queued(&mut self, request_id: &str) {
        if let Some(inode) = self.thread_state_inode {
            self.update_dynamic_file(inode, "queued\n");
        }
        self.append_audit("thread.demo.socket", request_id, "queued");
    }

    pub fn append_thread_socket_turn(&mut self, request_id: &str, user: &str, assistant: &str) {
        use std::fmt::Write as _;
        let fingerprint = socket_turn_fingerprint(user, assistant);
        if let Some(messages_inode) = self.thread_messages_inode
            && let Some(content) = self
                .nodes
                .get_mut(&messages_inode)
                .and_then(|node| node.content.as_mut())
                .and_then(NodeContent::as_dynamic_mut)
        {
            let _ = writeln!(
                content,
                "{{\"role\":\"user\",\"content\":{}}}",
                json_string(user)
            );
            let _ = writeln!(
                content,
                "{{\"role\":\"assistant\",\"content\":{}}}",
                json_string(assistant)
            );
        }
        if let Some(inode) = self.thread_latest_inode {
            self.update_dynamic_file(inode, format!("{assistant}\n"));
        }
        if let Some(inode) = self.thread_state_inode {
            self.update_dynamic_file(inode, "idle\n");
        }
        if let Some(inode) = self.thread_fingerprint_inode {
            self.update_dynamic_file(inode, format!("{fingerprint}\n"));
        }
        self.append_tool_loop_model_step(assistant);
        self.append_audit_with_fingerprint(
            "thread.demo.socket",
            request_id,
            "drained",
            &fingerprint,
        );
    }

    pub fn mark_thread_socket_error(&mut self, request_id: &str, error: &str) {
        if let Some(inode) = self.thread_state_inode {
            self.update_dynamic_file(inode, "idle\n");
        }
        if let Some(inode) = self.thread_latest_inode {
            self.update_dynamic_file(inode, format!("error: {error}\n"));
        }
        self.append_audit("thread.demo.socket", request_id, "error");
    }

    pub fn ensure_thread_socket_session(
        &mut self,
        session: &str,
        scope: &str,
        cwd: &str,
    ) -> Result<(), String> {
        validate_socket_session(session)?;
        if session == "demo" {
            self.update_thread_index("demo");
            return Ok(());
        }
        if let Some(inodes) = self.thread_sessions.get(session).copied() {
            self.update_dynamic_file(inodes.scope, format!("{scope}\n"));
            self.update_dynamic_file(inodes.cwd, format!("{cwd}\n"));
            self.update_dynamic_file(inodes.updated, format!("{}\n", now_text()));
            self.update_thread_index(session);
            return Ok(());
        }
        let Some(root) = self.thread_root_inode else {
            return Err("thread root unavailable".to_owned());
        };
        let created = now_text();
        let thread = self.add_dynamic_dir(root, session.to_owned());
        self.add_dynamic_file(thread, "context", LOCAL_USER_THREAD_CONTEXT_TEXT);
        self.add_dynamic_dir(thread, "inbox");
        self.add_dynamic_symlink(thread, "io.sock", LOCAL_THREAD_SOCKET_PATH);
        self.add_dynamic_file(thread, "memory_scope", LOCAL_USER_MEMORY_SCOPE_TEXT);
        self.add_dynamic_dir(thread, "control");
        let messages = self.add_dynamic_file(thread, "messages.jsonl", "");
        let latest = self.add_dynamic_file(thread, "latest.md", "");
        let state = self.add_dynamic_file(thread, "state", "idle\n");
        let fingerprint = self.add_dynamic_file(thread, "fingerprint", "");
        let scope_inode = self.add_dynamic_file_owned(thread, "scope", format!("{scope}\n"));
        let cwd_inode = self.add_dynamic_file_owned(thread, "cwd", format!("{cwd}\n"));
        let created_inode = self.add_dynamic_file_owned(thread, "created", format!("{created}\n"));
        let updated_inode = self.add_dynamic_file_owned(thread, "updated", format!("{created}\n"));
        self.thread_sessions.insert(
            session.to_owned(),
            crate::runtime_types::ThreadSessionInodes {
                root: thread,
                messages,
                latest,
                state,
                fingerprint,
                scope: scope_inode,
                cwd: cwd_inode,
                created: created_inode,
                updated: updated_inode,
            },
        );
        self.update_thread_index(session);
        Ok(())
    }

    pub fn thread_socket_messages(&self, session: &str) -> String {
        if session == "demo" {
            return self
                .thread_messages_inode
                .and_then(|inode| self.nodes.get(&inode))
                .and_then(Node::content)
                .unwrap_or_default()
                .to_owned();
        }
        self.thread_sessions
            .get(session)
            .and_then(|inodes| self.nodes.get(&inodes.messages))
            .and_then(Node::content)
            .unwrap_or_default()
            .to_owned()
    }

    pub fn mark_thread_session_socket_queued(&mut self, session: &str, request_id: &str) {
        if session == "demo" {
            self.mark_thread_socket_queued(request_id);
            return;
        }
        if let Some(inodes) = self.thread_sessions.get(session).copied() {
            self.update_dynamic_file(inodes.state, "queued\n");
            self.update_dynamic_file(inodes.updated, format!("{}\n", now_text()));
        }
        self.update_thread_index(session);
        self.append_audit(&format!("thread.{session}.socket"), request_id, "queued");
    }

    pub fn append_thread_session_socket_turn(
        &mut self,
        session: &str,
        request_id: &str,
        user: &str,
        assistant: &str,
    ) {
        use std::fmt::Write as _;

        if session == "demo" {
            self.append_thread_socket_turn(request_id, user, assistant);
            self.update_thread_index("demo");
            return;
        }
        let fingerprint = socket_turn_fingerprint(user, assistant);
        if let Some(inodes) = self.thread_sessions.get(session).copied() {
            if let Some(content) = self
                .nodes
                .get_mut(&inodes.messages)
                .and_then(|node| node.content.as_mut())
                .and_then(NodeContent::as_dynamic_mut)
            {
                let _ = writeln!(
                    content,
                    "{{\"role\":\"user\",\"content\":{}}}",
                    json_string(user)
                );
                let _ = writeln!(
                    content,
                    "{{\"role\":\"assistant\",\"content\":{}}}",
                    json_string(assistant)
                );
            }
            self.update_dynamic_file(inodes.latest, format!("{assistant}\n"));
            self.update_dynamic_file(inodes.state, "idle\n");
            self.update_dynamic_file(inodes.fingerprint, format!("{fingerprint}\n"));
            self.update_dynamic_file(inodes.updated, format!("{}\n", now_text()));
        }
        self.update_thread_index(session);
        self.append_audit_with_fingerprint(
            &format!("thread.{session}.socket"),
            request_id,
            "drained",
            &fingerprint,
        );
    }

    pub fn mark_thread_session_socket_error(
        &mut self,
        session: &str,
        request_id: &str,
        error: &str,
    ) {
        if session == "demo" {
            self.mark_thread_socket_error(request_id, error);
            self.update_thread_index("demo");
            return;
        }
        if let Some(inodes) = self.thread_sessions.get(session).copied() {
            self.update_dynamic_file(inodes.state, "idle\n");
            self.update_dynamic_file(inodes.latest, format!("error: {error}\n"));
            self.update_dynamic_file(inodes.updated, format!("{}\n", now_text()));
        }
        self.update_thread_index(session);
        self.append_audit(&format!("thread.{session}.socket"), request_id, "error");
    }

    fn update_thread_index(&mut self, current: &str) {
        let Some(count_inode) = self.thread_count_inode else {
            return;
        };
        let mut rows = vec!["demo\tworkspace\t\t\n".to_owned()];
        for (id, inodes) in &self.thread_sessions {
            let scope = self
                .nodes
                .get(&inodes.scope)
                .and_then(Node::content)
                .unwrap_or_default()
                .trim();
            let updated = self
                .nodes
                .get(&inodes.updated)
                .and_then(Node::content)
                .unwrap_or_default()
                .trim();
            let cwd = self
                .nodes
                .get(&inodes.cwd)
                .and_then(Node::content)
                .unwrap_or_default()
                .trim();
            rows.push(format!("{id}\t{scope}\t{updated}\t{cwd}\n"));
        }
        self.update_dynamic_file(count_inode, format!("{}\n", rows.len()));
        if let Some(inode) = self.thread_list_inode {
            self.update_dynamic_file(inode, rows.concat());
        }
        if let Some(inode) = self.thread_current_inode {
            self.update_dynamic_file(inode, format!("{current}\n"));
        }
    }

    fn episodic_item_count(&self) -> usize {
        self.memory_layer_items
            .episodic
            .and_then(|inode| self.nodes.get(&inode))
            .and_then(Node::content)
            .map_or(0, |content| content.lines().count())
    }
}

fn validate_socket_session(session: &str) -> Result<(), String> {
    ThreadId::new(session.to_owned()).map_err(|error| error.to_string())?;
    if matches!(session, "count" | "list" | "current") {
        return Err("reserved thread session id".to_owned());
    }
    Ok(())
}

fn now_text() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("unix:{seconds}")
}

fn socket_turn_fingerprint(user: &str, assistant: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in user.bytes().chain(assistant.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("fnv1a64:{hash:016x}")
}

fn parse_usd_micros(value: &str) -> Option<u64> {
    let value = value.trim();
    let mut parts = value.split('.');
    let whole = parts.next()?.parse::<u64>().ok()?;
    let fractional = parts.next().unwrap_or_default();
    if parts.next().is_some() || fractional.len() > 6 {
        return None;
    }
    let mut micros = fractional.parse::<u64>().unwrap_or_default();
    for _ in fractional.len()..6 {
        micros = micros.saturating_mul(10);
    }
    whole.checked_mul(1_000_000)?.checked_add(micros)
}
