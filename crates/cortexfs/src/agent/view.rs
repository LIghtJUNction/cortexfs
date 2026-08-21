use crate::*;
use cortexfs_metadatas::{compaction_threshold_tokens, recommended_context_tokens};
use std::collections::BTreeSet;

use cortexfs_runtime_client::agent::is_agent_launch_abi;

use crate::support::plain::{open_plain_directory, read_small_text_file};

/// Derives an agent runtime view from the stable control files.
///
/// A same-named user agent control directory under
/// `CTX_HOME/agent/<agent_name>.d/` or
/// `ctx_root/home/<euid>/agent/<agent_name>.d/` shadows the global
/// `ctx_root/agent/<agent_name>.d/`. The private state directory
/// `home/<uid>/agent/<agent_name>/` remains the agent home/session tree, not
/// an executable object.
///
/// The returned environment always contains the runtime-owned `CTX_ROOT`,
/// `CTX_PROVIDER_CONFIG_DIR`, `CTX_HOME`, `HOME`, and `CTX_PATH` values derived
/// from the ABI controls.
/// The `env` control is validated as data, but the stable ABI does not let it add process
/// variables. Runtime environment authority is fixed here and by the sandbox
/// launcher so text config cannot expand the authority established by `path`,
/// `mount`, and `policy`.
const MAX_AGENT_RUNTIME_CONTROL_BYTES: u64 = 64 * 1024;

pub fn derive_agent_runtime_view(
    ctx_root: &Path,
    agent_name: &str,
) -> Result<AgentRuntimeView, AgentRuntimeViewError> {
    if !is_object_name(agent_name) {
        return Err(AgentRuntimeViewError::InvalidAgentName);
    }

    let control_dir = resolve_agent_runtime_control_dir(ctx_root, agent_name)?;
    let owner = parse_agent_number_control(&control_dir, AgentControlKind::Owner, "owner")?;
    let uid = parse_agent_number_control(&control_dir, AgentControlKind::Uid, "uid")?;
    let gid = parse_agent_number_control(&control_dir, AgentControlKind::Gid, "gid")?;
    let groups = parse_agent_groups_control(&control_dir)?;
    let identity = AgentUnixIdentity::new(uid, gid, groups);
    let permissions =
        AgentPermissions::parse_control(&read_required_agent_control(&control_dir, "perm")?)
            .ok_or_else(|| AgentRuntimeViewError::InvalidControlFile("perm".to_owned()))?;

    let label = read_required_agent_control_value(&control_dir, "label")?;
    let policy_subject = policy_subject_from_label(&label)
        .ok_or_else(|| AgentRuntimeViewError::InvalidControlFile("label".to_owned()))?
        .to_owned();

    let iso = parse_agent_vocab_value(&control_dir, AgentControlKind::Iso, "iso")?;
    let parent = parse_agent_parent_control(&control_dir)?;
    let lifecycle = read_agent_lifecycle_control_value(&control_dir)?;

    let root = parse_agent_absolute_path_control(&control_dir, "root")?;
    let cwd = parse_agent_absolute_path_control(&control_dir, "cwd")?;

    let raw_path = read_required_agent_control_value(&control_dir, "path")?;
    validate_agent_ctx_path(&raw_path)?;
    let tool_path = ToolPath::parse(&raw_path);

    let mount_content = read_required_agent_control(&control_dir, "mount")?;
    let mount_table = MountTable::parse(&mount_content)
        .map_err(|_error| AgentRuntimeViewError::InvalidControlFile("mount".to_owned()))?;

    let model = read_agent_model_control_value(&control_dir, agent_name)?;
    if !abi::path::is_model_reference(&model) {
        return Err(AgentRuntimeViewError::InvalidControlFile(
            "model".to_owned(),
        ));
    }
    let window_content = read_required_agent_control(&control_dir, "window")?;
    let window_setting = AgentWindowSetting::parse_control(&window_content)
        .ok_or_else(|| AgentRuntimeViewError::InvalidControlFile("window".to_owned()))?;
    let (model_limit, model_recommended, model_compact) =
        read_agent_model_context(ctx_root, &model)?;
    let effective_window = window_setting
        .resolve_with_recommendation(model_limit, model_recommended)
        .map_err(|_error| AgentRuntimeViewError::InvalidControlFile("window".to_owned()))?;
    let compact_content = match read_required_agent_control(&control_dir, "compact") {
        Ok(content) => content,
        Err(AgentRuntimeViewError::MissingControlFile(_)) => "auto\n".to_owned(),
        Err(error) => return Err(error),
    };
    let compact_setting = AgentWindowSetting::parse_control(&compact_content)
        .ok_or_else(|| AgentRuntimeViewError::InvalidControlFile("compact".to_owned()))?;
    let effective_compact = compact_setting
        .resolve_with_recommendation(effective_window, model_compact)
        .map_err(|_error| AgentRuntimeViewError::InvalidControlFile("compact".to_owned()))?;
    let loop_kind = read_agent_loop_control(&control_dir)?;

    let policy_content = read_required_agent_control(&control_dir, "policy")?;
    let policy = PolicyV0::parse(&policy_content)
        .map_err(|_error| AgentRuntimeViewError::InvalidControlFile("policy".to_owned()))?;
    let declared_tools = parse_agent_tools_control(&control_dir)?;
    if !is_agent_launch_abi(&read_required_agent_control_value(&control_dir, "abi")?) {
        return Err(AgentRuntimeViewError::InvalidControlFile("abi".to_owned()));
    }
    let approval = parse_agent_approval_control(&control_dir)?;

    let owner_text = owner.to_string();
    let ctx_home = cortexfs_paths::ctx_home_path(ctx_root, &owner_text);
    let home = cortexfs_paths::agent_home_path(ctx_root, &owner_text, agent_name);
    let env = derive_agent_runtime_env(&AgentRuntimeEnv {
        ctx_root,
        ctx_home: &ctx_home,
        home: &home,
        ctx_path: &raw_path,
        control_dir: &control_dir,
        effective_window,
        effective_compact,
        compact_setting,
        loop_kind: &loop_kind,
    })?;

    Ok(AgentRuntimeView {
        agent_name: agent_name.to_owned(),
        control_dir,
        ctx_root: ctx_root.to_path_buf(),
        ctx_home,
        home,
        owner,
        identity,
        permissions,
        label,
        policy_subject,
        iso,
        parent,
        lifecycle,
        root,
        cwd,
        env,
        tool_path,
        mount_table,
        model,
        model_limit,
        model_recommended,
        model_compact,
        window_setting,
        effective_window,
        compact_setting,
        effective_compact,
        loop_kind,
        policy,
        declared_tools,
        approval,
    })
}

