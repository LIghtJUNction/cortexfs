pub const FS_WRITE_SCHEMA: &str = r#"{
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
}"#;

pub const FS_REPLACE_SCHEMA: &str = r#"{
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
}"#;

pub const SHELL_EXEC_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "shell.exec input",
  "description": "Run one shell command in the tool process environment.",
  "type": "object",
  "additionalProperties": false,
  "required": ["cmd"],
  "properties": { "cmd": { "type": "string" } }
}"#;

pub const TSH_CONFIG_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "tsh.config input",
  "description": "Read or update tsh.d/config. Omit all fields to show the current config.",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "path": { "type": "string" },
    "max_loaded_tools": { "type": "integer", "minimum": 1, "maximum": 1024 },
    "cache_capacity": { "type": "integer", "minimum": 1, "maximum": 1024 },
    "window_percent": { "type": "integer", "minimum": 1, "maximum": 100 }
  }
}"#;
