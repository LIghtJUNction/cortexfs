use super::manifest::{PackageAgent, PackageDocument};
use crate::*;
use std::path::{Component, Path};

pub(super) fn validate_package(document: &PackageDocument) -> Result<(), CliError> {
    if let Some(schema) = document.schema.as_deref()
        && !schema.is_empty()
        && schema != super::manifest::PACKAGE_SCHEMA
    {
        return Err(CliError::usage(format!(
            "unsupported package schema: {schema} (expected {})",
            super::manifest::PACKAGE_SCHEMA
        )));
    }
    if let Some(name) = document.name.as_deref()
        && !name.is_empty()
    {
        require_cli_name("package name", name)?;
    }
    if document.tools.is_empty() && document.agents.is_empty() {
        return Err(CliError::usage(
            "package must declare at least one tool or agent",
        ));
    }
    let mut names = std::collections::BTreeSet::<String>::new();
    for tool in &document.tools {
        validate_member(&tool.name, &tool.run, "tool")?;
        if !names.insert(format!("tool:{}", tool.name)) {
            return Err(CliError::usage(format!(
                "duplicate package tool: {}",
                tool.name
            )));
        }
        if let Some(schema) = tool.schema.as_deref() {
            let value: serde_json::Value = serde_json::from_str(schema).map_err(|error| {
                CliError::usage(format!("invalid schema for {}: {error}", tool.name))
            })?;
            if !value.is_object() {
                return Err(CliError::usage(format!(
                    "tool schema must be a JSON object: {}",
                    tool.name
                )));
            }
        }
    }
    for agent in &document.agents {
        validate_agent(agent, &mut names)?;
    }
    Ok(())
}

fn validate_agent(
    agent: &PackageAgent,
    names: &mut std::collections::BTreeSet<String>,
) -> Result<(), CliError> {
    validate_member(&agent.name, &agent.run, "agent")?;
    if !names.insert(format!("agent:{}", agent.name)) {
        return Err(CliError::usage(format!(
            "duplicate package agent: {}",
            agent.name
        )));
    }
    if let Some(parent) = agent.parent.as_deref()
        && parse_agent_parent_ref(parent).is_none()
    {
        return Err(CliError::usage(format!("invalid agent parent: {parent}")));
    }
    let mut tools = std::collections::BTreeSet::new();
    for tool in &agent.tools {
        require_cli_name("agent tool", tool)?;
        if tool == "tsh" || !tools.insert(tool) {
            return Err(CliError::usage(format!(
                "invalid or duplicate agent tool: {tool}"
            )));
        }
    }
    Ok(())
}

fn validate_member(name: &str, run: &Path, class: &str) -> Result<(), CliError> {
    require_cli_name(class, name)?;
    if run.as_os_str().is_empty()
        || run.is_absolute()
        || run.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::CurDir
            )
        })
    {
        return Err(CliError::usage(format!(
            "{class} {name} run must be a relative package path"
        )));
    }
    Ok(())
}