fn read_agent_model_context(
    ctx_root: &Path,
    model: &str,
) -> Result<(ModelContextLimit, ModelContextLimit, ModelContextLimit), AgentRuntimeViewError> {
    let model_name = resolve_agent_model_name(ctx_root, model)?;
    let (provider, model) = model_name
        .split_once('/')
        .ok_or_else(|| AgentRuntimeViewError::InvalidControlFile("model".to_owned()))?;
    let limit = read_model_context_control(ctx_root, provider, model, "limit", true)?
        .ok_or_else(|| AgentRuntimeViewError::MissingControlFile("limit".to_owned()))?;
    let recommended = read_model_context_control(ctx_root, provider, model, "recommended", false)?
        .or_else(|| {
            limit
                .tokens()
                .map(recommended_context_tokens)
                .and_then(ModelContextLimit::known)
        })
        .unwrap_or(ModelContextLimit::Unknown);
    let compact = read_model_context_control(ctx_root, provider, model, "compact", false)?
        .or_else(|| {
            recommended
                .tokens()
                .map(compaction_threshold_tokens)
                .and_then(ModelContextLimit::known)
        })
        .unwrap_or(ModelContextLimit::Unknown);
    Ok((limit, recommended, compact))
}

fn resolve_agent_model_name(ctx_root: &Path, model: &str) -> Result<String, AgentRuntimeViewError> {
    if is_model_alias(model) {
        let alias = cortexfs_paths::model_root_path(ctx_root).join(model);
        let metadata = fs::symlink_metadata(&alias)
            .map_err(|_error| AgentRuntimeViewError::InvalidControlFile("model".to_owned()))?;
        if !metadata.file_type().is_symlink() {
            return Err(AgentRuntimeViewError::InvalidControlFile(
                "model".to_owned(),
            ));
        }
        let target = fs::read_link(alias)
            .map_err(|_error| AgentRuntimeViewError::InvalidControlFile("model".to_owned()))?;
        let target = target
            .to_str()
            .and_then(|target| {
                [
                    cortexfs_paths::model_root_path(ctx_root),
                    cortexfs_paths::model_root_path(&cortexfs_paths::ctx_root()),
                ]
                .into_iter()
                .find_map(|root| target.strip_prefix(&format!("{}/", root.display())))
            })
            .filter(|target| is_model_name(target))
            .ok_or_else(|| AgentRuntimeViewError::InvalidControlFile("model".to_owned()))?;
        Ok(target.to_owned())
    } else if is_model_name(model) {
        Ok(model.to_owned())
    } else {
        Err(AgentRuntimeViewError::InvalidControlFile(
            "model".to_owned(),
        ))
    }
}

