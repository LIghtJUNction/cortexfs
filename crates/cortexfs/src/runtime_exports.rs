use cortex_store::RequestId;
use fuse3::Inode;

use crate::runtime_types::{
    AgentTask, ConversationExportRow, PendingResponse, TrainingExportMetadata, TrainingExportRow,
};
use crate::submission::SubmissionScope;
use crate::text::{external_subject, json_string};
use crate::{Node, NodeContent, RuntimeState};

impl RuntimeState {
    pub fn refresh_exports(&mut self) {
        self.refresh_training_exports();
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
                "{{\"messages\":[{{\"role\":\"user\",\"content\":{}}},{{\"role\":\"assistant\",\"content\":{}}}],\"source\":\"thread/demo/messages.jsonl\"}}",
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
        let (subject, space) = export_subject_and_space(pending);
        let metadata = export_metadata_json(subject.as_deref());
        self.export_seq = self.export_seq.saturating_add(1);
        let time = format!("{:020}", self.export_seq);
        let (line, provider, model) = pending.route.as_ref().map_or_else(
            || {
                (
                    format!(
                        "{{\"request_id\":\"{}\",\"time\":\"{}\",\"format\":\"{}\",\"fingerprint\":\"{}\",\"agent\":\"helper\",\"space\":{},{}\"request\":{},\"response\":{}}}",
                        request_id.as_str(),
                        time,
                        pending.format,
                        pending.fingerprint,
                        json_string(space),
                        metadata,
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
                        "{{\"request_id\":\"{}\",\"time\":\"{}\",\"format\":\"{}\",\"fingerprint\":\"{}\",\"agent\":\"helper\",\"space\":{},{}\"route\":{{\"provider\":{},\"model\":{},\"reason\":{}}},\"request\":{},\"response\":{}}}",
                        request_id.as_str(),
                        time,
                        pending.format,
                        pending.fingerprint,
                        json_string(space),
                        metadata,
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
            time,
            provider,
            model,
            agent: Some("helper".to_owned()),
            subject,
            space: Some(space.to_owned()),
            failed: false,
        });
        self.refresh_training_exports();
    }

    pub fn refresh_training_exports(&mut self) {
        self.refresh_conversations_export();
        self.refresh_tool_calls_export();
        self.refresh_agent_traces_export();
    }

    fn refresh_conversations_export(&mut self) {
        use std::fmt::Write as _;
        let Some(export_inode) = self.conversations_export_inode else {
            return;
        };
        let mut rows = String::new();
        for row in &self.conversation_rows {
            if !self.export_row_visible(row) {
                continue;
            }
            let _ = writeln!(rows, "{}", row.line);
        }
        self.update_dynamic_file(export_inode, rows);
    }

    fn refresh_tool_calls_export(&mut self) {
        let Some(export_inode) = self.tool_calls_export_inode else {
            return;
        };
        let content = self.filtered_training_rows(&self.tool_call_rows);
        self.update_dynamic_file(export_inode, content);
    }

    fn refresh_agent_traces_export(&mut self) {
        let Some(export_inode) = self.agent_traces_export_inode else {
            return;
        };
        let content = self.filtered_training_rows(&self.agent_trace_rows);
        self.update_dynamic_file(export_inode, content);
    }

    fn filtered_training_rows(&self, rows: &[TrainingExportRow]) -> String {
        use std::fmt::Write as _;

        let mut content = String::new();
        for row in rows {
            if !self.export_row_visible(row) {
                continue;
            }
            let _ = writeln!(content, "{}", row.line);
        }
        content
    }

    fn export_row_visible(&self, row: &impl ExportFilterRow) -> bool {
        let provider_filter = self.export_filter_value(self.export_filter_provider_inode);
        let model_filter = self.export_filter_value(self.export_filter_model_inode);
        let agent_filter = self.export_filter_value(self.export_filter_agent_inode);
        let subject_filter = self.export_filter_value(self.export_filter_subject_inode);
        let space_filter = self.export_filter_value(self.export_filter_space_inode);
        let from_filter = self.export_filter_value(self.export_filter_from_inode);
        let to_filter = self.export_filter_value(self.export_filter_to_inode);
        let exclude_failed =
            self.export_filter_value(self.export_filter_exclude_failed_inode) != "0";

        if exclude_failed && row.failed() {
            return false;
        }
        if !provider_filter.is_empty() && row.provider() != Some(provider_filter) {
            return false;
        }
        if !model_filter.is_empty() && row.model() != Some(model_filter) {
            return false;
        }
        if !agent_filter.is_empty() && row.agent() != Some(agent_filter) {
            return false;
        }
        if !subject_filter.is_empty() && row.subject() != Some(subject_filter) {
            return false;
        }
        if !space_filter.is_empty() && row.space() != Some(space_filter) {
            return false;
        }
        if !from_filter.is_empty() && row.time() < from_filter {
            return false;
        }
        if !to_filter.is_empty() && row.time() > to_filter {
            return false;
        }
        true
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
        let line = format!(
            "{{\"request_id\":\"{}\",\"tool\":\"{}\",\"status\":\"ok\",\"input\":{},\"output\":{},\"fingerprint\":\"{}\"}}",
            request_id.as_str(),
            pending.tool.unwrap_or(pending.format),
            json_string(&pending.request_body),
            json_string(response_body),
            pending.fingerprint,
        );
        let row = self.next_training_export_row(line, helper_training_metadata());
        self.tool_call_rows.push(row);
        self.refresh_tool_calls_export();
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
        let tool = pending.tool.unwrap_or(pending.format);
        for line in [
            format!(
                "{{\"agent\":\"helper\",\"thread\":\"demo\",\"request_id\":\"{}\",\"event\":\"tool_call\",\"tool\":\"{}\",\"input\":{},\"fingerprint\":\"{}\"}}",
                request_id.as_str(),
                tool,
                json_string(&pending.request_body),
                pending.fingerprint,
            ),
            format!(
                "{{\"agent\":\"helper\",\"thread\":\"demo\",\"request_id\":\"{}\",\"event\":\"tool_result\",\"tool\":\"{}\",\"output\":{},\"fingerprint\":\"{}\"}}",
                request_id.as_str(),
                tool,
                json_string(response_body),
                pending.fingerprint,
            ),
        ] {
            let row = self.next_training_export_row(line, helper_training_metadata());
            self.agent_trace_rows.push(row);
        }
        self.refresh_agent_traces_export();
    }

    pub fn append_agent_task_trace(
        &mut self,
        request_id: &RequestId,
        task: &AgentTask,
        response_body: &str,
    ) {
        for line in [
            format!(
                "{{\"agent\":\"helper\",\"request_id\":\"{}\",\"event\":\"task\",\"input\":{},\"fingerprint\":\"{}\"}}",
                request_id.as_str(),
                json_string(&task.body),
                task.fingerprint,
            ),
            format!(
                "{{\"agent\":\"helper\",\"request_id\":\"{}\",\"event\":\"task_result\",\"output\":{},\"fingerprint\":\"{}\"}}",
                request_id.as_str(),
                json_string(response_body),
                task.fingerprint,
            ),
        ] {
            let row = self.next_training_export_row(line, helper_training_metadata());
            self.agent_trace_rows.push(row);
        }
        self.refresh_agent_traces_export();
    }

    fn next_training_export_row(
        &mut self,
        line: String,
        metadata: TrainingExportMetadata,
    ) -> TrainingExportRow {
        self.export_seq = self.export_seq.saturating_add(1);
        TrainingExportRow {
            line,
            time: format!("{:020}", self.export_seq),
            provider: metadata.provider,
            model: metadata.model,
            agent: metadata.agent,
            subject: metadata.subject,
            space: metadata.space,
            failed: metadata.failed,
        }
    }
}

trait ExportFilterRow {
    fn time(&self) -> &str;
    fn provider(&self) -> Option<&str>;
    fn model(&self) -> Option<&str>;
    fn agent(&self) -> Option<&str>;
    fn subject(&self) -> Option<&str>;
    fn space(&self) -> Option<&str>;
    fn failed(&self) -> bool;
}

impl ExportFilterRow for ConversationExportRow {
    fn time(&self) -> &str {
        &self.time
    }

    fn provider(&self) -> Option<&str> {
        self.provider.as_deref()
    }

    fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    fn agent(&self) -> Option<&str> {
        self.agent.as_deref()
    }

    fn subject(&self) -> Option<&str> {
        self.subject.as_deref()
    }

    fn space(&self) -> Option<&str> {
        self.space.as_deref()
    }

    fn failed(&self) -> bool {
        self.failed
    }
}

impl ExportFilterRow for TrainingExportRow {
    fn time(&self) -> &str {
        &self.time
    }

    fn provider(&self) -> Option<&str> {
        self.provider.as_deref()
    }

    fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    fn agent(&self) -> Option<&str> {
        self.agent.as_deref()
    }

    fn subject(&self) -> Option<&str> {
        self.subject.as_deref()
    }

    fn space(&self) -> Option<&str> {
        self.space.as_deref()
    }

    fn failed(&self) -> bool {
        self.failed
    }
}

fn helper_training_metadata() -> TrainingExportMetadata {
    TrainingExportMetadata {
        agent: Some("helper".to_owned()),
        space: Some("home/1000".to_owned()),
        ..TrainingExportMetadata::default()
    }
}

fn export_metadata_json(subject: Option<&str>) -> String {
    subject.map_or_else(String::new, |subject| {
        format!("\"subject\":{},", json_string(subject))
    })
}

fn export_subject_and_space(pending: &PendingResponse) -> (Option<String>, &'static str) {
    if pending.scope == SubmissionScope::ExternalThread {
        (
            external_subject(&pending.request_body),
            "spaces/external/qq/groups/888888",
        )
    } else {
        (None, "home/1000")
    }
}
