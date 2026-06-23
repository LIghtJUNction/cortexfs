/// Installs a v1 executable object wrapper plus required `.d` control files.
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
    fs::create_dir_all(&control_dir).map_err(|_error| ObjectBootstrapError::CannotCreate)?;

    let executable = class_dir.join(name);
    let wrapper = executable_wrapper_script(wrapper_target);
    atomic_replace_text(&executable, &wrapper)
        .map_err(|_error| ObjectBootstrapError::CannotRecord)?;
    set_executable_mode(&executable)?;

    for file in control_files_for(class) {
        let content = object_control_content(class, name, file, control_overrides)?;
        atomic_replace_text(&control_dir.join(file), &content)
            .map_err(|_error| ObjectBootstrapError::CannotRecord)?;
    }

    Ok(ObjectBootstrap::new(executable, control_dir))
}

fn validate_control_overrides(
    class: ObjectClass,
    control_overrides: &[(&str, &str)],
) -> Result<(), ObjectBootstrapError> {
    for (file, value) in control_overrides.iter().copied() {
        if !control_files_for(class).contains(&file) {
            return Err(ObjectBootstrapError::InvalidControlFile);
        }
        validate_object_control_content(class, file, &ensure_trailing_newline(value))?;
    }
    Ok(())
}

fn object_control_content(
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

fn validate_object_control_content(
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

fn validate_model_control_content(file: &str, content: &str) -> Result<(), ObjectBootstrapError> {
    match file {
        "cap" if inspect_model_capabilities(content).is_ok() => Ok(()),
        "driver" if parse_model_driver_routes(content).is_ok() => Ok(()),
        "session" if matches!(content.trim(), "none" | "socket") => Ok(()),
        "cap" | "driver" | "session" => Err(ObjectBootstrapError::InvalidControlValue),
        _ if !content.contains('\0') => Ok(()),
        _ => Err(ObjectBootstrapError::InvalidControlValue),
    }
}

fn validate_agent_bootstrap_control_content(
    file: &str,
    content: &str,
) -> Result<(), ObjectBootstrapError> {
    if content.contains('\0') {
        return Err(ObjectBootstrapError::InvalidControlValue);
    }
    let Some(kind) = AgentControlKind::parse(file) else {
        return Ok(());
    };
    if inspect_agent_control(kind, content).is_ok() {
        Ok(())
    } else {
        Err(ObjectBootstrapError::InvalidControlValue)
    }
}

fn validate_tool_control_content(file: &str, content: &str) -> Result<(), ObjectBootstrapError> {
    match file {
        "schema" if inspect_tool_schema_json(content).is_ok() => Ok(()),
        "schema" => Err(ObjectBootstrapError::InvalidControlValue),
        _ if !content.contains('\0') => Ok(()),
        _ => Err(ObjectBootstrapError::InvalidControlValue),
    }
}

fn default_object_control_value(class: ObjectClass, object_name: &str, file: &str) -> String {
    match class {
        ObjectClass::Model => default_model_control_value(object_name, file),
        ObjectClass::Agent => default_agent_control_value(object_name, file),
        ObjectClass::Tool => default_tool_control_value(object_name, file),
    }
}

fn default_model_control_value(object_name: &str, file: &str) -> String {
    match file {
        "id" => object_name.to_owned(),
        "driver" => "rig".to_owned(),
        "cap" => "chat\nstream".to_owned(),
        "session" => "none".to_owned(),
        "status" => "idle".to_owned(),
        _ => String::new(),
    }
}

fn default_agent_control_value(object_name: &str, file: &str) -> String {
    match file {
        "owner" | "uid" | "gid" => "0".to_owned(),
        "label" => format!("user_u:agent_r:{object_name}_t:s0"),
        "iso" => "shared".to_owned(),
        "life" => "owned".to_owned(),
        "root" | "cwd" => "/".to_owned(),
        "env" => "CTX_ROOT=/ctx".to_owned(),
        "path" => "/ctx/tool".to_owned(),
        "mount" => "/ctx\t/ctx\tro\trbind,nosuid,nodev".to_owned(),
        "status" => "idle".to_owned(),
        "meta.json" => "{}".to_owned(),
        _ => String::new(),
    }
}

fn default_tool_control_value(object_name: &str, file: &str) -> String {
    match file {
        "name" => object_name.to_owned(),
        "schema" => "{\"type\":\"object\"}".to_owned(),
        "status" => "idle".to_owned(),
        _ => String::new(),
    }
}

fn is_valid_wrapper_target(value: &str) -> bool {
    !value.trim().is_empty() && !value.contains('\0') && !value.contains('\n')
}

fn executable_wrapper_script(wrapper_target: &str) -> String {
    format!(
        "#!/bin/sh\n# CortexFS generated object wrapper.\nexec {} \"$@\"\n",
        shell_single_quote(wrapper_target)
    )
}