fn read_model_context_control(
    ctx_root: &Path,
    provider: &str,
    model: &str,
    file: &str,
    required: bool,
) -> Result<Option<ModelContextLimit>, AgentRuntimeViewError> {
    let path = cortexfs_paths::model_control_path(ctx_root, provider, model).join(file);
    let content = match read_small_text_file(&path, MAX_AGENT_RUNTIME_CONTROL_BYTES) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !required => {
            return Ok(None);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(AgentRuntimeViewError::MissingControlFile(file.to_owned()));
        }
        Err(_error) => {
            return Err(AgentRuntimeViewError::CannotReadControl(file.to_owned()));
        }
    };
    ModelContextLimit::parse_control(&content)
        .map(Some)
        .ok_or_else(|| AgentRuntimeViewError::InvalidControlFile(file.to_owned()))
}

pub(crate) fn parse_agent_approval_control(
    control_dir: &Path,
) -> Result<AgentApprovalMode, AgentRuntimeViewError> {
    let approval = match read_required_agent_control_value(control_dir, "approval") {
        Ok(value) if value == "auto" => AgentApprovalMode::Auto,
        Ok(value) if value == "ask" => AgentApprovalMode::Ask,
        Err(AgentRuntimeViewError::MissingControlFile(_)) => AgentApprovalMode::Auto,
        Ok(_) => {
            return Err(AgentRuntimeViewError::InvalidControlFile(
                "approval".to_owned(),
            ));
        }
        Err(error) => return Err(error),
    };
    Ok(approval)
}

pub(crate) fn parse_agent_tools_control(
    control_dir: &Path,
) -> Result<BTreeSet<String>, AgentRuntimeViewError> {
    let content = match read_required_agent_control(control_dir, "tools") {
        Ok(content) => content,
        Err(AgentRuntimeViewError::MissingControlFile(_)) => return Ok(BTreeSet::default()),
        Err(error) => return Err(error),
    };
    if !inspect_agent_tools_control(&content).is_ok() {
        return Err(AgentRuntimeViewError::InvalidControlFile(
            "tools".to_owned(),
        ));
    }
    Ok(content
        .lines()
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

pub(crate) fn resolve_agent_runtime_control_dir(
    ctx_root: &Path,
    agent_name: &str,
) -> Result<PathBuf, AgentRuntimeViewError> {
    for control_dir in current_user_agent_control_dirs(ctx_root, agent_name) {
        if open_plain_directory(&control_dir).is_ok() {
            return Ok(control_dir);
        }
    }
    let control_dir = cortexfs_paths::agent_control_path(ctx_root, agent_name);
    if open_plain_directory(&control_dir).is_err() {
        return Err(AgentRuntimeViewError::MissingControlDirectory);
    }
    Ok(control_dir)
}

pub(crate) fn current_user_agent_control_dirs(ctx_root: &Path, agent_name: &str) -> Vec<PathBuf> {
    let mut controls = Vec::new();
    if let Some(ctx_home) = env::var_os("CTX_HOME").map(PathBuf::from)
        && ctx_home.starts_with(ctx_root)
    {
        controls.push(cortexfs_paths::agent_control_path(&ctx_home, agent_name));
    }
    let uid_home = cortexfs_paths::ctx_home_path(
        ctx_root,
        &nix::unistd::Uid::effective().as_raw().to_string(),
    );
    let uid_control = cortexfs_paths::agent_control_path(&uid_home, agent_name);
    if !controls.iter().any(|control| control == &uid_control) {
        controls.push(uid_control);
    }
    controls
}

pub(crate) fn parse_agent_number_control(
    control_dir: &Path,
    kind: AgentControlKind,
    file: &str,
) -> Result<u32, AgentRuntimeViewError> {
    let content = read_required_agent_control(control_dir, file)?;
    if !inspect_agent_control(kind, &content).is_ok() {
        return Err(AgentRuntimeViewError::InvalidControlFile(file.to_owned()));
    }
    required_single_agent_control_value(file, &content)?
        .parse::<u32>()
        .map_err(|_error| AgentRuntimeViewError::InvalidControlFile(file.to_owned()))
}

pub(crate) fn parse_agent_groups_control(
    control_dir: &Path,
) -> Result<Vec<u32>, AgentRuntimeViewError> {
    let content = read_required_agent_control(control_dir, "groups")?;
    if !inspect_agent_control(AgentControlKind::Groups, &content).is_ok() {
        return Err(AgentRuntimeViewError::InvalidControlFile(
            "groups".to_owned(),
        ));
    }
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            line.parse::<u32>()
                .map_err(|_error| AgentRuntimeViewError::InvalidControlFile("groups".to_owned()))
        })
        .collect()
}

