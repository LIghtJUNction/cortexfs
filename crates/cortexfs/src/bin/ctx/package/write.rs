use super::control::agent_controls;
use super::manifest::{Package, PackageTool};
use super::object::write_manifest;
use crate::*;
use cortexfs::object::install::InstallTier;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub(super) fn write_manifests(package: &Package, staging: &Path) -> Result<Vec<PathBuf>, CliError> {
    let policies = tool_policies(package);
    let tools = package.document.tools.iter().map(|tool| {
        let controls = tool_controls(tool, policies.get(&tool.name));
        write_manifest(
            staging,
            "tool",
            &tool.name,
            &package.root.join(&tool.run),
            &controls,
        )
    });
    let agents = package.document.agents.iter().map(|agent| {
        let controls = agent_controls(agent)?;
        write_manifest(
            staging,
            "agent",
            &agent.name,
            &package.root.join(&agent.run),
            &controls,
        )
    });
    tools.chain(agents).collect()
}

pub(super) fn ensure_targets_absent(
    source: &Path,
    package: &Package,
    tier: InstallTier,
) -> Result<(), CliError> {
    let uid = nix::unistd::Uid::effective().as_raw().to_string();
    let class_path = |class: &str| match tier {
        InstallTier::User => source.join("home").join(&uid).join(class),
        InstallTier::System => source.join(class),
    };
    for (class, name) in package
        .document
        .tools
        .iter()
        .map(|object| ("tool", object.name.as_str()))
        .chain(
            package
                .document
                .agents
                .iter()
                .map(|object| ("agent", object.name.as_str())),
        )
    {
        let directory = class_path(class);
        for path in [directory.join(name), directory.join(format!("{name}.d"))] {
            match fs::symlink_metadata(&path) {
                Ok(_metadata) => {
                    return Err(CliError::unavailable(format!(
                        "package object already exists: {}",
                        path.display()
                    )));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(CliError::unavailable(format!(
                        "cannot inspect package target {}: {error}",
                        path.display()
                    )));
                }
            }
        }
    }
    Ok(())
}

fn tool_policies(package: &Package) -> BTreeMap<String, String> {
    package
        .document
        .tools
        .iter()
        .map(|tool| {
            let policy = tool.policy.clone().unwrap_or_else(|| {
                package
                    .document
                    .agents
                    .iter()
                    .filter(|agent| agent.tools.iter().any(|name| name == &tool.name))
                    .map(|agent| format!("allow {}_t tool:{} execute", agent.name, tool.name))
                    .collect::<Vec<_>>()
                    .join("\n")
            });
            (tool.name.clone(), policy)
        })
        .collect()
}

fn tool_controls(tool: &PackageTool, policy: Option<&String>) -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "description".to_owned(),
            tool.description
                .clone()
                .unwrap_or_else(|| tool.name.clone()),
        ),
        (
            "schema".to_owned(),
            tool.schema.clone().unwrap_or_else(|| "{}".to_owned()),
        ),
        (
            "cap".to_owned(),
            tool.cap.clone().unwrap_or_else(|| "text".to_owned()),
        ),
        ("policy".to_owned(), policy.cloned().unwrap_or_default()),
    ])
}
