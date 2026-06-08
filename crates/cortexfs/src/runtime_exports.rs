use cortex_store::RequestId;
use fuse3::Inode;
use std::collections::HashSet;

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
        self.append_audit("export", "refresh", "refreshed");
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
                "{{\"messages\":[{{\"role\":\"user\",\"content\":{}}},{{\"role\":\"assistant\",\"content\":{}}}],\"source\":\"home/1000/thread/demo/messages.jsonl\"}}",
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
        let local_space = self.context.local_space.clone();
        let external_space = self.context.external_space.clone();
        let (subject, space) = export_subject_and_space(pending, &local_space, &external_space);
        let metadata = export_metadata_json(subject.as_deref());
        let agent = self.context.agent.clone();
        self.export_seq = self.export_seq.saturating_add(1);
        let time = format!("{:020}", self.export_seq);
        let (line, provider, model) = pending.route.as_ref().map_or_else(
            || {
                (
                    format!(
                        "{{\"request_id\":\"{}\",\"time\":\"{}\",\"format\":\"{}\",\"fingerprint\":\"{}\",\"agent\":{},\"space\":{},{}\"request\":{},\"response\":{}}}",
                        request_id.as_str(),
                        time,
                        pending.format,
                        pending.fingerprint,
                        json_string(&agent),
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
                        "{{\"request_id\":\"{}\",\"time\":\"{}\",\"format\":\"{}\",\"fingerprint\":\"{}\",\"agent\":{},\"space\":{},{}\"route\":{{\"provider\":{},\"model\":{},\"reason\":{}}},\"request\":{},\"response\":{}}}",
                        request_id.as_str(),
                        time,
                        pending.format,
                        pending.fingerprint,
                        json_string(&agent),
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
            fingerprint: pending.fingerprint.clone(),
            dedupe_key: pending.fingerprint.clone(),
            provider,
            model,
            agent: Some(agent),
            subject,
            space: Some(space.to_owned()),
            failed: false,
        });
        self.refresh_training_exports();
    }

    pub fn refresh_training_exports(&mut self) {
        self.refresh_conversations_export();
        self.refresh_preference_export();
        self.refresh_tool_calls_export();
        self.refresh_agent_traces_export();
    }

    fn refresh_conversations_export(&mut self) {
        let Some(export_inode) = self.conversations_export_inode else {
            return;
        };
        let rows = self.filtered_deduped_rows(&self.conversation_rows);
        self.update_dynamic_file(export_inode, rows);
    }

    fn refresh_tool_calls_export(&mut self) {
        let Some(export_inode) = self.tool_calls_export_inode else {
            return;
        };
        let content = self.filtered_training_rows(&self.tool_call_rows);
        self.update_dynamic_file(export_inode, content);
    }

    fn refresh_preference_export(&mut self) {
        let Some(export_inode) = self.preference_export_inode else {
            return;
        };
        let content = self.filtered_training_rows(&self.preference_rows);
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
        self.filtered_deduped_rows(rows)
    }

    fn filtered_deduped_rows(&self, rows: &[impl ExportFilterRow]) -> String {
        use std::fmt::Write as _;

        let mut content = String::new();
        let mut seen = HashSet::new();
        for row in rows {
            if !self.export_row_visible(row) {
                continue;
            }
            if !seen.insert(row.dedupe_key()) {
                continue;
            }
            let _ = writeln!(content, "{}", row.line());
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
        let tool = pending.tool.unwrap_or(pending.format);
        let source = tool_submit_source(tool, request_id);
        let line = format!(
            "{{\"request_id\":\"{}\",\"source\":{},\"tool\":\"{}\",\"status\":\"ok\",\"input\":{},\"output\":{},\"fingerprint\":\"{}\"}}",
            request_id.as_str(),
            json_string(&source),
            tool,
            json_string(&pending.request_body),
            json_string(response_body),
            pending.fingerprint,
        );
        let row = self
            .next_training_export_row(line, self.helper_training_metadata(&pending.fingerprint));
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
            "{}",
            permission_check_line(next_step, request_id, tool, pending)
        );
        let _ = writeln!(
            content,
            "{{\"step\":{},\"type\":\"tool_call\",\"request_id\":\"{}\",\"tool\":\"{}\",\"input\":{},\"fingerprint\":\"{}\"}}",
            next_step.saturating_add(1),
            request_id.as_str(),
            tool,
            json_string(&pending.request_body),
            pending.fingerprint,
        );
        let _ = writeln!(
            content,
            "{{\"step\":{},\"type\":\"tool_result\",\"request_id\":\"{}\",\"tool\":\"{}\",\"output\":{},\"fingerprint\":\"{}\"}}",
            next_step.saturating_add(2),
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
        let permission_check = permission_check_trace_line(request_id, tool, pending);
        let agent = self.context.agent.clone();
        let source = tool_submit_source(tool, request_id);
        for line in [
            permission_check,
            format!(
                "{{\"agent\":{},\"thread\":\"demo\",\"request_id\":\"{}\",\"source\":{},\"event\":\"tool_call\",\"tool\":\"{}\",\"input\":{},\"fingerprint\":\"{}\"}}",
                json_string(&agent),
                request_id.as_str(),
                json_string(&source),
                tool,
                json_string(&pending.request_body),
                pending.fingerprint,
            ),
            format!(
                "{{\"agent\":{},\"thread\":\"demo\",\"request_id\":\"{}\",\"source\":{},\"event\":\"tool_result\",\"tool\":\"{}\",\"output\":{},\"fingerprint\":\"{}\"}}",
                json_string(&agent),
                request_id.as_str(),
                json_string(&source),
                tool,
                json_string(response_body),
                pending.fingerprint,
            ),
        ] {
            let slot = trace_slot(&line);
            let row = self.next_training_export_row(
                line,
                self.helper_training_metadata_with_slot(&pending.fingerprint, slot),
            );
            self.agent_trace_rows.push(row);
        }
        self.refresh_agent_traces_export();
    }

    pub fn append_tool_permission_denial(
        &mut self,
        request_id: &RequestId,
        pending: &PendingResponse,
    ) {
        use std::fmt::Write as _;

        let tool = pending.tool.unwrap_or(pending.format);
        if let Some(steps_inode) = self.tool_loop_steps_inode
            && let Some(content) = self
                .nodes
                .get_mut(&steps_inode)
                .and_then(|node| node.content.as_mut())
                .and_then(NodeContent::as_dynamic_mut)
        {
            let next_step = content.lines().count().saturating_add(1);
            let _ = writeln!(
                content,
                "{}",
                permission_check_line_with_decision(next_step, request_id, tool, pending, "deny")
            );
        }

        let row = self.next_training_export_row(
            permission_check_trace_line_with_decision(request_id, tool, pending, "deny"),
            self.helper_training_metadata_with_slot(&pending.fingerprint, "permission_check"),
        );
        self.agent_trace_rows.push(row);
        self.refresh_agent_traces_export();
    }

    pub fn append_agent_task_trace(
        &mut self,
        request_id: &RequestId,
        task: &AgentTask,
        response_body: &str,
    ) {
        let agent = self.context.agent.clone();
        let source = agent_task_source(&agent, request_id);
        for line in [
            format!(
                "{{\"agent\":{},\"request_id\":\"{}\",\"source\":{},\"event\":\"task\",\"input\":{},\"fingerprint\":\"{}\"}}",
                json_string(&agent),
                request_id.as_str(),
                json_string(&source),
                json_string(&task.body),
                task.fingerprint,
            ),
            format!(
                "{{\"agent\":{},\"request_id\":\"{}\",\"source\":{},\"event\":\"task_result\",\"output\":{},\"fingerprint\":\"{}\"}}",
                json_string(&agent),
                request_id.as_str(),
                json_string(&source),
                json_string(response_body),
                task.fingerprint,
            ),
        ] {
            let slot = trace_slot(&line);
            let row = self.next_training_export_row(
                line,
                self.helper_training_metadata_with_slot(&task.fingerprint, slot),
            );
            self.agent_trace_rows.push(row);
        }
        self.refresh_agent_traces_export();
    }

    pub(crate) fn next_training_export_row(
        &mut self,
        line: String,
        metadata: TrainingExportMetadata,
    ) -> TrainingExportRow {
        self.export_seq = self.export_seq.saturating_add(1);
        TrainingExportRow {
            line,
            time: format!("{:020}", self.export_seq),
            dedupe_key: if metadata.dedupe_key.is_empty() {
                metadata.fingerprint.clone()
            } else {
                metadata.dedupe_key
            },
            fingerprint: metadata.fingerprint,
            provider: metadata.provider,
            model: metadata.model,
            agent: metadata.agent,
            subject: metadata.subject,
            space: metadata.space,
            failed: metadata.failed,
        }
    }

    fn helper_training_metadata(&self, fingerprint: &str) -> TrainingExportMetadata {
        TrainingExportMetadata {
            fingerprint: fingerprint.to_owned(),
            agent: Some(self.context.agent.clone()),
            space: Some(self.context.local_space.clone()),
            ..TrainingExportMetadata::default()
        }
    }

    fn helper_training_metadata_with_slot(
        &self,
        fingerprint: &str,
        slot: &str,
    ) -> TrainingExportMetadata {
        TrainingExportMetadata {
            dedupe_key: format!("{fingerprint}:{slot}"),
            ..self.helper_training_metadata(fingerprint)
        }
    }
}

fn permission_for_tool(tool: &str) -> &'static str {
    match tool {
        crate::SHELL_EXEC_TOOL => cortex_tools::HOST_SHELL_EXEC_PERMISSION,
        crate::FILESYSTEM_READ_TOOL => "host.fs.read",
        crate::MCP_LOCAL_FS_READ_TOOL => "mcp.local-fs.read_file",
        _ => "tool.invoke",
    }
}

