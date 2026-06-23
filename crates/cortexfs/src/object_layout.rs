/// Inspects a model, agent, or tool object triple under a `CortexFS` root.
#[must_use]
pub fn inspect_object_layout(root: &Path, class: ObjectClass, name: &str) -> ObjectLayoutReport {
    let mut issues = Vec::new();
    if !is_object_name_for_class(class, name) {
        issues.push(ObjectLayoutIssue::MissingExecutable(format!(
            "{}/{}",
            class.as_str(),
            name
        )));
        return ObjectLayoutReport::new(issues);
    }
    if class == ObjectClass::Model && name == DEBUG_ECHO_MODEL {
        return ObjectLayoutReport::new(issues);
    }

    let exec_label = format!("{}/{name}", class.as_str());
    let exec_path = root.join(class.as_str()).join(name);
    require_executable_file(&exec_path, &exec_label, &mut issues);

    let control_label = format!("{}/{name}.d", class.as_str());
    let control_dir = root.join(class.as_str()).join(format!("{name}.d"));
    require_object_control_dir(&control_dir, &control_label, &mut issues);
    for file in control_files_for(class) {
        let label = format!("{control_label}/{file}");
        require_object_control_file(&control_dir.join(file), &label, &mut issues);
    }

    inspect_object_socket(root, class, name, &control_dir, &mut issues);
    inspect_model_capability_control(class, name, &control_dir, &mut issues);
    inspect_model_driver_control(class, name, &control_dir, &mut issues);
    inspect_tool_schema_control(class, name, &control_dir, &mut issues);
    inspect_agent_control_files(class, name, &control_dir, &mut issues);
    ObjectLayoutReport::new(issues)
}

fn control_files_for(class: ObjectClass) -> &'static [&'static str] {
    match class {
        ObjectClass::Model => MODEL_CONTROL_FILES,
        ObjectClass::Agent => AGENT_CONTROL_FILES,
        ObjectClass::Tool => TOOL_CONTROL_FILES,
    }
}

fn inspect_object_socket(
    root: &Path,
    class: ObjectClass,
    name: &str,
    control_dir: &Path,
    issues: &mut Vec<ObjectLayoutIssue>,
) {
    let socket_label = format!("{}/{name}.sock", class.as_str());
    let socket_path = root.join(class.as_str()).join(format!("{name}.sock"));
    match class {
        ObjectClass::Agent => require_unix_socket(&socket_path, &socket_label, true, issues),
        ObjectClass::Model => {
            let session_label = format!("{}/{name}.d/session", class.as_str());
            inspect_model_socket(
                &socket_path,
                &socket_label,
                &session_label,
                control_dir,
                issues,
            );
        }
        ObjectClass::Tool => require_unix_socket(&socket_path, &socket_label, false, issues),
    }
}

fn inspect_model_socket(
    socket_path: &Path,
    socket_label: &str,
    session_label: &str,
    control_dir: &Path,
    issues: &mut Vec<ObjectLayoutIssue>,
) {
    let session_path = control_dir.join("session");
    let Ok(content) = fs::read_to_string(&session_path) else {
        return;
    };
    let value = content.trim();
    match value {
        "socket" => require_unix_socket(socket_path, socket_label, true, issues),
        "none" => require_unix_socket(socket_path, socket_label, false, issues),
        _ => issues.push(ObjectLayoutIssue::InvalidControlValue {
            path: session_label.to_owned(),
            value: value.to_owned(),
        }),
    }
}

fn inspect_model_capability_control(
    class: ObjectClass,
    name: &str,
    control_dir: &Path,
    issues: &mut Vec<ObjectLayoutIssue>,
) {
    if class != ObjectClass::Model {
        return;
    }

    let Ok(content) = fs::read_to_string(control_dir.join("cap")) else {
        return;
    };
    for issue in inspect_model_capabilities(&content).issues() {
        let value = match *issue {
            ModelCapabilityIssue::ProviderPrivate { ref capability, .. }
            | ModelCapabilityIssue::Unknown { ref capability, .. } => capability,
        };
        issues.push(ObjectLayoutIssue::InvalidControlValue {
            path: format!("model/{name}.d/cap"),
            value: value.to_owned(),
        });
    }
}

fn inspect_model_driver_control(
    class: ObjectClass,
    name: &str,
    control_dir: &Path,
    issues: &mut Vec<ObjectLayoutIssue>,
) {
    if class != ObjectClass::Model {
        return;
    }

    let Ok(content) = fs::read_to_string(control_dir.join("driver")) else {
        return;
    };
    if let Err(error) = parse_model_driver_routes(&content) {
        issues.push(ObjectLayoutIssue::InvalidControlValue {
            path: format!("model/{name}.d/driver"),
            value: model_driver_route_error_value(&error),
        });
    }
}

fn model_driver_route_error_value(error: &ModelDriverRouteError) -> String {
    match *error {
        ModelDriverRouteError::Empty => "empty".to_owned(),
        ModelDriverRouteError::MissingEquals { line } => format!("line {line} missing ="),
        ModelDriverRouteError::UnknownUseCase { line, ref value } => {
            format!("line {line} unknown use case {value}")
        }
        ModelDriverRouteError::DuplicateUseCase { line, ref value } => {
            format!("line {line} duplicate use case {value}")
        }
        ModelDriverRouteError::EmptyDriver { line } => format!("line {line} empty driver"),
        ModelDriverRouteError::InvalidDriverName { line, ref value } => {
            format!("line {line} invalid driver {value}")
        }
    }
}

