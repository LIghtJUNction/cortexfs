use crate::*;

use crate::support::plain;

const MAX_OBJECT_METADATA_CONTROL_BYTES: u64 = 64 * 1024;
pub(crate) const MAX_ECHO_MODEL_STDIN_BYTES: usize = 1024 * 1024;

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
    let limit = read_object_control_body_for_metadata(control_dir, "limit")?;
    let context_length = model_metadata_context_length(&limit)?;
    Ok(exec_metadata(&[
        ("object", "model".to_owned()),
        ("id", id),
        ("name", name.to_owned()),
        ("description", description.to_owned()),
        ("type", model_type.to_owned()),
        ("created_at", String::new()),
        ("owned_by", owned_by.to_owned()),
        ("context_length", context_length),
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

pub(crate) fn object_exec_metadata(
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

pub(crate) fn exec_metadata(fields: &[(&str, String)]) -> String {
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

pub(crate) fn model_metadata_description(name: &str, driver: &str) -> &'static str {
    if name == "debug/echo" && driver == "debug" {
        "Built-in debug echo model"
    } else {
        ""
    }
}

pub(crate) fn model_metadata_type(driver: &str) -> &str {
    if driver == "debug" { "debug" } else { "chat" }
}

pub(crate) fn model_metadata_owner(name: &str, driver: &str) -> &'static str {
    if name == "debug/echo" && driver == "debug" {
        "cortexfs"
    } else {
        ""
    }
}

pub(crate) fn model_metadata_context_length(limit: &str) -> Result<String, FuseV1Error> {
    ModelContextLimit::parse_control(limit)
        .map(|limit| limit.to_string())
        .ok_or(FuseV1Error::InvalidContent)
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
        input = read_echo_model_stdin_limited(std::io::stdin(), MAX_ECHO_MODEL_STDIN_BYTES)?;
    }
    let run = env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned());
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

pub(crate) fn read_echo_model_stdin_limited(
    reader: impl Read,
    max_bytes: usize,
) -> std::io::Result<String> {
    let limit = u64::try_from(max_bytes.saturating_add(1)).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("stdin read limit is invalid: {error}"),
        )
    })?;
    let mut input = String::new();
    reader.take(limit).read_to_string(&mut input)?;
    if input.len() > max_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "stdin exceeds debug echo model input limit",
        ));
    }
    Ok(input)
}

pub(crate) fn read_object_control_for_metadata(
    control_dir: &Path,
    file: &str,
) -> Result<String, FuseV1Error> {
    read_object_control_body_for_metadata(control_dir, file)
        .map(|content| content.trim_end_matches('\n').to_owned())
}

fn read_object_control_body_for_metadata(
    control_dir: &Path,
    file: &str,
) -> Result<String, FuseV1Error> {
    let path = control_dir.join(file);
    let metadata =
        plain::path_metadata_no_follow(&path).map_err(|error| fuse_metadata_error(&error))?;
    if !metadata.is_file() || metadata.len() > MAX_OBJECT_METADATA_CONTROL_BYTES {
        return Err(FuseV1Error::InvalidContent);
    }
    let len = usize::try_from(metadata.len()).map_err(|_error| FuseV1Error::InvalidContent)?;
    let mut file = plain::open_plain_file(&path).map_err(|error| fuse_metadata_error(&error))?;
    let mut content = vec![0; len];
    file.read_exact(&mut content)
        .map_err(|error| fuse_metadata_error(&error))?;
    String::from_utf8(content).map_err(|_error| FuseV1Error::InvalidContent)
}

pub(crate) fn is_valid_env_key(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first == b'_' || first.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}
