use crate::*;
use cortexfs_runtime_client::agent::{AGENT_LAUNCH_ABI, is_agent_launch_abi};

/// Installs an executable object wrapper plus required `.d` control files.
///
/// The wrapper is a small POSIX shell `exec` shim to an existing runtime,
/// script, or tool command. This helper does not start sockets, providers, or
/// supervisors; it only creates stable filesystem ABI entries that ordinary
/// runtimes can execute.
pub fn install_executable_object_wrapper(
    root: &Path,
    class: ObjectClass,
    name: &str,
    wrapper_target: &str,
    control_overrides: &[(&str, &str)],
) -> Result<ObjectBootstrap, ObjectBootstrapError> {
    if !is_object_name_for_class(class, name) {
        return Err(ObjectBootstrapError::InvalidObjectName);
    }
    if !is_valid_wrapper_target(wrapper_target) {
        return Err(ObjectBootstrapError::InvalidWrapperTarget);
    }
    validate_control_overrides(class, control_overrides)?;

    let class_dir = root.join(class.as_str());
    let control_dir = class_dir.join(format!("{name}.d"));
    support::plain::create_plain_dir(&control_dir)
        .map_err(|_error| ObjectBootstrapError::CannotCreate)?;

    let executable = class_dir.join(name);
    let wrapper = executable_wrapper_script(class, name, wrapper_target);
    atomic_replace_text(&executable, &wrapper)
        .map_err(|_error| ObjectBootstrapError::CannotRecord)?;
    set_executable_mode(&executable)?;

    install_object_control_files(&control_dir, class, name, control_overrides)?;
    if class != ObjectClass::Model {
        ensure_object_hook_dirs(&control_dir)?;
    }

    Ok(ObjectBootstrap::new(executable, control_dir))
}

