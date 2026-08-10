use super::manifest::{PackageAgent, PackageDocument};
use crate::{CliError, is_object_name, parse_agent_parent_ref, require_cli_name};
use semver::Version;
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
    if let Some(version) = document.version.as_deref() {
        Version::parse(version)
            .map_err(|error| CliError::usage(format!("invalid package version: {error}")))?;
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
        if let Some(schema) = tool.schema.as_ref()
            && !schema.is_object()
        {
            return Err(CliError::usage(format!(
                "tool schema must be a TOML table: {}",
                tool.name
            )));
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
    if !is_object_name(&format!("{}_t", agent.name)) {
        return Err(CliError::usage(format!(
            "agent name is too long for its policy subject: {}",
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
    if let Some(identity) = agent.identity.as_ref() {
        let mut groups = std::collections::BTreeSet::new();
        if identity.groups.iter().any(|group| !groups.insert(group)) {
            return Err(CliError::usage(format!(
                "duplicate agent identity group: {}",
                agent.name
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
