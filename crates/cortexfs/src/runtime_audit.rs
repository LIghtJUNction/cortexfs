use crate::runtime_types::RouteMetadata;
use crate::text::{audit_cost_content, json_string};
use crate::{FILESYSTEM_READ_TOOL, MCP_LOCAL_FS_READ_TOOL, NodeContent, RuntimeState};

impl RuntimeState {
    pub fn append_audit(&mut self, format: &str, name: &str, event: &str) {
        self.append_audit_entry(format, name, event, None);
    }

    pub fn append_audit_with_fingerprint(
        &mut self,
        format: &str,
        name: &str,
        event: &str,
        fingerprint: &str,
    ) {
        self.append_audit_entry(format, name, event, Some(fingerprint));
    }

    pub fn append_audit_with_route(
        &mut self,
        format: &str,
        name: &str,
        event: &str,
        fingerprint: Option<&str>,
        route: &RouteMetadata,
    ) {
        use std::fmt::Write as _;
        if let Some(content) = self
            .nodes
            .get_mut(&self.audit_inode)
            .and_then(|node| node.content.as_mut())
            .and_then(NodeContent::as_dynamic_mut)
        {
            let fingerprint = fingerprint.map_or_else(|| "null".to_owned(), json_string);
            let _ = writeln!(
                content,
                "{{\"event\":\"{event}\",\"format\":\"{format}\",\"name\":\"{name}\",\"fingerprint\":{fingerprint},\"provider\":{},\"model\":{},\"decision\":{}}}",
                json_string(&route.provider),
                json_string(&route.model),
                json_string(&route.reason),
            );
        }
        self.record_audit_event(format, event);
    }

    fn append_audit_entry(
        &mut self,
        format: &str,
        name: &str,
        event: &str,
        fingerprint: Option<&str>,
    ) {
        use std::fmt::Write as _;
        if let Some(content) = self
            .nodes
            .get_mut(&self.audit_inode)
            .and_then(|node| node.content.as_mut())
            .and_then(NodeContent::as_dynamic_mut)
        {
            if let Some(fingerprint) = fingerprint {
                let _ = writeln!(
                    content,
                    "{{\"event\":\"{event}\",\"format\":\"{format}\",\"name\":\"{name}\",\"fingerprint\":\"{fingerprint}\"}}"
                );
            } else {
                let _ = writeln!(
                    content,
                    "{{\"event\":\"{event}\",\"format\":\"{format}\",\"name\":\"{name}\"}}"
                );
            }
        }
        self.record_audit_event(format, event);
    }

    fn record_audit_event(&mut self, format: &str, event: &str) {
        self.audit_total_events = self.audit_total_events.saturating_add(1);
        match event {
            "staged" => self.audit_staged_events = self.audit_staged_events.saturating_add(1),
            "queued" => self.audit_queued_events = self.audit_queued_events.saturating_add(1),
            "drained" => {
                self.audit_drained_events = self.audit_drained_events.saturating_add(1);
                self.record_billable_audit_event(format);
            }
            "error" => self.audit_error_events = self.audit_error_events.saturating_add(1),
            "denied" => self.audit_denied_events = self.audit_denied_events.saturating_add(1),
            _other => {}
        }
        self.refresh_audit_usage();
        self.refresh_audit_cost();
    }

    fn record_billable_audit_event(&mut self, format: &str) {
        self.audit_billable_events = self.audit_billable_events.saturating_add(1);
        if format == "agent.task" {
            self.audit_agent_tasks = self.audit_agent_tasks.saturating_add(1);
        }
        if matches!(format, FILESYSTEM_READ_TOOL | MCP_LOCAL_FS_READ_TOOL) {
            self.audit_tool_calls = self.audit_tool_calls.saturating_add(1);
        }
    }

    pub fn refresh_audit_usage(&mut self) {
        let content = format!(
            "events={}\nstaged={}\nqueued={}\ndrained={}\nerrors={}\ndenied={}\n",
            self.audit_total_events,
            self.audit_staged_events,
            self.audit_queued_events,
            self.audit_drained_events,
            self.audit_error_events,
            self.audit_denied_events,
        );
        self.update_dynamic_file(self.audit_usage_inode, content);
    }

    fn refresh_audit_cost(&mut self) {
        let content = audit_cost_content(
            self.audit_billable_events,
            self.audit_tool_calls,
            self.audit_agent_tasks,
        );
        self.update_dynamic_file(self.audit_cost_inode, content);
    }
}