pub(crate) fn install_object_control_files(
    control_dir: &Path,
    class: ObjectClass,
    name: &str,
    control_overrides: &[(&str, &str)],
) -> Result<(), ObjectBootstrapError> {
    validate_control_overrides(class, control_overrides)?;
    for file in control_files_for(class) {
        let content = object_control_content(class, name, file, control_overrides)?;
        atomic_replace_text_with_mode(&control_dir.join(file), &content, 0o644)
            .map_err(|_error| ObjectBootstrapError::CannotRecord)?;
    }
    if class == ObjectClass::Agent {
        for file in AGENT_OPTIONAL_CONTROL_FILES {
            if AGENT_CONTROL_FILES.contains(file) {
                continue;
            }
            if let Some(value) = control_overrides
                .iter()
                .find_map(|&(name, value)| (name == *file).then_some(value))
            {
                let content = ensure_trailing_newline(value);
                atomic_replace_text_with_mode(&control_dir.join(file), &content, 0o644)
                    .map_err(|_error| ObjectBootstrapError::CannotRecord)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn ensure_object_hook_dirs(control_dir: &Path) -> Result<(), ObjectBootstrapError> {
    let hook_dir = control_dir.join(OBJECT_HOOK_DIR);
    support::plain::create_plain_dir(&hook_dir)
        .map_err(|_error| ObjectBootstrapError::CannotCreate)?;
    for phase in OBJECT_HOOK_PHASE_DIRS {
        support::plain::create_plain_dir(&hook_dir.join(phase))
            .map_err(|_error| ObjectBootstrapError::CannotCreate)?;
    }
    Ok(())
}

pub(crate) fn validate_control_overrides(
    class: ObjectClass,
    control_overrides: &[(&str, &str)],
) -> Result<(), ObjectBootstrapError> {
    for (file, value) in control_overrides.iter().copied() {
        if !(control_files_for(class).contains(&file)
            || class == ObjectClass::Agent && AGENT_OPTIONAL_CONTROL_FILES.contains(&file))
        {
            return Err(ObjectBootstrapError::InvalidControlFile);
        }
        validate_object_control_content(class, file, &ensure_trailing_newline(value))?;
    }
    Ok(())
}

pub(crate) fn object_control_content(
    class: ObjectClass,
    object_name: &str,
    file: &str,
    control_overrides: &[(&str, &str)],
) -> Result<String, ObjectBootstrapError> {
    let value = control_overrides
        .iter()
        .copied()
        .find_map(|(override_file, value)| (override_file == file).then_some(value))
        .map_or_else(
            || default_object_control_value(class, object_name, file),
            ToOwned::to_owned,
        );
    let content = ensure_trailing_newline(&value);
    validate_object_control_content(class, file, &content)?;
    Ok(content)
}

pub fn validate_object_control_content(
    class: ObjectClass,
    file: &str,
    content: &str,
) -> Result<(), ObjectBootstrapError> {
    match class {
        ObjectClass::Model => validate_model_control_content(file, content),
        ObjectClass::Agent => validate_agent_bootstrap_control_content(file, content),
        ObjectClass::Tool => validate_tool_control_content(file, content),
    }
}

pub(crate) fn validate_model_control_content(
    file: &str,
    content: &str,
) -> Result<(), ObjectBootstrapError> {
    match file {
        "cap" if inspect_model_capabilities(content).is_ok() => Ok(()),
        "driver" if parse_model_driver_routes(content).is_ok() => Ok(()),
        "effort" if ModelEffort::parse(content).is_some() => Ok(()),
        "fallback" if parse_model_fallback(content).1.is_ok() => Ok(()),
        "session" if matches!(content.trim(), "none" | "socket") => Ok(()),
        "cap" | "driver" | "effort" | "fallback" | "session" => {
            Err(ObjectBootstrapError::InvalidControlValue)
        }
        _ if !content.contains('\0') => Ok(()),
        _ => Err(ObjectBootstrapError::InvalidControlValue),
    }
}

pub(crate) fn validate_agent_bootstrap_control_content(
    file: &str,
    content: &str,
) -> Result<(), ObjectBootstrapError> {
    let valid = match file {
        "abi" => is_agent_launch_abi(content),
        "tools" => inspect_agent_tools_control(content).is_ok(),
        "meta.json" => serde_json::from_str::<Value>(content).is_ok_and(|value| value.is_object()),
        "system.md" | "prompt.template.md" => !content.contains('\0'),
        _ if content.contains('\0') => false,
        _ => {
            let Some(kind) = AgentControlKind::parse(file) else {
                return Ok(());
            };
            inspect_agent_control(kind, content).is_ok()
        }
    };
    valid
        .then_some(())
        .ok_or(ObjectBootstrapError::InvalidControlValue)
}

pub(crate) fn validate_tool_control_content(
    file: &str,
    content: &str,
) -> Result<(), ObjectBootstrapError> {
    match file {
        "schema" if inspect_tool_schema_json(content).is_ok() => Ok(()),
        "mcp" if object::mcp::validate_locator(content) => Ok(()),
        "schema" | "mcp" => Err(ObjectBootstrapError::InvalidControlValue),
        _ if !content.contains('\0') => Ok(()),
        _ => Err(ObjectBootstrapError::InvalidControlValue),
    }
}

pub(crate) fn default_object_control_value(
    class: ObjectClass,
    object_name: &str,
    file: &str,
) -> String {
    match class {
        ObjectClass::Model => default_model_control_value(object_name, file),
        ObjectClass::Agent => default_agent_control_value(object_name, file),
        ObjectClass::Tool => default_tool_control_value(object_name, file),
    }
}

pub(crate) fn default_model_control_value(object_name: &str, file: &str) -> String {
    match file {
        "id" => object_name.to_owned(),
        "driver" => "default=openai-chat".to_owned(),
        "cap" => "chat\nstream".to_owned(),
        "effort" => ModelEffort::Auto.as_control_value().to_owned(),
        "fallback" => "\n".to_owned(),
        "session" => "none".to_owned(),
        "status" => "idle".to_owned(),
        _ => String::new(),
    }
}

pub(crate) fn default_agent_control_value(object_name: &str, file: &str) -> String {
    match file {
        "abi" => AGENT_LAUNCH_ABI.to_owned(),
        "owner" | "uid" | "gid" => "0".to_owned(),
        "label" => format!("user_u:agent_r:{object_name}_t:s0"),
        "iso" => "shared".to_owned(),
        "life" => "owned".to_owned(),
        "root" | "cwd" => "/".to_owned(),
        "env" => "CTX_ROOT=/ctx".to_owned(),
        "path" => "/ctx/tool".to_owned(),
        "mount" => "/ctx\t/ctx\tro\trbind,nosuid,nodev".to_owned(),
        "window" => "auto".to_owned(),
        "status" => "idle".to_owned(),
        "system.md" => format!("You are CortexFS agent `{object_name}`."),
        "prompt.template.md" => DEFAULT_AGENT_PROMPT_TEMPLATE.to_owned(),
        "meta.json" => "{}".to_owned(),
        _ => String::new(),
    }
}

pub(crate) fn default_tool_control_value(object_name: &str, file: &str) -> String {
    match file {
        "name" => object_name.to_owned(),
        "schema" => "{\"type\":\"object\"}".to_owned(),
        "status" => "idle".to_owned(),
        _ => String::new(),
    }
}

pub(crate) fn is_valid_wrapper_target(value: &str) -> bool {
    !value.trim().is_empty() && !value.bytes().any(|byte| byte.is_ascii_control())
}

/// Renders the canonical executable wrapper for one filesystem object.
#[must_use]
pub fn executable_wrapper_script(class: ObjectClass, name: &str, wrapper_target: &str) -> String {
    format!(
        "#!/bin/sh\n# CortexFS generated object wrapper.\n# cortexfs.object={}\n# cortexfs.name={}\nexec {} \"$0\" \"$@\"\n",
        class.as_str(),
        name,
        shell_single_quote(wrapper_target)
    )
}

/// Writes one provider-model executable/control pair into an isolated stage.
/// The caller publishes the enclosing provider directory as one transaction.
pub(crate) fn stage_generated_model_pair(
    provider_dir: &fs::File,
    model: &str,
    id: &str,
    wrapper_target: &str,
    control_overrides: &[(&str, &str)],
) -> Result<(), ObjectBootstrapError> {
    if !is_object_name(model) || !is_object_name_for_class(ObjectClass::Model, id) {
        return Err(ObjectBootstrapError::InvalidObjectName);
    }
    if !is_valid_wrapper_target(wrapper_target) {
        return Err(ObjectBootstrapError::InvalidWrapperTarget);
    }
    validate_control_overrides(ObjectClass::Model, control_overrides)?;
    let control_name = format!("{model}.d");
    let control = support::plain::create_plain_dir_at(provider_dir, &control_name, 0o700)
        .map_err(|_error| ObjectBootstrapError::CannotCreate)?;
    for file in MODEL_CONTROL_FILES {
        let content = object_control_content(ObjectClass::Model, id, file, control_overrides)?;
        support::plain::write_text_file_at(&control, file, &content, 0o644)
            .map_err(|_error| ObjectBootstrapError::CannotRecord)?;
    }
    let wrapper = executable_wrapper_script(ObjectClass::Model, id, wrapper_target);
    support::plain::write_text_file_at(provider_dir, model, &wrapper, 0o755)
        .map_err(|_error| ObjectBootstrapError::CannotRecord)?;
    provider_dir
        .sync_all()
        .map_err(|_error| ObjectBootstrapError::CannotRecord)
}