pub(crate) fn parse_agent_vocab_value(
    control_dir: &Path,
    kind: AgentControlKind,
    file: &str,
) -> Result<String, AgentRuntimeViewError> {
    let content = read_required_agent_control(control_dir, file)?;
    if !inspect_agent_control(kind, &content).is_ok() {
        return Err(AgentRuntimeViewError::InvalidControlFile(file.to_owned()));
    }
    required_single_agent_control_value(file, &content)
}

pub(crate) fn read_agent_lifecycle_control_value(
    control_dir: &Path,
) -> Result<ChildLifecycle, AgentRuntimeViewError> {
    match parse_agent_vocab_value(control_dir, AgentControlKind::Life, "life") {
        Ok(lifecycle) => ChildLifecycle::parse(&lifecycle)
            .map_err(|_error| AgentRuntimeViewError::InvalidControlFile("life".to_owned())),
        Err(AgentRuntimeViewError::MissingControlFile(_)) => Ok(ChildLifecycle::Owned),
        Err(error) => Err(error),
    }
}

pub(crate) fn parse_agent_parent_control(
    control_dir: &Path,
) -> Result<Option<String>, AgentRuntimeViewError> {
    let content = read_required_agent_control(control_dir, "parent")?;
    if !inspect_agent_control(AgentControlKind::Parent, &content).is_ok() {
        return Err(AgentRuntimeViewError::InvalidControlFile(
            "parent".to_owned(),
        ));
    }
    let value = optional_single_agent_control_value("parent", &content)?;
    Ok(value.filter(|parent| !parent.is_empty()))
}

pub(crate) fn parse_agent_absolute_path_control(
    control_dir: &Path,
    file: &str,
) -> Result<PathBuf, AgentRuntimeViewError> {
    let value = read_required_agent_control_value(control_dir, file)?;
    if !is_stable_chroot_absolute_path(&value) {
        return Err(AgentRuntimeViewError::InvalidControlFile(file.to_owned()));
    }
    Ok(PathBuf::from(value))
}

pub(crate) struct AgentRuntimeEnv<'a> {
    ctx_root: &'a Path,
    ctx_home: &'a Path,
    home: &'a Path,
    ctx_path: &'a str,
    control_dir: &'a Path,
    effective_window: AgentEffectiveWindow,
    effective_compact: AgentEffectiveWindow,
    compact_setting: AgentWindowSetting,
    loop_kind: &'a AgentLoop,
}

pub(crate) fn derive_agent_runtime_env(
    config: &AgentRuntimeEnv<'_>,
) -> Result<Vec<(String, String)>, AgentRuntimeViewError> {
    let env_content = read_required_agent_control(config.control_dir, "env")?;
    let mut env = vec![
        ("CTX_ROOT".to_owned(), config.ctx_root.display().to_string()),
        (
            "CTX_PROVIDER_CONFIG_DIR".to_owned(),
            cortexfs_paths::shared_path(config.ctx_root, "providers.d")
                .display()
                .to_string(),
        ),
        ("CTX_HOME".to_owned(), config.ctx_home.display().to_string()),
        ("HOME".to_owned(), config.home.display().to_string()),
        ("PATH".to_owned(), support::command::TRUSTED_PATH.to_owned()),
        ("CTX_PATH".to_owned(), config.ctx_path.to_owned()),
        (
            "CTX_AGENT_LOOP".to_owned(),
            config.loop_kind.as_str().to_owned(),
        ),
        (
            "CTX_AGENT_COMPACT_SETTING".to_owned(),
            config.compact_setting.value(),
        ),
    ];
    if let Some(budget) = budget_from_effective(config.effective_window) {
        env.push((
            "CTX_CONTEXT_WINDOW_TOKENS".to_owned(),
            budget.tokens().to_string(),
        ));
        env.push((
            "CTX_CONTEXT_WINDOW_CHARS".to_owned(),
            budget.total_chars().to_string(),
        ));
    }
    if let Some(budget) = budget_from_effective(config.effective_compact) {
        env.push((
            "CTX_CONTEXT_COMPACTION_TOKENS".to_owned(),
            budget.tokens().to_string(),
        ));
    }
    let agent_env = parse_agent_env_control(&env_content)?;
    env.push((
        "CTX_AGENT_STEPS".to_owned(),
        agent_step_limit(&agent_env).to_string(),
    ));
    Ok(env)
}

