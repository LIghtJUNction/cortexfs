pub(crate) const FS_READ_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "fs.read input",
  "description": "Read one UTF-8 text file visible to the tool process.",
  "type": "object",
  "additionalProperties": false,
  "required": ["path"],
  "properties": {
    "path": {
      "type": "string",
      "description": "Path to a UTF-8 text file visible to the tool process."
    }
  }
}"#;

pub(crate) const FS_WRITE_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "fs.write input",
  "description": "Write UTF-8 text to one path visible to the tool process.",
  "type": "object",
  "additionalProperties": false,
  "required": ["path", "content"],
  "properties": {
    "path": {
      "type": "string",
      "description": "Path to write."
    },
    "content": {
      "type": "string",
      "description": "UTF-8 content to write."
    }
  }
}"#;

pub(crate) const FS_REPLACE_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "fs.replace input",
  "description": "Replace exactly one UTF-8 text span in one visible file.",
  "type": "object",
  "additionalProperties": false,
  "required": ["path", "old", "new"],
  "properties": {
    "path": {
      "type": "string",
      "description": "Path to edit."
    },
    "old": {
      "type": "string",
      "description": "Existing UTF-8 text span. It must occur exactly once."
    },
    "new": {
      "type": "string",
      "description": "Replacement UTF-8 text."
    }
  }
}"#;

pub(crate) const SHELL_EXEC_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "shell.exec input",
  "description": "Run one shell command in the tool process environment.",
  "type": "object",
  "additionalProperties": false,
  "required": ["cmd"],
  "properties": {
    "cmd": {
      "type": "string",
      "description": "Command line passed to sh -c."
    }
  }
}"#;

pub(crate) const TSH_CONFIG_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "tsh.config input",
  "description": "Read or update tsh.d/config. Omit all fields to show the current config.",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "path": {
      "type": "string",
      "description": "Optional config path. If supplied, it must equal CTX_ROOT/tool/tsh.d/config or /ctx/tool/tsh.d/config."
    },
    "max_loaded_tools": {
      "type": "integer",
      "minimum": 1,
      "maximum": 1024,
      "description": "Maximum unpinned tool metadata entries kept in the tsh context."
    },
    "cache_capacity": {
      "type": "integer",
      "minimum": 1,
      "maximum": 1024,
      "description": "Maximum unpinned dynamic tool artifacts kept resident by W-TinyLFU."
    },
    "window_percent": {
      "type": "integer",
      "minimum": 1,
      "maximum": 100,
      "description": "Percentage of the dynamic cache used as the W-TinyLFU admission window."
    }
  }
}"#;
