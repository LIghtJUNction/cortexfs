use cortex_store::RequestId;
use fuse3::Inode;

use crate::runtime_types::{AgentTask, ConversationExportRow, PendingResponse};
use crate::text::json_string;
use crate::{Node, NodeContent, RuntimeState};

impl RuntimeState {
    pub fn refresh_exports(&mut self) {
        self.refresh_conversations_export();
        let sft = self.sft_export();
        if let Some(inode) = self.sft_export_inode {
            self.update_dynamic_file(inode, sft);
        }
        if let Some(inode) = self.export_refresh_inode {
            self.update_dynamic_file(inode, "1\n");
        }
        self.append_audit("exports", "refresh", "refreshed");
    }

    fn sft_export(&self) -> String {
        use std::fmt::Write as _;

        let Some(messages) = self
            .thread_messages_inode
            .and_then(|inode| self.nodes.get(&inode))
            .and_then(Node::content)
        else {
            return String::new();
        };
        let mut rows = String::new();
        let parsed = messages
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .collect::<Vec<_>>();
        for pair in parsed.chunks(2) {
            if pair.len() != 2 {
                continue;
            }
            let Some(user) = pair.first() else {
                continue;
            };
            let Some(assistant) = pair.get(1) else {
                continue;
            };
            if user.get("role").and_then(serde_json::Value::as_str) != Some("user")
                || assistant.get("role").and_then(serde_json::Value::as_str) != Some("assistant")
            {
                continue;
            }
            let prompt = user
                .get("content")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let completion = assistant
                .get("content")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let _ = writeln!(
                rows,
                "{{\"messages\":[{{\"role\":\"user\",\"content\":{}}},{{\"role\":\"assistant\",\"content\":{}}}],\"source\":\"threads/demo/messages.jsonl\"}}",
                json_string(prompt),
                json_string(completion),
            );
        }
        rows
    }

    pub fn append_conversation_export(
        &mut self,
        request_id: &RequestId,
        pending: &PendingResponse,
        response_body: &str,
    ) {
        let request = json_string(&pending.request_body);
        let response = json_string(response_body);
        let (line, provider, model) = pending.route.as_ref().map_or_else(
            || {
                (
                    format!(
                        "{{\"request_id\":\"{}\",\"format\":\"{}\",\"fingerprint\":\"{}\",\"request\":{},\"response\":{}}}",
                        request_id.as_str(),
                        pending.format,
                        pending.fingerprint,
                        request,
                        response,
                    ),
                    None,
                    None,
                )
            },
            |route| {
                (
                    format!(
                        "{{\"request_id\":\"{}\",\"format\":\"{}\",\"fingerprint\":\"{}\",\"route\":{{\"provider\":{},\"model\":{},\"reason\":{}}},\"request\":{},\"response\":{}}}",
                        request_id.as_str(),
                        pending.format,
                        pending.fingerprint,
                        json_string(&route.provider),
                        json_string(&route.model),
                        json_string(&route.reason),
                        request,
                        response,
                    ),
                    Some(route.provider.clone()),
                    Some(route.model.clone()),
                )
            },
        );
        self.conversation_rows.push(ConversationExportRow {
            line,
            provider,
            model,
            failed: false,
        });
        self.refresh_conversations_export();
    }

    pub fn refresh_conversations_export(&mut self) {
        use std::fmt::Write as _;
        let Some(export_inode) = self.conversations_export_inode else {
            return;
        };
        let provider_filter = self.export_filter_value(self.export_filter_provider_inode);
        let model_filter = self.export_filter_value(self.export_filter_model_inode);
        let exclude_failed =
            self.export_filter_value(self.export_filter_exclude_failed_inode) != "0";
        let mut rows = String::new();
        for row in &self.conversation_rows {
            if exclude_failed && row.failed {
                continue;
            }
            if !provider_filter.is_empty() && row.provider.as_deref() != Some(provider_filter) {
                continue;
            }
            if !model_filter.is_empty() && row.model.as_deref() != Some(model_filter) {
                continue;
            }
            let _ = writeln!(rows, "{}", row.line);
        }
        self.update_dynamic_file(export_inode, rows);
    }

    fn export_filter_value(&self, inode: Option<Inode>) -> &str {
        inode
            .and_then(|inode| self.nodes.get(&inode))
            .and_then(Node::content)
            .map(str::trim)
            .unwrap_or_default()
    }

