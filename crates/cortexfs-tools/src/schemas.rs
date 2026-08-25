pub const FS_READ_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "fs.read input",
  "description": "Read one UTF-8 text file visible to the tool process.",
  "type": "object",
  "additionalProperties": false,
  "required": ["path"],
  "properties": { "path": { "type": "string" } }
}"#;

pub const FS_LIST_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "fs.list input",
  "description": "List bounded no-follow metadata for a visible directory.",
  "type": "object",
  "additionalProperties": false,
  "required": ["path"],
  "properties": {
    "path": { "type": "string" },
    "max_entries": { "type": "integer", "minimum": 1, "maximum": 256 }
  }
}"#;

pub const FS_STAT_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "fs.stat input",
  "description": "Read bounded no-follow metadata for one visible path.",
  "type": "object",
  "additionalProperties": false,
  "required": ["path"],
  "properties": { "path": { "type": "string" } }
}"#;
