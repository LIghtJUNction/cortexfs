use crate::support::layout::{
    LayoutPathRole, PathLayoutIssue, PlainPathKindCheck, check_plain_file, require_plain,
};
use crate::*;

/// Inspects a model, agent, or tool object triple under a `CortexFS` root.
#[must_use]
pub fn inspect_object_layout(root: &Path, class: ObjectClass, name: &str) -> ObjectLayoutReport {
    let mut issues = Vec::new();
    if !is_object_name_for_class(class, name) {
        issues.push(PathLayoutIssue::missing(
            format!("{}/{name}", class.as_str()),
            LayoutPathRole::Executable,
        ));
        return ObjectLayoutReport::new(issues);
    }
    if class == ObjectClass::Model && name == DEBUG_ECHO_MODEL {
        return ObjectLayoutReport::new(issues);
    }

    let exec_label = format!("{}/{name}", class.as_str());
    let exec_path = root.join(class.as_str()).join(name);
    require_plain(
        &exec_path,
        &exec_label,
        LayoutPathRole::Executable,
        &mut issues,
    );

    let control_label = format!("{}/{name}.d", class.as_str());
    let control_dir = root.join(class.as_str()).join(format!("{name}.d"));
    require_plain(
        &control_dir,
        &control_label,
        LayoutPathRole::ControlDirectory,
        &mut issues,
    );
    for file in control_files_for(class) {
        if class == ObjectClass::Agent && AGENT_OPTIONAL_CONTROL_FILES.contains(file) {
            continue;
        }
        let label = format!("{control_label}/{file}");
        require_plain(
            &control_dir.join(file),
            &label,
            LayoutPathRole::ControlFile,
            &mut issues,
        );
    }
    if class == ObjectClass::Agent {
        for file in AGENT_OPTIONAL_CONTROL_FILES {
            let label = format!("{control_label}/{file}");
            if check_plain_file(&control_dir.join(file)) == PlainPathKindCheck::WrongKind {
                issues.push(PathLayoutIssue::wrong_kind(
                    label,
                    LayoutPathRole::ControlFile,
                ));
            }
        }
    }
    if class != ObjectClass::Model {
        require_object_hook_dirs(&control_dir, &control_label, &mut issues);
    }

    inspect_object_socket(root, class, name, &control_dir, &mut issues);
    inspect_model_capability_control(class, name, &control_dir, &mut issues);
    inspect_model_driver_control(class, name, &control_dir, &mut issues);
    inspect_model_effort_control(class, name, &control_dir, &mut issues);
    inspect_model_fallback_control(class, name, &control_dir, &mut issues);
    inspect_tool_schema_control(class, name, &control_dir, &mut issues);
    inspect_agent_control_files(class, name, &control_dir, &mut issues);
    ObjectLayoutReport::new(issues)
}

pub(crate) fn control_files_for(class: ObjectClass) -> &'static [&'static str] {
    match class {
        ObjectClass::Model => MODEL_CONTROL_FILES,
        ObjectClass::Agent => AGENT_CONTROL_FILES,
        ObjectClass::Tool => TOOL_CONTROL_FILES,
    }
}

pub(crate) fn inspect_object_socket(
    root: &Path,
    class: ObjectClass,
    name: &str,
    control_dir: &Path,
    issues: &mut Vec<PathLayoutIssue>,
) {
    let socket_label = format!("{}/{name}.sock", class.as_str());
    let socket_path = root.join(class.as_str()).join(format!("{name}.sock"));
    match class {
        ObjectClass::Agent | ObjectClass::Tool => {
            require_unix_socket(&socket_path, &socket_label, false, issues);
        }
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
    }
}

pub(crate) fn inspect_model_socket(
    socket_path: &Path,
    socket_label: &str,
    session_label: &str,
    control_dir: &Path,
    issues: &mut Vec<PathLayoutIssue>,
) {
    let session_path = control_dir.join("session");
    let Ok(content) = read_object_layout_control_file(&session_path) else {
        return;
    };
    let value = content.trim();
    match value {
        "socket" => require_unix_socket(socket_path, socket_label, true, issues),
        "none" => require_unix_socket(socket_path, socket_label, false, issues),
        _ => issues.push(PathLayoutIssue::invalid_value(
            session_label.to_owned(),
            value.to_owned(),
        )),
    }
}

