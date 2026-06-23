/// Returns executable metadata text for a model object.
pub fn model_exec_metadata(name: &str, control_dir: &Path) -> Result<String, FuseV1Error> {
    if !is_model_name(name) {
        return Err(FuseV1Error::InvalidPath);
    }
    let id = read_object_control_for_metadata(control_dir, "id")?;
    let driver_content = read_object_control_for_metadata(control_dir, "driver")?;
    let driver_routes =
        parse_model_driver_routes(&driver_content).map_err(|_error| FuseV1Error::InvalidContent)?;
    let driver = driver_routes
        .primary_driver_for(ModelDriverUseCase::Default)
        .unwrap_or("");
    let session = read_object_control_for_metadata(control_dir, "session")?;
    let status = read_object_control_for_metadata(control_dir, "status")?;
    let cap = read_object_control_for_metadata(control_dir, "cap")?;
    let description = model_metadata_description(name, driver);
    let model_type = model_metadata_type(driver);
    let owned_by = model_metadata_owner(name, driver);
    let context_length = model_metadata_context_length(name, driver);
    Ok(exec_metadata(&[
        ("object", "model".to_owned()),
        ("id", id),
        ("name", name.to_owned()),
        ("description", description.to_owned()),
        ("type", model_type.to_owned()),
        ("created_at", String::new()),
        ("owned_by", owned_by.to_owned()),
        ("context_length", context_length.to_string()),
        ("driver", driver.to_owned()),
        (
            "driver.default",
            driver_routes.route_value(ModelDriverUseCase::Default),
        ),
        (
            "driver.exec",
            driver_routes.route_value(ModelDriverUseCase::Exec),
        ),
        (
            "driver.socket",
            driver_routes.route_value(ModelDriverUseCase::Socket),
        ),
        (
            "driver.agent",
            driver_routes.route_value(ModelDriverUseCase::Agent),
        ),
        ("session", session),
        ("status", status),
        ("cap", cap.lines().collect::<Vec<_>>().join(",")),
    ]))
}

/// Returns executable metadata text for a tool object.
pub fn tool_exec_metadata(name: &str, control_dir: &Path) -> Result<String, FuseV1Error> {
    if !is_object_name(name) {
        return Err(FuseV1Error::InvalidPath);
    }
    let declared_name = read_object_control_for_metadata(control_dir, "name")
        .unwrap_or_else(|_error| name.to_owned());
    let description =
        read_object_control_for_metadata(control_dir, "description").unwrap_or_default();
    let cap = read_object_control_for_metadata(control_dir, "cap").unwrap_or_default();
    let status = read_object_control_for_metadata(control_dir, "status")
        .unwrap_or_else(|_error| "unknown".to_owned());
    Ok(exec_metadata(&[
        ("object", "tool".to_owned()),
        ("name", name.to_owned()),
        ("declared_name", declared_name),
        ("description", description),
        ("runner", "cortexfs-object-runner".to_owned()),
        ("status", status),
        ("cap", cap.lines().collect::<Vec<_>>().join(",")),
    ]))
}

/// Returns executable metadata text for an agent object.
pub fn agent_exec_metadata(name: &str, control_dir: &Path) -> Result<String, FuseV1Error> {
    if !is_object_name(name) {
        return Err(FuseV1Error::InvalidPath);
    }
    let owner = read_object_control_for_metadata(control_dir, "owner").unwrap_or_default();
    let uid = read_object_control_for_metadata(control_dir, "uid").unwrap_or_default();
    let gid = read_object_control_for_metadata(control_dir, "gid").unwrap_or_default();
    let label = read_object_control_for_metadata(control_dir, "label").unwrap_or_default();
    let model = read_object_control_for_metadata(control_dir, "model").unwrap_or_default();
    let status = read_object_control_for_metadata(control_dir, "status")
        .unwrap_or_else(|_error| "unknown".to_owned());
    let pid = read_object_control_for_metadata(control_dir, "pid").unwrap_or_default();
    Ok(exec_metadata(&[
        ("object", "agent".to_owned()),
        ("name", name.to_owned()),
        ("runner", "cortexfs-object-runner".to_owned()),
        ("owner", owner),
        ("uid", uid),
        ("gid", gid),
        ("label", label),
        ("model", model),
        ("status", status),
        ("pid", pid),
    ]))
}

fn object_exec_metadata(
    class: ObjectClass,
    name: &str,
    control_dir: &Path,
) -> Result<String, FuseV1Error> {
    match class {
        ObjectClass::Model => model_exec_metadata(name, control_dir),
        ObjectClass::Agent => agent_exec_metadata(name, control_dir),
        ObjectClass::Tool => tool_exec_metadata(name, control_dir),
    }
}

fn exec_metadata(fields: &[(&str, String)]) -> String {
    let mut output = format!("#!{CORTEXFS_OBJECT_RUNNER}\n");
    for field in fields {
        output.push_str("# cortexfs.");
        output.push_str(field.0);
        output.push('=');
        output.push_str(&field.1);
        output.push('\n');
    }
    output
}

fn model_metadata_description(name: &str, driver: &str) -> &'static str {
    if name == "debug/echo" && driver == "debug" {
        "Built-in debug echo model"
    } else {
        ""
    }
}

fn model_metadata_type(driver: &str) -> &str {
    if driver == "debug" { "debug" } else { "chat" }
}

fn model_metadata_owner(name: &str, driver: &str) -> &'static str {
    if name == "debug/echo" && driver == "debug" {
        "cortexfs"
    } else {
        ""
    }
}

fn model_metadata_context_length(_name: &str, _driver: &str) -> u64 {
    0
}

/// Runs the built-in debug echo model and writes canonical JSONL.
pub fn run_echo_model<I, S, W>(args: I, mut stdout: W) -> std::io::Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    W: Write,
{
    let mut input = args
        .into_iter()
        .map(|value| value.as_ref().to_owned())
        .collect::<Vec<_>>()
        .join(" ");
    if input.is_empty() {
        std::io::stdin().read_to_string(&mut input)?;
    }
    let run = std::env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned());
    let text = serde_json::to_string(&input).unwrap_or_else(|_error| "\"\"".to_owned());
    stdout.write_all(
        format!(r#"{{"type":"start","run":"{run}","model":"debug/echo"}}"#).as_bytes(),
    )?;
    stdout.write_all(b"\n")?;
    stdout.write_all(format!(r#"{{"type":"delta","run":"{run}","text":{text}}}"#).as_bytes())?;
    stdout.write_all(b"\n")?;
    stdout.write_all(format!(r#"{{"type":"done","run":"{run}","status":"ok"}}"#).as_bytes())?;
    stdout.write_all(b"\n")
}

fn read_object_control_for_metadata(control_dir: &Path, file: &str) -> Result<String, FuseV1Error> {
    fs::read_to_string(control_dir.join(file))
        .map(|content| content.trim_end_matches('\n').to_owned())
        .map_err(|error| fuse_metadata_error(&error))
}

fn policy_subject_from_label(label: &str) -> Option<&str> {
    if is_object_name(label) {
        return Some(label);
    }
    let mut fields = label.split(':');
    let _user = fields.next()?;
    let _role = fields.next()?;
    let subject = fields.next()?;
    let _level = fields.next()?;
    if fields.next().is_none() && is_object_name(subject) {
        Some(subject)
    } else {
        None
    }
}

fn is_valid_env_key(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first == b'_' || first.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}