fn inspect_tool_schema_control(
    class: ObjectClass,
    name: &str,
    control_dir: &Path,
    issues: &mut Vec<ObjectLayoutIssue>,
) {
    if class != ObjectClass::Tool {
        return;
    }

    let Ok(content) = fs::read_to_string(control_dir.join("schema")) else {
        return;
    };
    for issue in inspect_tool_schema_json(&content).issues() {
        issues.push(ObjectLayoutIssue::InvalidControlValue {
            path: format!("tool/{name}.d/schema"),
            value: tool_schema_issue_value(issue).to_owned(),
        });
    }
}

fn tool_schema_issue_value(issue: &ToolSchemaIssue) -> &str {
    match *issue {
        ToolSchemaIssue::AuthorityField(ref field) => field,
        ToolSchemaIssue::InvalidJson
        | ToolSchemaIssue::NotObject
        | ToolSchemaIssue::InvalidSchema => "",
    }
}

fn inspect_agent_control_files(
    class: ObjectClass,
    name: &str,
    control_dir: &Path,
    issues: &mut Vec<ObjectLayoutIssue>,
) {
    if class != ObjectClass::Agent {
        return;
    }

    for file in AGENT_CONTROL_FILES {
        let Some(kind) = AgentControlKind::parse(file) else {
            continue;
        };
        let Ok(content) = fs::read_to_string(control_dir.join(file)) else {
            continue;
        };
        for issue in inspect_agent_control(kind, &content).issues() {
            issues.push(ObjectLayoutIssue::InvalidControlValue {
                path: format!("agent/{name}.d/{file}"),
                value: agent_control_issue_value(issue).to_owned(),
            });
        }
    }
}

fn agent_control_issue_value(issue: &AgentControlIssue) -> &str {
    match *issue {
        AgentControlIssue::InvalidNumber { ref value, .. }
        | AgentControlIssue::InvalidValue { ref value, .. } => value,
        AgentControlIssue::EmptyValue | AgentControlIssue::MultipleValues { .. } => "",
    }
}

fn require_executable_file(path: &Path, label: &str, issues: &mut Vec<ObjectLayoutIssue>) {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 => {}
        Ok(_metadata) => issues.push(ObjectLayoutIssue::NotExecutable(label.to_owned())),
        Err(_error) => issues.push(ObjectLayoutIssue::MissingExecutable(label.to_owned())),
    }
}

fn require_object_control_dir(path: &Path, label: &str, issues: &mut Vec<ObjectLayoutIssue>) {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_metadata) => issues.push(ObjectLayoutIssue::NotControlDirectory(label.to_owned())),
        Err(_error) => issues.push(ObjectLayoutIssue::MissingControlDirectory(label.to_owned())),
    }
}

fn require_object_control_file(path: &Path, label: &str, issues: &mut Vec<ObjectLayoutIssue>) {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_metadata) => issues.push(ObjectLayoutIssue::NotControlFile(label.to_owned())),
        Err(_error) => issues.push(ObjectLayoutIssue::MissingControlFile(label.to_owned())),
    }
}

fn require_unix_socket(
    path: &Path,
    label: &str,
    required: bool,
    issues: &mut Vec<ObjectLayoutIssue>,
) {
    match fs::metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => {}
        Ok(_metadata) => issues.push(ObjectLayoutIssue::NotSocket(label.to_owned())),
        Err(_error) if required => issues.push(ObjectLayoutIssue::MissingSocket(label.to_owned())),
        Err(_error) => {}
    }
}

pub(crate) fn is_stable_chroot_absolute_path(value: &str) -> bool {
    if !value.starts_with('/')
        || value.contains('\0')
        || value.contains('\t')
        || value.contains('\n')
    {
        return false;
    }
    if value == "/" {
        return true;
    }
    value
        .split('/')
        .skip(1)
        .all(|part| !part.is_empty() && part != "." && part != "..")
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum JsonStringField {
    String(String),
    Other(Value),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum JsonU64Field {
    Number(u64),
    Other(Value),
}

impl JsonStringField {
    fn as_str(&self) -> Option<&str> {
        match *self {
            Self::String(ref value) => Some(value),
            Self::Other(ref value) => {
                let _ = value;
                None
            }
        }
    }
}

fn is_json_u64(value: Option<&JsonU64Field>) -> bool {
    value.is_some_and(|value| match *value {
        JsonU64Field::Number(ref number) => {
            let _ = number;
            true
        }
        JsonU64Field::Other(ref value) => {
            let _ = value;
            false
        }
    })
}

fn provider_native_fields(value: &Value) -> Vec<&str> {
    let mut fields = Vec::new();
    collect_provider_native_fields(value, &mut fields);
    fields
}

fn collect_provider_native_fields<'a>(value: &'a Value, fields: &mut Vec<&'a str>) {
    if let Some(object) = value.as_object() {
        for (key, child) in object {
            if is_provider_native_field(key) {
                fields.push(key);
            }
            collect_provider_native_fields(child, fields);
        }
        return;
    }

    if let Some(items) = value.as_array() {
        for item in items {
            collect_provider_native_fields(item, fields);
        }
    }
}

fn is_provider_native_field(key: &str) -> bool {
    matches!(
        key,
        "thread_id"
            | "response_id"
            | "conversation_id"
            | "provider_thread_id"
            | "provider_response_id"
            | "native_thread"
            | "native_state"
            | "openai_response_id"
            | "anthropic_message_id"
            | "gemini_response_id"
    )
}