fn permission_check_line(
    step: usize,
    request_id: &RequestId,
    tool: &str,
    pending: &PendingResponse,
) -> String {
    permission_check_line_with_decision(step, request_id, tool, pending, "allow")
}

fn permission_check_line_with_decision(
    step: usize,
    request_id: &RequestId,
    tool: &str,
    pending: &PendingResponse,
    decision: &str,
) -> String {
    format!(
        "{{\"step\":{},\"type\":\"permission_check\",\"request_id\":\"{}\",\"tool\":\"{}\",\"permission\":\"{}\",\"decision\":\"{}\",\"policy\":\"agent/helper/policy/allowed_tools\",\"fingerprint\":\"{}\"}}",
        step,
        request_id.as_str(),
        tool,
        permission_for_tool(tool),
        decision,
        pending.fingerprint,
    )
}

fn permission_check_trace_line(
    request_id: &RequestId,
    tool: &str,
    pending: &PendingResponse,
) -> String {
    permission_check_trace_line_with_decision(request_id, tool, pending, "allow")
}

fn permission_check_trace_line_with_decision(
    request_id: &RequestId,
    tool: &str,
    pending: &PendingResponse,
    decision: &str,
) -> String {
    let source = tool_submit_source(tool, request_id);
    format!(
        "{{\"agent\":\"helper\",\"thread\":\"demo\",\"request_id\":\"{}\",\"source\":{},\"event\":\"permission_check\",\"tool\":\"{}\",\"permission\":\"{}\",\"decision\":\"{}\",\"policy\":\"agent/helper/policy/allowed_tools\",\"fingerprint\":\"{}\"}}",
        request_id.as_str(),
        json_string(&source),
        tool,
        permission_for_tool(tool),
        decision,
        pending.fingerprint,
    )
}

