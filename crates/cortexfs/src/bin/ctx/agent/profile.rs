use crate::*;
use serde::Deserialize;

/// Host-side authoring schema for agent profiles (import-only; runtime uses `.d/*`).
pub(crate) const AGENT_PROFILE_SCHEMA_V1: &str = "cortexfs.agent.profile/v1";

/// Conventional profile file names (Microsoft-style `agent.yaml` first).
pub(crate) const AGENT_PROFILE_FILE_NAMES: &[&str] = &["agent.yaml", "agent.yml", "agent.json"];
const MAX_AGENT_PROFILE_CONTROL_BYTES: u64 = 64 * 1024;

/// Parsed host-side agent profile ready to materialize into `agent/<name>.d/*`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct AgentProfile {
    pub(crate) name: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) instructions: Option<String>,
    pub(crate) models: Vec<String>,
    pub(crate) tools: Vec<String>,
    pub(crate) label: Option<String>,
    pub(crate) parent: Option<String>,
    pub(crate) temporary: bool,
    pub(crate) shared: Vec<AgentShared>,
    pub(crate) mounts: Vec<AgentMount>,
}

#[derive(Debug, Deserialize)]
struct ProfileDocument {
    #[serde(default)]
    schema: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    instructions: Option<String>,
    #[serde(default)]
    model: Option<ProfileStringOrList>,
    #[serde(default)]
    models: Option<Vec<String>>,
    #[serde(default)]
    tools: Option<ProfileStringOrList>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    temporary: Option<bool>,
    #[serde(default)]
    life: Option<String>,
    #[serde(default)]
    shared: Option<Vec<ProfileShared>>,
    #[serde(default)]
    mounts: Option<Vec<ProfileMount>>,
    #[serde(default)]
    mount: Option<Vec<ProfileMount>>,
    /// Microsoft `AgentSchema` nested template (`AgentManifest.template`).
    #[serde(default)]
    template: Option<Box<Self>>,
    /// Microsoft `AgentSchema` kind: prompt | hosted | workflow.
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    image: Option<String>,
    #[serde(default, rename = "dockerfilePath")]
    dockerfile_path: Option<String>,
    #[serde(default)]
    resources: Option<Vec<ProfileResource>>,
}

#[derive(Debug, Deserialize)]
struct ProfileShared {
    name: String,
    access: String,
}

#[derive(Debug, Deserialize)]
struct ProfileMount {
    source: String,
    target: String,
    mode: String,
}

