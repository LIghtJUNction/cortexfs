use std::path::{Path, PathBuf};

use crate::{agent_root_path, bin_root_path, model_root_path, tool_root_path};

#[must_use]
pub fn object_path(root: &Path, class: &str, name: &str) -> PathBuf {
    object_root_path(root, class).join(name)
}

#[must_use]
pub fn object_root_path(root: &Path, class: &str) -> PathBuf {
    root.join(class)
}

#[must_use]
pub fn object_control_path(root: &Path, class: &str, name: &str) -> PathBuf {
    root.join(class).join(format!("{name}.d"))
}

#[must_use]
pub fn object_control_file_path(root: &Path, class: &str, name: &str, file: &str) -> PathBuf {
    object_control_path(root, class, name).join(file)
}

#[must_use]
pub fn control_file_path(control: &Path, file: &str) -> PathBuf {
    control.join(file)
}

#[must_use]
pub fn object_socket_path(root: &Path, class: &str, name: &str) -> PathBuf {
    object_root_path(root, class).join(format!("{name}.sock"))
}

#[must_use]
pub fn model_path(root: &Path, provider: &str, model: &str) -> PathBuf {
    model_provider_path(root, provider).join(model)
}

#[must_use]
pub fn model_provider_path(root: &Path, provider: &str) -> PathBuf {
    model_root_path(root).join(provider)
}

#[must_use]
pub fn model_reference_path(root: &Path, reference: &str) -> Option<PathBuf> {
    let (provider, model) = reference.split_once('/')?;
    (!provider.is_empty() && !model.is_empty()).then(|| model_path(root, provider, model))
}

#[must_use]
pub fn model_control_path(root: &Path, provider: &str, model: &str) -> PathBuf {
    model_provider_path(root, provider).join(format!("{model}.d"))
}

#[must_use]
pub fn model_control_file_path(root: &Path, provider: &str, model: &str, file: &str) -> PathBuf {
    model_control_path(root, provider, model).join(file)
}

#[must_use]
pub fn model_socket_path(root: &Path, provider: &str, model: &str) -> PathBuf {
    model_provider_path(root, provider).join(format!("{model}.sock"))
}

#[must_use]
pub fn model_route_path(root: &Path) -> PathBuf {
    model_root_path(root).join("route")
}

#[must_use]
pub fn agent_path(root: &Path, agent: &str) -> PathBuf {
    agent_root_path(root).join(agent)
}

#[must_use]
pub fn agent_control_path(root: &Path, agent: &str) -> PathBuf {
    agent_root_path(root).join(format!("{agent}.d"))
}

#[must_use]
pub fn agent_control_file_path(root: &Path, agent: &str, file: &str) -> PathBuf {
    agent_control_path(root, agent).join(file)
}

#[must_use]
pub fn agent_socket_path(root: &Path, agent: &str) -> PathBuf {
    agent_root_path(root).join(format!("{agent}.sock"))
}

#[must_use]
pub fn tool_path(root: &Path, tool: &str) -> PathBuf {
    tool_root_path(root).join(tool)
}

#[must_use]
pub fn tool_control_path(root: &Path, tool: &str) -> PathBuf {
    tool_root_path(root).join(format!("{tool}.d"))
}

#[must_use]
pub fn tool_control_file_path(root: &Path, tool: &str, file: &str) -> PathBuf {
    tool_control_path(root, tool).join(file)
}

#[must_use]
pub fn tool_config_path(root: &Path) -> PathBuf {
    tool_root_path(root).join("tsh.d").join("config")
}

#[must_use]
pub fn object_runner_path(root: &Path) -> PathBuf {
    bin_root_path(root).join("cortexfs-object-runner")
}
