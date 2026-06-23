use serde_json::Value;

/// Tool schema control-file validation issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolSchemaIssue {
    /// `tool/<name>.d/schema` is not valid JSON.
    InvalidJson,
    /// Schema is valid JSON but not an object.
    NotObject,
    /// Schema is an object but not a valid JSON Schema document.
    InvalidSchema,
    /// Top-level field tries to describe authority instead of input/output.
    AuthorityField(String),
}

/// Result of inspecting `tool/<name>.d/schema`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ToolSchemaReport {
    issues: Vec<ToolSchemaIssue>,
}

impl_issue_report!(ToolSchemaReport, ToolSchemaIssue);

/// Inspects a `tool/<name>.d/schema` file body.
#[must_use]
pub fn inspect_tool_schema_json(content: &str) -> ToolSchemaReport {
    let Ok(value) = serde_json::from_str::<Value>(content) else {
        return ToolSchemaReport::new(vec![ToolSchemaIssue::InvalidJson]);
    };
    let Some(object) = value.as_object() else {
        return ToolSchemaReport::new(vec![ToolSchemaIssue::NotObject]);
    };

    let mut issues = Vec::new();
    if !jsonschema::meta::is_valid(&value) {
        issues.push(ToolSchemaIssue::InvalidSchema);
    }
    issues.extend(
        object
            .keys()
            .filter(|field| is_tool_schema_authority_field(field))
            .map(|field| ToolSchemaIssue::AuthorityField(field.clone())),
    );
    ToolSchemaReport::new(issues)
}

fn is_tool_schema_authority_field(field: &str) -> bool {
    matches!(
        field,
        "policy"
            | "allow"
            | "deny"
            | "authority"
            | "grant"
            | "grants"
            | "permissions"
            | "capability_grants"
            | "mount"
            | "uid"
            | "gid"
            | "groups"
            | "network"
    )
}