#[derive(Debug, Deserialize)]
struct ProfileResource {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ProfileStringOrList {
    One(String),
    Many(Vec<String>),
}

impl ProfileStringOrList {
    fn into_vec(self) -> Vec<String> {
        match self {
            Self::One(value) => vec![value],
            Self::Many(values) => values,
        }
    }
}

/// Loads a host-side agent profile from a path, directory, or short name.
///
/// Resolution order for `spec`:
/// 1. Existing file path (any name, including `agent.yaml`)
/// 2. Existing directory → look for `agent.yaml` / `agent.yml` / `agent.json`
/// 3. Short name (no `/`) → search well-known host locations for
///    `<name>/agent.yaml` or `<name>.yaml`
pub(crate) fn load_agent_profile(spec: &Path) -> Result<AgentProfile, CliError> {
    let path = resolve_agent_profile_path(spec)?;
    let text = fs::read_to_string(&path).map_err(|error| {
        CliError::usage(format!(
            "cannot read agent profile {}: {error}",
            path.display()
        ))
    })?;
    parse_agent_profile_text(&text)
}

/// Resolves `--from` to a concrete host profile file path.
pub(crate) fn resolve_agent_profile_path(spec: &Path) -> Result<PathBuf, CliError> {
    if spec.as_os_str().is_empty() {
        return Err(CliError::usage(
            "agent profile path must not be empty (expected agent.yaml path, directory, or short name)",
        ));
    }

    if spec.is_file() {
        return Ok(spec.to_path_buf());
    }

    if spec.is_dir() {
        return find_agent_yaml_in_dir(spec).ok_or_else(|| {
            CliError::usage(format!(
                "no agent.yaml in directory {} (looked for {})",
                spec.display(),
                AGENT_PROFILE_FILE_NAMES.join(", ")
            ))
        });
    }

    // Bare / short name: search conventional host locations.
    let key = spec.to_string_lossy();
    if is_agent_profile_short_name(&key) {
        if let Some(path) = find_agent_profile_by_short_name(&key) {
            return Ok(path);
        }
        return Err(CliError::usage(format!(
            "agent profile not found for `{key}`; tried .cortexfs/agents/{key}/agent.yaml, .cortexfs/agents/{key}.yaml, ~/.config/cortexfs/agents/…, and agents/{key}/agent.yaml"
        )));
    }

    // Explicit path that does not exist yet (clearer error than short-name search).
    Err(CliError::usage(format!(
        "cannot read agent profile {}: No such file or directory",
        spec.display()
    )))
}

fn is_agent_profile_short_name(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('/')
        && !value.contains('\\')
        && value != "."
        && value != ".."
        && is_object_name(value)
}

fn find_agent_yaml_in_dir(dir: &Path) -> Option<PathBuf> {
    for name in AGENT_PROFILE_FILE_NAMES {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn find_agent_profile_by_short_name(name: &str) -> Option<PathBuf> {
    for base in agent_profile_search_bases() {
        let dir = base.join(name);
        if let Some(path) = find_agent_yaml_in_dir(&dir) {
            return Some(path);
        }
        for ext in ["yaml", "yml", "json"] {
            let candidate = base.join(format!("{name}.{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn agent_profile_search_bases() -> Vec<PathBuf> {
    let mut bases = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        bases.push(cwd.join(".cortexfs").join("agents"));
        bases.push(cwd.join("agents"));
    }
    if let Some(xdg) = env::var_os("XDG_CONFIG_HOME") {
        bases.push(PathBuf::from(xdg).join("cortexfs").join("agents"));
    } else if let Some(home) = env::var_os("HOME") {
        bases.push(
            PathBuf::from(home)
                .join(".config")
                .join("cortexfs")
                .join("agents"),
        );
    }
    bases
}

pub(crate) fn parse_agent_profile_text(text: &str) -> Result<AgentProfile, CliError> {
    let doc: ProfileDocument = serde_yaml::from_str(text)
        .map_err(|error| CliError::usage(format!("invalid agent profile YAML/JSON: {error}")))?;
    profile_document_to_profile(doc)
}

fn profile_document_to_profile(mut doc: ProfileDocument) -> Result<AgentProfile, CliError> {
    if let Some(schema) = doc.schema.as_deref()
        && schema != AGENT_PROFILE_SCHEMA_V1
        && !schema.is_empty()
    {
        return Err(CliError::usage(format!(
            "unsupported agent profile schema: {schema} (expected {AGENT_PROFILE_SCHEMA_V1} or omit)"
        )));
    }

    // Microsoft AgentManifest: concrete fields live under template.
    if let Some(template) = doc.template.take() {
        if doc.name.is_none() {
            doc.name.clone_from(&template.name);
        }
        if doc.description.is_none() {
            doc.description.clone_from(&template.description);
        }
        if doc.instructions.is_none() {
            doc.instructions.clone_from(&template.instructions);
        }
        if doc.model.is_none() {
            doc.model = template.model;
        }
        if doc.models.is_none() {
            doc.models = template.models;
        }
        if doc.tools.is_none() {
            doc.tools = template.tools;
        }
        if doc.label.is_none() {
            doc.label = template.label;
        }
        if doc.parent.is_none() {
            doc.parent = template.parent;
        }
        if doc.temporary.is_none() {
            doc.temporary = template.temporary;
        }
        if doc.life.is_none() {
            doc.life = template.life;
        }
        if doc.shared.is_none() {
            doc.shared = template.shared;
        }
        if doc.mounts.is_none() {
            doc.mounts = template.mounts;
        }
        if doc.mount.is_none() {
            doc.mount = template.mount;
        }
        if doc.kind.is_none() {
            doc.kind = template.kind;
        }
        if doc.image.is_none() {
            doc.image = template.image;
        }
        if doc.dockerfile_path.is_none() {
            doc.dockerfile_path = template.dockerfile_path;
        }
        if doc.resources.is_none() {
            doc.resources = template.resources;
        }
    }

    reject_unsupported_microsoft_kind(&doc)?;

    let mut models = Vec::new();
    if let Some(model) = doc.model {
        models.extend(model.into_vec());
    }
    if let Some(extra) = doc.models {
        models.extend(extra);
    }
    if models.is_empty()
        && let Some(resources) = doc.resources.as_ref()
    {
        for resource in resources {
            if resource.kind.as_deref() == Some("model")
                && let Some(id) = resource.id.as_deref().or(resource.name.as_deref())
                && !id.is_empty()
            {
                models.push(id.to_owned());
                break;
            }
        }
    }

    let tools = doc
        .tools
        .map_or_else(Vec::new, ProfileStringOrList::into_vec);
    let temporary =
        doc.temporary.unwrap_or(false) || doc.life.as_deref().is_some_and(|life| life == "temp");

    let shared = doc
        .shared
        .unwrap_or_default()
        .into_iter()
        .map(|entry| AgentShared {
            name: entry.name,
            access: entry.access,
        })
        .collect();

    let mounts = doc
        .mounts
        .or(doc.mount)
        .unwrap_or_default()
        .into_iter()
        .map(|entry| AgentMount {
            source: entry.source,
            target: entry.target,
            mode: entry.mode,
        })
        .collect();

    Ok(AgentProfile {
        name: nonempty_opt(doc.name),
        description: nonempty_opt(doc.description),
        instructions: nonempty_opt(doc.instructions),
        models,
        tools,
        label: nonempty_opt(doc.label),
        parent: nonempty_opt(doc.parent),
        temporary,
        shared,
        mounts,
    })
}

fn reject_unsupported_microsoft_kind(doc: &ProfileDocument) -> Result<(), CliError> {
    let kind = doc.kind.as_deref().unwrap_or("");
    if kind == "workflow" {
        return Err(CliError::usage(
            "agent profile kind workflow is not supported; use cortexfs.agent.profile/v1 fields",
        ));
    }
    if kind == "hosted"
        || doc.image.as_deref().is_some_and(|value| !value.is_empty())
        || doc
            .dockerfile_path
            .as_deref()
            .is_some_and(|value| !value.is_empty())
    {
        return Err(CliError::usage(
            "hosted/container agent profiles are not supported; materialize model/instructions/tools only",
        ));
    }
    Ok(())
}

fn nonempty_opt(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    })
}

/// Merges a loaded profile with CLI overrides. CLI wins when set.
pub(crate) fn agent_new_args_from_profile(
    profile: AgentProfile,
    mut args: AgentNewArgs,
) -> Result<AgentNewArgs, CliError> {
    if args.name.is_empty() {
        args.name = profile.name.ok_or_else(|| {
            CliError::usage("agent profile is missing name; pass NAME or set profile name")
        })?;
    }
    if args.parent.is_none() {
        args.parent = profile.parent;
    }
    if args.label.is_none() {
        args.label = profile.label;
    }
    if args.models.is_empty() {
        args.models = profile.models;
    }
    if args.tools.is_empty() {
        args.tools = profile.tools;
    }
    if args.shared.is_empty() {
        args.shared = profile.shared;
    }
    if args.mounts.is_empty() {
        args.mounts = profile.mounts;
    }
    if !args.temporary {
        args.temporary = profile.temporary;
    }
    if args.instructions.is_none() {
        args.instructions = profile.instructions;
    }
    if args.description.is_none() {
        args.description = profile.description;
    }
    Ok(args)
}

pub(crate) fn agent_profile_meta_json(description: Option<&str>) -> String {
    match description {
        Some(text) if !text.is_empty() => serde_json::json!({
            "description": text,
            "source": "profile",
        })
        .to_string(),
        _ => "{}".to_owned(),
    }
}

/// Applies a host-side profile onto an existing agent control directory.
///
/// Runtime authority remains discrete `.d/*` files; this only rewrites selected
/// controls that the profile declares.
pub(crate) fn agent_apply(
    root: &Path,
    name: &str,
    profile_spec: &Path,
) -> Result<ExitCode, CliError> {
    require_cli_name("agent name", name)?;
    let profile = load_agent_profile(profile_spec)?;
    validate_agent_apply_profile(name, &profile)?;
    let control = agent_control_dir(root, name);
    let control_metadata = fs::symlink_metadata(&control)
        .map_err(|_error| CliError::unavailable(format!("agent not found: {name}")))?;
    if !control_metadata.file_type().is_dir() || control_metadata.file_type().is_symlink() {
        return Err(CliError::unavailable(format!(
            "agent control is not a plain directory: {}",
            control.display()
        )));
    }

    let mut writes = Vec::new();
    if let Some(instructions) = profile.instructions.as_deref() {
        require_plain_agent_control_target(&control.join("system.md"))?;
        writes.push(("system.md", ensure_profile_newline(instructions)));
    }
    if let Some(description) = profile.description.as_deref() {
        let meta = read_agent_profile_control(&control, "meta.json")?;
        writes.push((
            "meta.json",
            ensure_profile_newline(&merge_agent_profile_meta(&meta, description)?),
        ));
    }

    let model = profile.models.first().cloned();
    if let Some(ref model) = model {
        require_plain_agent_control_target(&control.join("model"))?;
        writes.push(("model", ensure_profile_newline(model)));
    }

    if model.is_some() || !profile.tools.is_empty() {
        let label = read_agent_profile_control(&control, "label")?;
        let subject = policy_subject_from_label(label.trim()).ok_or_else(|| {
            CliError::unavailable("invalid agent label; cannot rebuild policy from profile")
        })?;
        let existing_model = read_agent_profile_control(&control, "model")?;
        let selected_model = model.unwrap_or_else(|| existing_model.trim().to_owned());
        if selected_model.is_empty() || !is_model_name(&selected_model) {
            return Err(CliError::usage(format!(
                "invalid model name: {selected_model}"
            )));
        }
        let existing_policy = read_agent_profile_control(&control, "policy")?;
        let policy =
            agent_apply_policy_text(&existing_policy, subject, &selected_model, &profile.tools);
        writes.push(("policy", ensure_profile_newline(&policy)));
    }

    if !profile.mounts.is_empty() {
        require_plain_agent_control_target(&control.join("mount"))?;
        let uid = current_uid_text().map_err(CliError::unavailable)?;
        let mount = agent_new_mount_control(&uid, name, &profile.mounts);
        writes.push(("mount", ensure_profile_newline(&mount)));
    }

    let updated = writes.iter().map(|entry| entry.0).collect::<Vec<_>>();
    for (file, content) in writes {
        let path = control.join(file);
        atomic_replace_text_with_mode(&path, &content, 0o600).map_err(|error| {
            CliError::unavailable(format!("cannot write {}: {error}", path.display()))
        })?;
    }

    if updated.is_empty() {
        print_line(&format!(
            "agent {} profile applied (no control fields in profile)",
            terminal_safe_text(name)
        ))?;
    } else {
        print_line(&format!(
            "agent {} profile applied {}",
            terminal_safe_text(name),
            updated.join(",")
        ))?;
    }
    Ok(ExitCode::SUCCESS)
}

fn validate_agent_apply_profile(name: &str, profile: &AgentProfile) -> Result<(), CliError> {
    if let Some(profile_name) = profile.name.as_deref() {
        require_cli_name("profile agent name", profile_name)?;
    }
    let args = AgentNewArgs {
        name: name.to_owned(),
        temporary: profile.temporary,
        parent: profile.parent.clone(),
        label: profile.label.clone(),
        models: profile.models.clone(),
        tools: profile.tools.clone(),
        shared: profile.shared.clone(),
        mounts: profile.mounts.clone(),
        instructions: profile.instructions.clone(),
        description: profile.description.clone(),
    };
    agent_new_request_json(&args).map(|_json| ())
}

fn require_plain_agent_control_target(path: &Path) -> Result<(), CliError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        CliError::unavailable(format!("cannot stat {}: {error}", path.display()))
    })?;
    let file_type = metadata.file_type();
    if file_type.is_dir()
        || file_type.is_symlink()
        || file_type.is_socket()
        || file_type.is_fifo()
        || file_type.is_block_device()
        || file_type.is_char_device()
    {
        return Err(CliError::unavailable(format!(
            "agent control is not a plain file: {}",
            path.display()
        )));
    }
    Ok(())
}

fn read_agent_profile_control(control: &Path, file: &str) -> Result<String, CliError> {
    let path = control.join(file);
    read_small_plain_text_file(
        &path,
        MAX_AGENT_PROFILE_CONTROL_BYTES,
        "agent profile control",
    )
    .map_err(|error| CliError::unavailable(format!("cannot read {}: {error}", path.display())))
}

fn merge_agent_profile_meta(existing: &str, description: &str) -> Result<String, CliError> {
    let mut value: serde_json::Value = serde_json::from_str(existing)
        .map_err(|error| CliError::usage(format!("invalid agent meta.json: {error}")))?;
    let Some(object) = value.as_object_mut() else {
        return Err(CliError::usage(
            "invalid agent meta.json: expected JSON object",
        ));
    };
    object.insert(
        "description".to_owned(),
        serde_json::Value::String(description.to_owned()),
    );
    object.insert(
        "source".to_owned(),
        serde_json::Value::String("profile".to_owned()),
    );
    serde_json::to_string(&value)
        .map_err(|error| CliError::usage(format!("invalid agent meta.json: {error}")))
}

fn ensure_profile_newline(value: &str) -> String {
    if value.ends_with('\n') {
        value.to_owned()
    } else {
        format!("{value}\n")
    }
}

/// Rebuilds policy from profile tools, or rewrites only the model allow line.
pub(crate) fn agent_apply_policy_text(
    existing: &str,
    subject: &str,
    model: &str,
    tools: &[String],
) -> String {
    if !tools.is_empty() {
        return agent_new_policy(subject, model, tools);
    }
    let mut lines = Vec::new();
    let mut wrote_model = false;
    for line in existing.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed
            .split_whitespace()
            .any(|part| part.starts_with("model:"))
        {
            if !wrote_model {
                lines.push(format!("allow {subject} model:{model} use"));
                wrote_model = true;
            }
            continue;
        }
        lines.push(trimmed.to_owned());
    }
    if !wrote_model {
        lines.insert(0, format!("allow {subject} model:{model} use"));
    }
    lines.join("\n")
}
