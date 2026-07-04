struct ReferenceToolSpec {
    name: &'static str,
    wrapper_target: &'static str,
    description: &'static str,
    schema: &'static str,
    cap: &'static str,
    policy: &'static str,
}

const REFERENCE_GLOBAL_TOOLS: &[ReferenceToolSpec] = &[
    ReferenceToolSpec {
        name: "tsh",
        wrapper_target: "/bin/false",
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
];

const DEFAULT_TSH_CONFIG: &str = "\
max_loaded_tools=64
cache_capacity=32
window_percent=1
";

const DEPRECATED_REFERENCE_PLACEHOLDER_TOOLS: &[&str] = &[
    "mcp.github.search_issues",
    "agent.create",
    "agent.start",
    "agent.stop",
];

fn reference_tool_stub_script(name: &str) -> Option<&'static str> {
    match name {
        "tsh" => Some(reference_exec_named_tool_script("tsh")),
        _ => None,
    }
}

fn reference_exec_named_tool_script(name: &'static str) -> &'static str {
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