/// Reads an object control file when `class` matches, then runs `inspect`.
pub(crate) fn with_object_control_file(
    class: ObjectClass,
    expected: ObjectClass,
    control_dir: &Path,
    file: &str,
    issues: &mut Vec<PathLayoutIssue>,
    inspect: impl FnOnce(&str, &mut Vec<PathLayoutIssue>),
) {
    if class != expected {
        return;
    }
    let Ok(content) = read_object_layout_control_file(&control_dir.join(file)) else {
        return;
    };
    inspect(&content, issues);
}

pub(crate) fn push_control_invalid_values(
    path: &str,
    values: impl IntoIterator<Item = String>,
    issues: &mut Vec<PathLayoutIssue>,
) {
    for value in values {
        issues.push(PathLayoutIssue::invalid_value(path, value));
    }
}

pub(crate) fn inspect_model_capability_control(
    class: ObjectClass,
    name: &str,
    control_dir: &Path,
    issues: &mut Vec<PathLayoutIssue>,
) {
    with_object_control_file(
        class,
        ObjectClass::Model,
        control_dir,
        "cap",
        issues,
        |content, issues| {
            let path = format!("model/{name}.d/cap");
            let report = inspect_model_capabilities(content);
            let values = report.issues().iter().map(|issue| match *issue {
                ModelCapabilityIssue::ProviderPrivate { ref capability, .. }
                | ModelCapabilityIssue::Unknown { ref capability, .. } => capability.clone(),
            });
            push_control_invalid_values(&path, values, issues);
        },
    );
}

pub(crate) fn inspect_model_driver_control(
    class: ObjectClass,
    name: &str,
    control_dir: &Path,
    issues: &mut Vec<PathLayoutIssue>,
) {
    with_object_control_file(
        class,
        ObjectClass::Model,
        control_dir,
        "driver",
        issues,
        |content, issues| {
            if let Err(error) = parse_model_driver_routes(content) {
                issues.push(PathLayoutIssue::invalid_value(
                    format!("model/{name}.d/driver"),
                    model_driver_route_error_value(&error),
                ));
            }
        },
    );
}