/// Resolves the single agent tool-loop limit from validated runtime controls.
#[must_use]
pub(crate) fn agent_step_limit(env: &[(String, String)]) -> u8 {
    env.iter()
        .find_map(|entry| (entry.0 == "CTX_AGENT_STEPS").then_some(entry.1.as_str()))
        .and_then(|value| value.parse().ok())
        .filter(|steps: &u8| *steps > 0)
        .unwrap_or(abi::constants::DEFAULT_AGENT_STEPS)
}

fn read_agent_loop_control(control_dir: &Path) -> Result<AgentLoop, AgentRuntimeViewError> {
    match read_small_text_file(&control_dir.join("loop"), 256) {
        Ok(content) => AgentLoop::parse(&content)
            .ok_or_else(|| AgentRuntimeViewError::InvalidControlFile("loop".to_owned())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(AgentLoop::default()),
        Err(_error) => Err(AgentRuntimeViewError::CannotReadControl("loop".to_owned())),
    }
}

pub(crate) fn parse_agent_env_control(
    content: &str,
) -> Result<Vec<(String, String)>, AgentRuntimeViewError> {
    let mut env = Vec::new();
    for raw_line in content.lines().filter(|line| !line.trim().is_empty()) {
        let (key, value) = raw_line
            .split_once('=')
            .ok_or_else(|| AgentRuntimeViewError::InvalidControlFile("env".to_owned()))?;
        if !is_valid_env_key(key)
            || value.bytes().any(|byte| byte.is_ascii_control())
            || key == "CTX_AGENT_STEPS" && !value.parse::<u8>().is_ok_and(|steps| steps > 0)
        {
            return Err(AgentRuntimeViewError::InvalidControlFile("env".to_owned()));
        }
        env.push((key.to_owned(), value.to_owned()));
    }
    Ok(env)
}

pub(crate) fn validate_agent_ctx_path(value: &str) -> Result<(), AgentRuntimeViewError> {
    if value
        .split(':')
        .filter(|component| !component.is_empty())
        .all(is_stable_chroot_absolute_path)
    {
        Ok(())
    } else {
        Err(AgentRuntimeViewError::InvalidControlFile("path".to_owned()))
    }
}

pub(crate) fn read_required_agent_control_value(
    control_dir: &Path,
    file: &str,
) -> Result<String, AgentRuntimeViewError> {
    let content = read_required_agent_control(control_dir, file)?;
    required_single_agent_control_value(file, &content)
}

pub(crate) fn read_agent_model_control_value(
    control_dir: &Path,
    agent_name: &str,
) -> Result<String, AgentRuntimeViewError> {
    match read_required_agent_control_value(control_dir, "model") {
        Ok(model) => Ok(model),
        Err(AgentRuntimeViewError::MissingControlFile(_)) if is_worker_agent_name(agent_name) => {
            Ok(default_agent_model_for_name(agent_name).to_owned())
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn read_required_agent_control(
    control_dir: &Path,
    file: &str,
) -> Result<String, AgentRuntimeViewError> {
    let path = control_dir.join(file);
    read_small_text_file(&path, MAX_AGENT_RUNTIME_CONTROL_BYTES).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            AgentRuntimeViewError::MissingControlFile(file.to_owned())
        } else {
            AgentRuntimeViewError::CannotReadControl(file.to_owned())
        }
    })
}

pub(crate) fn required_single_agent_control_value(
    file: &str,
    content: &str,
) -> Result<String, AgentRuntimeViewError> {
    optional_single_agent_control_value(file, content)?
        .ok_or_else(|| AgentRuntimeViewError::InvalidControlFile(file.to_owned()))
}

pub(crate) fn optional_single_agent_control_value(
    file: &str,
    content: &str,
) -> Result<Option<String>, AgentRuntimeViewError> {
    let lines = content.lines().collect::<Vec<_>>();
    if lines.len() > 1 {
        return Err(AgentRuntimeViewError::InvalidControlFile(file.to_owned()));
    }
    let Some(raw_value) = lines.first() else {
        return Ok(None);
    };
    let value = raw_value.trim();
    if *raw_value != value || value.contains('\0') {
        return Err(AgentRuntimeViewError::InvalidControlFile(file.to_owned()));
    }
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value.to_owned()))
    }
}
