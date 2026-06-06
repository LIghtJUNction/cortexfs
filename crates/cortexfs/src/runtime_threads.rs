use crate::runtime_types::{PendingResponse, ThreadUpdate};
use crate::text::{
    assistant_content, external_display_name, external_subject, json_string, user_content,
};
use crate::{Node, NodeContent, RuntimeState};

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
        use std::fmt::Write as _;
        let Some(steps_inode) = self.tool_loop_steps_inode else {
            return;
        };
        let Some(content) = self
            .nodes
            .get_mut(&steps_inode)
            .and_then(|node| node.content.as_mut())
            .and_then(NodeContent::as_dynamic_mut)
        else {
            return;
        };
        let next_step = content.lines().count().saturating_add(1);
        let _ = writeln!(
            content,
            "{{\"step\":{next_step},\"type\":\"control\",\"command\":\"{command_name}\",\"state\":\"{next_state}\"}}"
        );
    }

    pub fn append_thread_messages(&mut self, pending: &PendingResponse, response_body: &str) {
        use std::fmt::Write as _;
        let Some(messages_inode) = self.thread_messages_inode else {
            return;
        };
        let user_content = json_string(&user_content(&pending.request_body));
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
        self.append_tool_loop_model_step(&assistant_text);
    }

    fn append_tool_loop_model_step(&mut self, message: &str) {
        use std::fmt::Write as _;
        let Some(steps_inode) = self.tool_loop_steps_inode else {
            return;
        };
        let Some(content) = self
            .nodes
            .get_mut(&steps_inode)
            .and_then(|node| node.content.as_mut())
            .and_then(NodeContent::as_dynamic_mut)
        else {
            return;
        };
        let next_step = content.lines().count().saturating_add(1);
        let _ = writeln!(
            content,
            "{{\"step\":{next_step},\"type\":\"model\",\"message\":{}}}",
            json_string(message),
        );
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
        let user_content = json_string(&user_content(&pending.request_body));
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
    }
}