pub(crate) fn model_driver_route_error_value(error: &ModelDriverRouteError) -> String {
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

pub(crate) fn inspect_model_effort_control(
    class: ObjectClass,
    name: &str,
    control_dir: &Path,
    issues: &mut Vec<PathLayoutIssue>,
) {
    with_object_control_file(
        class,
        ObjectClass::Model,
        control_dir,
        "effort",
        issues,
        |content, issues| {
            if ModelEffort::parse(content).is_none() {
                issues.push(PathLayoutIssue::invalid_value(
                    format!("model/{name}.d/effort"),
                    content.trim().to_owned(),
                ));
            }
        },
    );
}

pub(crate) fn inspect_model_fallback_control(
    class: ObjectClass,
    name: &str,
    control_dir: &Path,
    issues: &mut Vec<PathLayoutIssue>,
) {
    with_object_control_file(
        class,
        ObjectClass::Model,
        control_dir,
        "fallback",
        issues,
        |content, issues| {
            let path = format!("model/{name}.d/fallback");
            let report = parse_model_fallback(content).1;
            let values = report
                .issues()
                .iter()
                .map(|issue| model_fallback_issue_value(issue).to_owned());
            push_control_invalid_values(&path, values, issues);
        },
    );
}

pub(crate) fn model_fallback_issue_value(issue: &ModelFallbackIssue) -> &str {
    match issue {
        &ModelFallbackIssue::InvalidLine { ref value, .. }
        | &ModelFallbackIssue::DuplicateModel { ref value, .. } => value.as_str(),
    }
}

pub(crate) fn inspect_tool_schema_control(
    class: ObjectClass,
    name: &str,
    control_dir: &Path,
    issues: &mut Vec<PathLayoutIssue>,
) {
    with_object_control_file(
        class,
        ObjectClass::Tool,
        control_dir,
        "schema",
        issues,
        |content, issues| {
            let path = format!("tool/{name}.d/schema");
            let report = inspect_tool_schema_json(content);
            let values = report
                .issues()
                .iter()
                .map(|issue| issue.value().unwrap_or("").to_owned());
            push_control_invalid_values(&path, values, issues);
        },
    );
}

pub(crate) fn inspect_agent_control_files(
    class: ObjectClass,
    name: &str,
    control_dir: &Path,
    issues: &mut Vec<PathLayoutIssue>,
) {
    if class != ObjectClass::Agent {
        return;
    }

    for file in AGENT_CONTROL_FILES.iter().copied().chain(
        AGENT_OPTIONAL_CONTROL_FILES
            .iter()
            .copied()
            .filter(|file| !AGENT_CONTROL_FILES.contains(file)),
    ) {
        let control_path = control_dir.join(file);
        if check_plain_file(&control_path) != PlainPathKindCheck::Ok {
            continue;
        }
        let path = format!("agent/{name}.d/{file}");
        let Ok(content) = read_object_layout_control_file(&control_path) else {
            issues.push(PathLayoutIssue::invalid_value(path, "invalid content"));
            continue;
        };
        if validate_agent_bootstrap_control_content(file, &content).is_err() {
            issues.push(PathLayoutIssue::invalid_value(path, "invalid content"));
        }
    }
}

pub(crate) fn require_object_hook_dirs(
    control_dir: &Path,
    label: &str,
    issues: &mut Vec<PathLayoutIssue>,
) {
    let hook_label = format!("{label}/{OBJECT_HOOK_DIR}");
    let hook_dir = control_dir.join(OBJECT_HOOK_DIR);
    require_plain(
        &hook_dir,
        &hook_label,
        LayoutPathRole::ControlDirectory,
        issues,
    );
    for phase in OBJECT_HOOK_PHASE_DIRS {
        let phase_label = format!("{hook_label}/{phase}");
        require_plain(
            &hook_dir.join(phase),
            &phase_label,
            LayoutPathRole::ControlDirectory,
            issues,
        );
    }
}

pub(crate) fn require_unix_socket(
    path: &Path,
    label: &str,
    required: bool,
    issues: &mut Vec<PathLayoutIssue>,
) {
    match object_layout_socket_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => {}
        Ok(_metadata) => issues.push(PathLayoutIssue::wrong_kind(label, LayoutPathRole::Socket)),
        Err(error) if required || error.kind() != std::io::ErrorKind::NotFound => {
            issues.push(PathLayoutIssue::missing(label, LayoutPathRole::Socket));
        }
        Err(_error) => match path.symlink_metadata() {
            Ok(_metadata) => {
                issues.push(PathLayoutIssue::wrong_kind(label, LayoutPathRole::Socket));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_error) => {
                issues.push(PathLayoutIssue::missing(label, LayoutPathRole::Socket));
            }
        },
    }
}

pub mod disk;
use disk::*;

pub(crate) fn is_stable_chroot_absolute_path(value: &str) -> bool {
    if !value.starts_with('/') || value.bytes().any(|byte| byte.is_ascii_control()) {
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
pub(crate) enum JsonStringField {
    String(String),
    Other(Value),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum JsonU64Field {
    Number(u64),
    Other(Value),
}

impl JsonStringField {
    pub(crate) fn as_str(&self) -> Option<&str> {
        match *self {
            Self::String(ref value) => Some(value),
            Self::Other(ref value) => {
                let _ = value;
                None
            }
        }
    }
}

pub(crate) fn is_json_u64(value: Option<&JsonU64Field>) -> bool {
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

pub(crate) fn provider_native_fields(value: &Value) -> Vec<&str> {
    let mut fields = Vec::new();
    collect_provider_native_fields(value, &mut fields);
    fields
}

pub(crate) fn collect_provider_native_fields<'a>(value: &'a Value, fields: &mut Vec<&'a str>) {
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

pub(crate) fn is_provider_native_field(key: &str) -> bool {
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
