use crate::runtime_types::{PendingResponse, ThreadUpdate};
use crate::text::{
    assistant_content, external_display_name, external_subject, json_string, user_content,
};
use crate::{LOCAL_USER_THREAD_DISPLAY_PATH, Node, NodeContent, RuntimeState};

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

    fn episodic_item_count(&self) -> usize {
        self.memory_layer_items
            .episodic
            .and_then(|inode| self.nodes.get(&inode))
            .and_then(Node::content)
            .map_or(0, |content| content.lines().count())
    }
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
