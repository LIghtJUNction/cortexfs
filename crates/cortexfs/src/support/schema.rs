use serde_json::Value;

use crate::ControlLineIssue;

/// Tool schema control-file validation uses the shared control-line issue model.
pub type ToolSchemaIssue = ControlLineIssue;

/// Result of inspecting `tool/<name>.d/schema`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ToolSchemaReport {
    issues: Vec<ControlLineIssue>,
}

impl_issue_report!(ToolSchemaReport, ControlLineIssue);

/// Inspects a `tool/<name>.d/schema` file body.
#[must_use]
pub fn inspect_tool_schema_json(content: &str) -> ToolSchemaReport {
    let Ok(value) = serde_json::from_str::<Value>(content) else {
        return ToolSchemaReport::new(vec![ControlLineIssue::InvalidJson]);
    };
    let Some(object) = value.as_object() else {
        return ToolSchemaReport::new(vec![ControlLineIssue::NotObject]);
    };

    let mut issues = Vec::new();
    if !jsonschema::meta::is_valid(&value) {
        issues.push(ControlLineIssue::InvalidSchema);
    }
    issues.extend(
        object
            .keys()
            .filter(|field| is_tool_schema_authority_field(field))
            .map(|field| ControlLineIssue::AuthorityField(field.clone())),
    );
    ToolSchemaReport::new(issues)
}

pub(crate) fn is_tool_schema_authority_field(field: &str) -> bool {
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
