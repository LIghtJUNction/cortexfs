use super::*;

pub(crate) struct ReferenceToolSpec {
    pub(crate) name: &'static str,
    pub(crate) wrapper_target: &'static str,
    pub(crate) description: &'static str,
    pub(crate) schema: &'static str,
    pub(crate) cap: &'static str,
    pub(crate) policy: &'static str,
}

pub(crate) const REFERENCE_GLOBAL_TOOLS: &[ReferenceToolSpec] = &[
    ReferenceToolSpec {
        name: "tsh",
        wrapper_target: support::command::FALSE,
        description: "CortexFS tool shell. Resolve and run tools through CTX_PATH.",
        schema: r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "tsh input",
  "description": "Run a CortexFS tool by name through CTX_PATH.",
  "type": "object",
  "additionalProperties": true
}"#,
        cap: "tsh",
        policy: "allow architect_t tool:tsh execute\nallow coder_t tool:tsh execute\nallow reviewer_t tool:tsh execute",
    },
    ReferenceToolSpec {
        name: "fs.read",
        wrapper_target: REFERENCE_OBJECT_RUNNER,
        description: "Read one UTF-8 text file from the visible filesystem.",
        schema: r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "fs.read input",
  "description": "Read a UTF-8 text file visible to the tool process.",
  "type": "object",
  "additionalProperties": false,
  "required": ["path"],
  "properties": {
    "path": { "type": "string" }
  }
}"#,
        cap: "fs.read",
        policy: "allow architect_t tool:fs.read execute\nallow coder_t tool:fs.read execute\nallow reviewer_t tool:fs.read execute",
    },
    ReferenceToolSpec {
        name: "fs.list",
        wrapper_target: REFERENCE_OBJECT_RUNNER,
        description: "List bounded no-follow metadata in a visible directory.",
        schema: tool::core::tools::FS_LIST_SCHEMA,
        cap: "fs.list",
        policy: "allow architect_t tool:fs.list execute\nallow coder_t tool:fs.list execute\nallow reviewer_t tool:fs.list execute\nallow worker_t tool:fs.list execute",
    },
    ReferenceToolSpec {
        name: "fs.stat",
        wrapper_target: REFERENCE_OBJECT_RUNNER,
        description: "Read bounded no-follow metadata for a visible path.",
        schema: tool::core::tools::FS_STAT_SCHEMA,
        cap: "fs.stat",
        policy: "allow architect_t tool:fs.stat execute\nallow coder_t tool:fs.stat execute\nallow reviewer_t tool:fs.stat execute\nallow worker_t tool:fs.stat execute",
    },
    ReferenceToolSpec {
        name: "fs.write",
        wrapper_target: REFERENCE_OBJECT_RUNNER,
        description: "Atomically write UTF-8 text to a visible filesystem path.",
        schema: r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "fs.write input",
  "description": "Write UTF-8 text to a path visible to the tool process.",
  "type": "object",
  "additionalProperties": false,
  "required": ["path", "content"],
  "properties": {
    "path": { "type": "string" },
    "content": { "type": "string" }
  }
}"#,
        cap: "fs.write",
        policy: "allow coder_t tool:fs.write execute",
    },
    ReferenceToolSpec {
        name: "fs.replace",
        wrapper_target: REFERENCE_OBJECT_RUNNER,
        description: "Replace exactly one UTF-8 text span in a visible file.",
        schema: r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "fs.replace input",
  "description": "Replace exactly one UTF-8 text span in a visible file.",
  "type": "object",
  "additionalProperties": false,
  "required": ["path", "old", "new"],
  "properties": {
    "path": { "type": "string" },
    "old": { "type": "string" },
    "new": { "type": "string" }
  }
}"#,
        cap: "fs.replace",
        policy: "allow coder_t tool:fs.replace execute",
    },
    ReferenceToolSpec {
        name: "shell.exec",
        wrapper_target: REFERENCE_OBJECT_RUNNER,
        description: "Run one bounded shell command in the visible workspace.",
        schema: r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "shell.exec input",
  "description": "Run one shell command with sh -c.",
  "type": "object",
  "additionalProperties": false,
  "required": ["cmd"],
  "properties": {
    "cmd": { "type": "string" }
  }
}"#,
        cap: "shell.exec",
        policy: "allow coder_t tool:shell.exec execute",
    },
    ReferenceToolSpec {
        name: "tsh.config",
        wrapper_target: REFERENCE_OBJECT_RUNNER,
        description: "Read or update persistent tsh runtime configuration.",
        schema: r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "tsh.config input",
  "description": "Read or update tsh.d/config.",
  "type": "object",
  "additionalProperties": true
}"#,
        cap: "tsh.config",
        policy: "allow architect_t tool:tsh.config execute\nallow coder_t tool:tsh.config execute\nallow reviewer_t tool:tsh.config execute",
    },
    ReferenceToolSpec {
        name: "agent.create",
        wrapper_target: REFERENCE_OBJECT_RUNNER,
        description: "Create one explicitly authorized owned child agent.",
        schema: agent::createop::AGENT_CREATE_SCHEMA,
        cap: "agent.create",
        policy: "allow architect_t tool:agent.create execute\nallow coder_t tool:agent.create execute\nallow reviewer_t tool:agent.create execute",
    },
    ReferenceToolSpec {
        name: "agent.update",
        wrapper_target: REFERENCE_OBJECT_RUNNER,
        description: "Replace one prompt control of the calling agent itself.",
        schema: agent::updateop::AGENT_UPDATE_SCHEMA,
        cap: "agent.update",
        policy: "allow architect_t tool:agent.update execute\nallow coder_t tool:agent.update execute\nallow reviewer_t tool:agent.update execute\nallow worker_t tool:agent.update execute",
    },
];

pub(crate) const DEFAULT_TSH_CONFIG: &str = "\
max_loaded_tools=64
cache_capacity=32
window_percent=1
";

pub(crate) fn reference_tool_stub_script(name: &str) -> Option<&'static str> {
    match name {
        "tsh" => Some(reference_exec_named_tool_script("tsh")),
        _ => None,
    }
}

pub(crate) fn reference_exec_named_tool_script(name: &'static str) -> &'static str {
    match name {
        "tsh" => {
            r#"#!/bin/sh
# CortexFS reference-tree tsh tool.
exec /ctx/bin/tsh "$@"
"#
        }
        _ => "",
    }
}
