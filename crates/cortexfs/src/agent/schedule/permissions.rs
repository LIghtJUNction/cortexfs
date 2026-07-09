use super::dependencies::{required_permission_object_name, required_word};
use super::*;
use crate::*;

pub(crate) fn inspect_string_array(
    node: Option<&String>,
    field: &str,
    value: Option<&Value>,
    issues: &mut Vec<AgentScheduleIssue>,
) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    let Some(values) = value.as_array() else {
        issues.push(AgentScheduleIssue::InvalidField {
            node: node.cloned(),
            field: field.to_owned(),
            value: value.to_string(),
        });
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in values {
        let Some(value) = item.as_str() else {
            issues.push(AgentScheduleIssue::InvalidField {
                node: node.cloned(),
                field: field.to_owned(),
                value: item.to_string(),
            });
            continue;
        };
        if !is_object_name(value) {
            issues.push(AgentScheduleIssue::InvalidField {
                node: node.cloned(),
                field: field.to_owned(),
                value: value.to_owned(),
            });
            continue;
        }
        out.push(value.to_owned());
    }
    out
}

pub(crate) fn requires_permission(
    value: Option<&Value>,
    expected_class: PolicyObjectClass,
    expected_name: &str,
    expected_permission: &str,
) -> bool {
    let Some(values) = value.and_then(Value::as_array) else {
        return false;
    };
    values.iter().any(|value| {
        let Ok(permission) = serde_json::from_value::<SchedulePermissionJson>(value.clone()) else {
            return false;
        };
        let Some(class_name) = permission.class.as_ref().and_then(Value::as_str) else {
            return false;
        };
        PolicyObjectClass::parse(class_name) == Some(expected_class)
            && permission.name.as_ref().and_then(Value::as_str) == Some(expected_name)
            && permission.permission.as_ref().and_then(Value::as_str) == Some(expected_permission)
    })
}

pub(crate) fn inspect_required_permissions(
    node: &str,
    value: Option<&Value>,
    parent_subject: &str,
    parent_policy: &PolicyV0,
    issues: &mut Vec<AgentScheduleIssue>,
) {
    let Some(value) = value else {
        return;
    };
    let Some(values) = value.as_array() else {
        issues.push(AgentScheduleIssue::InvalidField {
            node: Some(node.to_owned()),
            field: "requires".to_owned(),
            value: value.to_string(),
        });
        return;
    };
    for value in values {
        inspect_required_permission(node, value, parent_subject, parent_policy, issues);
    }
}

pub(crate) fn inspect_required_permission(
    node: &str,
    value: &Value,
    parent_subject: &str,
    parent_policy: &PolicyV0,
    issues: &mut Vec<AgentScheduleIssue>,
) {
    let Ok(permission) = serde_json::from_value::<SchedulePermissionJson>(value.clone()) else {
        issues.push(AgentScheduleIssue::InvalidField {
            node: Some(node.to_owned()),
            field: "requires".to_owned(),
            value: value.to_string(),
        });
        return;
    };
    let Some(class_name) = required_word(
        Some(&node.to_owned()),
        "requires.class",
        permission.class.as_ref(),
        issues,
    ) else {
        return;
    };
    let Some(class) = PolicyObjectClass::parse(&class_name) else {
        issues.push(AgentScheduleIssue::InvalidField {
            node: Some(node.to_owned()),
            field: "requires.class".to_owned(),
            value: class_name,
        });
        return;
    };
    let Some(name) = required_permission_object_name(
        Some(&node.to_owned()),
        "requires.name",
        permission.name.as_ref(),
        class,
        issues,
    ) else {
        return;
    };
    let Some(permission_name) = required_word(
        Some(&node.to_owned()),
        "requires.permission",
        permission.permission.as_ref(),
        issues,
    ) else {
        return;
    };
    let Some(permission) = PolicyPermission::parse_for_class(class, &permission_name) else {
        issues.push(AgentScheduleIssue::InvalidField {
            node: Some(node.to_owned()),
            field: "requires.permission".to_owned(),
            value: permission_name,
        });
        return;
    };
    if !parent_policy.allows(parent_subject, class, &name, permission) {
        issues.push(AgentScheduleIssue::PermissionNotGranted {
            node: node.to_owned(),
            class: class_name,
            name,
            permission: permission_name,
        });
    }
}