fn tool_submit_source(tool: &str, request_id: &RequestId) -> String {
    format!("tool/{tool}/invoke/inbox/{}.req.json", request_id.as_str())
}

fn agent_task_source(agent: &str, request_id: &RequestId) -> String {
    format!("agent/{agent}/inbox/{}.req.json", request_id.as_str())
}

trait ExportFilterRow {
    fn line(&self) -> &str;
    fn time(&self) -> &str;
    fn dedupe_key(&self) -> &str;
    fn provider(&self) -> Option<&str>;
    fn model(&self) -> Option<&str>;
    fn agent(&self) -> Option<&str>;
    fn subject(&self) -> Option<&str>;
    fn space(&self) -> Option<&str>;
    fn failed(&self) -> bool;
}

impl ExportFilterRow for ConversationExportRow {
    fn line(&self) -> &str {
        &self.line
    }

    fn time(&self) -> &str {
        &self.time
    }

    fn dedupe_key(&self) -> &str {
        &self.dedupe_key
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
    fn line(&self) -> &str {
        &self.line
    }

    fn time(&self) -> &str {
        &self.time
    }

    fn dedupe_key(&self) -> &str {
        &self.dedupe_key
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

fn trace_slot(line: &str) -> &'static str {
    if line.contains("\"event\":\"permission_check\"") {
        "permission_check"
    } else if line.contains("\"event\":\"tool_call\"") {
        "tool_call"
    } else if line.contains("\"event\":\"tool_result\"") {
        "tool_result"
    } else if line.contains("\"event\":\"task_result\"") {
        "task_result"
    } else if line.contains("\"event\":\"task\"") {
        "task"
    } else {
        "trace"
    }
}

fn export_metadata_json(subject: Option<&str>) -> String {
    subject.map_or_else(String::new, |subject| {
        format!("\"subject\":{},", json_string(subject))
    })
}

fn export_subject_and_space<'a>(
    pending: &PendingResponse,
    local_space: &'a str,
    external_space: &'a str,
) -> (Option<String>, &'a str) {
    if pending.scope == SubmissionScope::ExternalThread {
        (external_subject(&pending.request_body), external_space)
    } else {
        (None, local_space)
    }
}