    pub fn append_tool_call_export(
        &mut self,
        request_id: &RequestId,
        pending: &PendingResponse,
        response_body: &str,
    ) {
        use std::fmt::Write as _;
        let Some(export_inode) = self.tool_calls_export_inode else {
            return;
        };
        let Some(content) = self
            .nodes
            .get_mut(&export_inode)
            .and_then(|node| node.content.as_mut())
            .and_then(NodeContent::as_dynamic_mut)
        else {
            return;
        };
        let _ = writeln!(
            content,
            "{{\"request_id\":\"{}\",\"tool\":\"{}\",\"status\":\"ok\",\"input\":{},\"output\":{},\"fingerprint\":\"{}\"}}",
            request_id.as_str(),
            pending.tool.unwrap_or(pending.format),
            json_string(&pending.request_body),
            json_string(response_body),
            pending.fingerprint,
        );
    }

    pub fn append_tool_loop_steps(
        &mut self,
        request_id: &RequestId,
        pending: &PendingResponse,
        response_body: &str,
    ) {
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
        let tool = pending.tool.unwrap_or(pending.format);
        let _ = writeln!(
            content,
            "{{\"step\":{},\"type\":\"tool_call\",\"request_id\":\"{}\",\"tool\":\"{}\",\"input\":{},\"fingerprint\":\"{}\"}}",
            next_step,
            request_id.as_str(),
            tool,
            json_string(&pending.request_body),
            pending.fingerprint,
        );
        let _ = writeln!(
            content,
            "{{\"step\":{},\"type\":\"tool_result\",\"request_id\":\"{}\",\"tool\":\"{}\",\"output\":{},\"fingerprint\":\"{}\"}}",
            next_step.saturating_add(1),
            request_id.as_str(),
            tool,
            json_string(response_body),
            pending.fingerprint,
        );
    }

    pub fn append_agent_trace_export(
        &mut self,
        request_id: &RequestId,
        pending: &PendingResponse,
        response_body: &str,
    ) {
        use std::fmt::Write as _;
        let Some(export_inode) = self.agent_traces_export_inode else {
            return;
        };
        let Some(content) = self
            .nodes
            .get_mut(&export_inode)
            .and_then(|node| node.content.as_mut())
            .and_then(NodeContent::as_dynamic_mut)
        else {
            return;
        };
        let tool = pending.tool.unwrap_or(pending.format);
        let _ = writeln!(
            content,
            "{{\"agent\":\"helper\",\"thread\":\"demo\",\"request_id\":\"{}\",\"event\":\"tool_call\",\"tool\":\"{}\",\"input\":{},\"fingerprint\":\"{}\"}}",
            request_id.as_str(),
            tool,
            json_string(&pending.request_body),
            pending.fingerprint,
        );
        let _ = writeln!(
            content,
            "{{\"agent\":\"helper\",\"thread\":\"demo\",\"request_id\":\"{}\",\"event\":\"tool_result\",\"tool\":\"{}\",\"output\":{},\"fingerprint\":\"{}\"}}",
            request_id.as_str(),
            tool,
            json_string(response_body),
            pending.fingerprint,
        );
    }

    pub fn append_agent_task_trace(
        &mut self,
        request_id: &RequestId,
        task: &AgentTask,
        response_body: &str,
    ) {
        use std::fmt::Write as _;
        let Some(export_inode) = self.agent_traces_export_inode else {
            return;
        };
        let Some(content) = self
            .nodes
            .get_mut(&export_inode)
            .and_then(|node| node.content.as_mut())
            .and_then(NodeContent::as_dynamic_mut)
        else {
            return;
        };
        let _ = writeln!(
            content,
            "{{\"agent\":\"helper\",\"request_id\":\"{}\",\"event\":\"task\",\"input\":{},\"fingerprint\":\"{}\"}}",
            request_id.as_str(),
            json_string(&task.body),
            task.fingerprint,
        );
        let _ = writeln!(
            content,
            "{{\"agent\":\"helper\",\"request_id\":\"{}\",\"event\":\"task_result\",\"output\":{},\"fingerprint\":\"{}\"}}",
            request_id.as_str(),
            json_string(response_body),
            task.fingerprint,
        );
    }
}
