use crate::runtime_types::RouteMetadata;
use crate::text::{audit_cost_content, json_string};
use crate::{FILESYSTEM_READ_TOOL, MCP_LOCAL_FS_READ_TOOL, NodeContent, RuntimeState};

pub struct AuditRouteEvent<'a> {
    pub format: &'a str,
    pub name: &'a str,
    pub event: &'a str,
    pub fingerprint: Option<&'a str>,
    pub route: &'a RouteMetadata,
    pub external_subject: Option<&'a str>,
    pub space: Option<&'a str>,
}

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
        self.append_audit_route_event(&AuditRouteEvent {
            format,
            name,
            event,
            fingerprint,
            route,
            external_subject: None,
            space: None,
        });
    }

    pub fn append_audit_route_event(&mut self, audit: &AuditRouteEvent<'_>) {
        use std::fmt::Write as _;
        let context = &self.context;
        if let Some(content) = self
            .nodes
            .get_mut(&self.audit_inode)
            .and_then(|node| node.content.as_mut())
            .and_then(NodeContent::as_dynamic_mut)
        {
            let fingerprint = audit
                .fingerprint
                .map_or_else(|| "null".to_owned(), json_string);
            let external_subject = optional_json_string(audit.external_subject);
            let space = audit.space.unwrap_or(&context.local_space);
            let _ = writeln!(
                content,
                "{{\"event\":\"{}\",\"format\":\"{}\",\"name\":\"{}\",\"host_uid\":{},\"host_gid\":{},\"host_pid\":{},\"external_subject\":{},\"space\":{},\"agent\":{},\"operation\":{},\"object_class\":{},\"provider\":{},\"model\":{},\"tool\":{},\"mcp_server\":{},\"decision\":{},\"latency_ms\":0,\"input_tok\":0,\"output_tok\":0,\"cost_usd\":0,\"error\":null,\"fingerprint\":{fingerprint}}}",
                audit.event,
                audit.format,
                audit.name,
                context.host_uid,
                context.host_gid,
                context.host_pid,
                external_subject,
                json_string(space),
                json_string(&context.agent),
                json_string(operation_for_event(audit.event)),
                json_string(object_class_for_format(audit.format)),
                json_string(&audit.route.provider),
                json_string(&audit.route.model),
                optional_tool_for_format(audit.format),
                optional_mcp_server_for_format(audit.format),
                json_string(&audit.route.reason),
            );
        }
        self.record_audit_event(audit.format, audit.event);
    }

    fn append_audit_entry(
        &mut self,
        format: &str,
        name: &str,
        event: &str,
        fingerprint: Option<&str>,
    ) {
        self.append_audit_entry_with_subject(format, name, event, fingerprint, None);
    }

    fn append_audit_entry_with_subject(
        &mut self,
        format: &str,
        name: &str,
        event: &str,
        fingerprint: Option<&str>,
        external_subject: Option<&str>,
    ) {
        use std::fmt::Write as _;
        let context = &self.context;
        if let Some(content) = self
            .nodes
            .get_mut(&self.audit_inode)
            .and_then(|node| node.content.as_mut())
            .and_then(NodeContent::as_dynamic_mut)
        {
            let external_subject = optional_json_string(external_subject);
            if let Some(fingerprint) = fingerprint {
                let _ = writeln!(
                    content,
                    "{{\"event\":\"{event}\",\"format\":\"{format}\",\"name\":\"{name}\",\"host_uid\":{},\"host_gid\":{},\"host_pid\":{},\"external_subject\":{},\"space\":{},\"agent\":{},\"operation\":{},\"object_class\":{},\"provider\":null,\"model\":null,\"tool\":{},\"mcp_server\":{},\"decision\":{},\"latency_ms\":0,\"input_tok\":0,\"output_tok\":0,\"cost_usd\":0,\"error\":null,\"fingerprint\":\"{fingerprint}\"}}",
                    context.host_uid,
                    context.host_gid,
                    context.host_pid,
                    external_subject,
                    json_string(&context.local_space),
                    json_string(&context.agent),
                    json_string(operation_for_event(event)),
                    json_string(object_class_for_format(format)),
                    optional_tool_for_format(format),
                    optional_mcp_server_for_format(format),
                    json_string(decision_for_event(event)),
                );
            } else {
                let _ = writeln!(
                    content,
                    "{{\"event\":\"{event}\",\"format\":\"{format}\",\"name\":\"{name}\",\"host_uid\":{},\"host_gid\":{},\"host_pid\":{},\"external_subject\":{},\"space\":{},\"agent\":{},\"operation\":{},\"object_class\":{},\"provider\":null,\"model\":null,\"tool\":{},\"mcp_server\":{},\"decision\":{},\"latency_ms\":0,\"input_tok\":0,\"output_tok\":0,\"cost_usd\":0,\"error\":null,\"fingerprint\":null}}",
                    context.host_uid,
                    context.host_gid,
                    context.host_pid,
                    external_subject,
                    json_string(&context.local_space),
                    json_string(&context.agent),
                    json_string(operation_for_event(event)),
                    json_string(object_class_for_format(format)),
                    optional_tool_for_format(format),
                    optional_mcp_server_for_format(format),
                    json_string(decision_for_event(event)),
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
        if let Some(inode) = self.user_audit_events_inode {
            self.update_dynamic_file(inode, format!("{}\n", self.audit_total_events));
        }
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

fn optional_json_string(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_owned(), json_string)
}

fn operation_for_event(event: &str) -> &'static str {
    match event {
        "queued" => "submit",
        "drained" | "error" => "invoke",
        "denied" => "use",
        "configured" => "configure",
        "refreshed" => "inspect",
        "requested" => "rotate",
        "claimed" | "acquired" => "claim",
        _ => "write",
    }
}

fn decision_for_event(event: &str) -> &'static str {
    match event {
        "denied" => "deny",
        "error" => "error",
        _ => "allow",
    }
}

fn object_class_for_format(format: &str) -> &'static str {
    if matches!(format, FILESYSTEM_READ_TOOL | MCP_LOCAL_FS_READ_TOOL) {
        return "mcp_tool";
    }
    if format.starts_with("mcp.") {
        return "mcp_server";
    }
    if format.starts_with("provider.") {
        return "provider";
    }
    if format.starts_with("cluster.") {
        return "cluster";
    }
    if format.starts_with("memory.") {
        return "memory";
    }
    if format.starts_with("vector.") {
        return "vector_index";
    }
    if format.starts_with("database.") {
        return "database";
    }
    if format.starts_with("thread.") || format.starts_with("tool-loop.") {
        return "thread";
    }
    if format.starts_with("agent.") {
        return "agent";
    }
    if format.starts_with("collab.") {
        return "cluster";
    }
    if format == "export" || format.starts_with("export.") {
        return "audit_log";
    }
    "request"
}

fn optional_tool_for_format(format: &str) -> String {
    if matches!(format, FILESYSTEM_READ_TOOL | MCP_LOCAL_FS_READ_TOOL) {
        json_string(format)
    } else {
        "null".to_owned()
    }
}

fn optional_mcp_server_for_format(format: &str) -> String {
    if format.starts_with("mcp.") || format == MCP_LOCAL_FS_READ_TOOL {
        json_string("local-fs")
    } else {
        "null".to_owned()
    }
}
