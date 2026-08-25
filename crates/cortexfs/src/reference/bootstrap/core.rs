use super::*;
use crate::abi::constants::DEFAULT_AGENT_STEPS;
use cortexfs_runtime_client::agent::AGENT_LAUNCH_ABI;

/// Materializes the documented reference tree under `root`.
///
/// This is a filesystem bootstrap helper for tests, local inspection, and
/// simple demos. It creates ABI-visible files, control directories, symlinks,
/// session skeletons, shared queue directories, and Unix socket path entries.
/// It does not start agents, models, MCP servers, providers, or a supervisor.
pub fn ensure_reference_tree(root: &Path) -> Result<ReferenceTreeBootstrap, ReferenceTreeError> {
    let plan = plan_reference_tree_upgrade(root);
    reject_unsupported_version(&plan)?;
    let agent_groups = reference_agent_groups(1000, 1000);
    create_reference_root(root)?;
    ensure_reference_bin(root)?;
    for agent in REFERENCE_AGENTS {
        if plan.actions.iter().any(|action| {
            matches!(
                action,
                BootstrapAction::SkipAgent { name, .. } if name == agent.name
            )
        }) {
            continue;
        }
        ensure_reference_agent(root, agent.name, agent.parent, &agent_groups)?;
    }
    ensure_reference_agent_aliases(root)?;
    ensure_reference_global_tools(root)?;
    ensure_reference_channels(root)?;
    ensure_reference_docs(root)?;
    ensure_reference_home(root)?;
    migrate_reference_legacy_session_meta_models(root)?;
    apply_precomputed_reference_tree_upgrade(root, plan)?;
    Ok(ReferenceTreeBootstrap::new(root.to_path_buf()))
}

/// Materializes runtime-visible model wrappers for models projected from
/// provider configuration and cache files.
///
/// The FUSE projection can expose provider models virtually, but agent
/// sandboxes bind the backing source tree at `/ctx`. This helper keeps the
/// backing source tree aligned for runtime execution without making the pure
/// reference-tree bootstrap depend on host provider state.
pub fn ensure_runtime_models(root: &Path) -> Result<(), ReferenceTreeError> {
    ensure_runtime_models_from(
        root,
        Path::new(SYSTEM_PROVIDER_CONFIG_DIR),
        Path::new(SYSTEM_PROVIDER_MODEL_CACHE_DIR),
    )
}

/// Materializes runtime-visible model wrappers from explicit provider
/// configuration and cache directories.
pub fn ensure_runtime_models_from(
    root: &Path,
    config_dir: &Path,
    cache_dir: &Path,
) -> Result<(), ReferenceTreeError> {
    ensure_reference_debug_model(root)?;
    let snapshot =
        reference::reconcile::reconcile_provider_model_tree(root, config_dir, cache_dir)?;
    ensure_reference_model_aliases(root, &snapshot)
}

pub(crate) struct ReferenceAgentSpec {
    pub(crate) name: &'static str,
    pub(crate) parent: Option<&'static str>,
    pub(crate) model: &'static str,
}

pub(crate) const REFERENCE_AGENTS: &[ReferenceAgentSpec] = &[
    ReferenceAgentSpec {
        name: "architect",
        parent: None,
        model: DEFAULT_MODEL_ALIAS,
    },
    ReferenceAgentSpec {
        name: "executor",
        parent: Some("agent:architect"),
        model: DEFAULT_MODEL_ALIAS,
    },
    ReferenceAgentSpec {
        name: "product-manager",
        parent: Some("agent:architect"),
        model: DEFAULT_MODEL_ALIAS,
    },
];
pub(crate) const REFERENCE_OBJECT_RUNNER: &str = CORTEXFS_OBJECT_RUNNER;

pub(crate) fn ensure_reference_debug_model(root: &Path) -> Result<(), ReferenceTreeError> {
    install_executable_object_wrapper(
        root,
        ObjectClass::Model,
        DEBUG_ECHO_MODEL,
        REFERENCE_OBJECT_RUNNER,
        &[
            ("id", DEBUG_ECHO_MODEL),
            (
                "metadata.json",
                r#"{"metadata":{"id":"debug/echo","name":"debug/echo","provider":"debug"},"schema":"cortexfs.model-metadata/v2"}"#,
            ),
            ("driver", "default=debug\nexec=debug\nagent=debug"),
            ("cap", "chat\nstream"),
            ("effort", "auto"),
            ("limit", "unknown"),
            ("default", ""),
            ("session", "none"),
            ("status", "idle"),
            ("log", ""),
        ],
    )
    .map(|_object| ())
    .map_err(ReferenceTreeError::Object)
}

pub(crate) fn ensure_reference_model_aliases(
    root: &Path,
    snapshot: &ProviderSnapshot,
) -> Result<(), ReferenceTreeError> {
    for alias in MODEL_ALIASES {
        ensure_reference_model_alias(
            &cortexfs_paths::model_root_path(root).join(alias),
            alias,
            snapshot,
        )?;
    }
    Ok(())
}

pub(crate) fn ensure_reference_docs(root: &Path) -> Result<(), ReferenceTreeError> {
    let docs = cortexfs_paths::shared_path(root, MANUAL_SHARED_DIR);
    let man = docs.join(MANUAL_MAN_DIR);
    create_reference_dir(&docs)?;
    create_reference_dir(&man)?;
    write_reference_text(&docs.join(MANUAL_INDEX_FILE), MANUAL_INDEX)?;
    for manual in MANUALS {
        write_reference_text(&man.join(manual.file_name), manual.content)?;
    }
    Ok(())
}

pub(crate) fn create_reference_root(root: &Path) -> Result<(), ReferenceTreeError> {
    for entry in ROOT_ENTRIES {
        match *entry {
            "status" => write_reference_text(&cortexfs_paths::status_path(root), "ready\n")?,
            directory => create_reference_dir(
                &cortexfs_paths::root_entry_path(root, directory)
                    .ok_or(ReferenceTreeError::CannotCreate)?,
            )?,
        }
    }
    Ok(())
}

pub(crate) fn ensure_reference_bin(root: &Path) -> Result<(), ReferenceTreeError> {
    let ctx = cortexfs_paths::bin_root_path(root).join("ctx");
    write_reference_text(
        &ctx,
        "#!/bin/sh\n# CortexFS reference-tree ctx placeholder.\nexec /usr/bin/ctx \"$@\"\n",
    )?;
    set_reference_executable(&ctx)?;
    for name in [
        "ctxterm",
        "tsh",
        "cortexfs-object-runner",
        "cortexfs-channel-tool",
    ] {
        let path = cortexfs_paths::bin_root_path(root).join(name);
        write_reference_text(
            &path,
            &format!(
                "#!/bin/sh\n# CortexFS reference-tree {name} placeholder.\nexec /usr/bin/{name} \"$@\"\n"
            ),
        )?;
        set_reference_executable(&path)?;
    }
    Ok(())
}

pub(crate) fn ensure_reference_agent(
    root: &Path,
    name: &str,
    parent: Option<&str>,
    groups: &str,
) -> Result<(), ReferenceTreeError> {
    install_executable_object_wrapper(root, ObjectClass::Agent, name, support::command::FALSE, &[])
        .map_err(ReferenceTreeError::Object)?;
    let control = cortexfs_paths::agent_control_path(root, name);
    let label = format!("user_u:agent_r:{name}_t:s0\n");
    let ctx_root = cortexfs_paths::ctx_root();
    let home_root = format!(
        "{}\n",
        cortexfs_paths::agent_home_path(&ctx_root, "1000", name)
            .join("root")
            .display()
    );
    let policy_subject = format!("{name}_t");
    let policy = reference_agent_policy(&policy_subject, name);
    let mount = format!(
        "{root}\t{root}\tro\trbind,nosuid,nodev\n{}\t/home/agent\trw\trbind,nosuid,nodev\n",
        cortexfs_paths::agent_home_path(&ctx_root, "1000", name).display(),
        root = ctx_root.display(),
    );
    let overrides = [
        ("owner", "1000\n".to_owned()),
        ("uid", "1000\n".to_owned()),
        ("gid", "1000\n".to_owned()),
        ("groups", groups.to_owned()),
        (
            "perm",
            if reference_agent_can_write_source(name) {
                "rwx\n"
            } else {
                "r--\n"
            }
            .to_owned(),
        ),
        ("label", label),
        ("iso", "shared\n".to_owned()),
        (
            "parent",
            parent.map_or_else(|| "\n".to_owned(), |value| format!("{value}\n")),
        ),
        ("life", "owned\n".to_owned()),
        ("abi", format!("{AGENT_LAUNCH_ABI}\n")),
        ("root", home_root),
        ("cwd", "/workspace\n".to_owned()),
        (
            "env",
            format!("CTX_ROOT={CTX_ROOT}\nCTX_AGENT_STEPS={DEFAULT_AGENT_STEPS}\n"),
        ),
        (
            "path",
            format!(
                "{}:{}\n",
                cortexfs_paths::tool_root_path(&ctx_root).display(),
                cortexfs_paths::home_tool_path(&ctx_root, "1000").display()
            ),
        ),
        ("mount", mount),
        ("model", format!("{}\n", reference_agent_model(name))),
        ("window", "auto\n".to_owned()),
        ("system.md", reference_agent_system_prompt(name)),
        (
            "prompt.template.md",
            DEFAULT_AGENT_PROMPT_TEMPLATE.to_owned(),
        ),
        ("policy", policy),
        ("status", "idle\n".to_owned()),
        ("pid", "\n".to_owned()),
        ("log", "\n".to_owned()),
        ("meta.json", "{}\n".to_owned()),
    ];
    for (file, content) in overrides {
        write_reference_text(
            &cortexfs_paths::agent_control_file_path(root, name, file),
            &content,
        )?;
    }
    write_reference_text(
        &cortexfs_paths::agent_path(root, name),
        &reference_agent_wrapper_script(name),
    )?;
    set_reference_executable(&cortexfs_paths::agent_path(root, name))?;
    let uid = read_reference_owner_id(&cortexfs_paths::agent_control_file_path(root, name, "uid"))?;
    let gid = read_reference_owner_id(&cortexfs_paths::agent_control_file_path(root, name, "gid"))?;
    ensure_reference_socket(&cortexfs_paths::agent_socket_path(root, name), uid, gid)?;
    ensure_reference_agent_control_ownership(&control)
}

fn reference_agent_groups(uid: u32, primary_gid: u32) -> String {
    let primary = nix::unistd::Gid::from_raw(primary_gid);
    let groups = active_user_manager_groups(uid, primary_gid)
        .or_else(|| {
            nix::unistd::User::from_uid(nix::unistd::Uid::from_raw(uid))
                .ok()
                .flatten()
                .and_then(|user| std::ffi::CString::new(user.name).ok())
                .and_then(|name| nix::unistd::getgrouplist(&name, primary).ok())
                .map(|groups| groups.into_iter().map(nix::unistd::Gid::as_raw).collect())
        })
        .unwrap_or_else(|| vec![primary_gid]);
    format_reference_agent_groups(groups)
}

fn active_user_manager_groups(uid: u32, gid: u32) -> Option<Vec<u32>> {
    let unit = format!("user@{uid}.service");
    let output = Command::new("systemctl")
        .args(["show", "--property=MainPID", "--value", unit.as_str()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let pid = std::str::from_utf8(&output.stdout)
        .ok()?
        .trim()
        .parse::<u32>()
        .ok()?;
    if pid == 0 {
        return None;
    }
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    parse_user_manager_groups(&status, uid, gid)
}

fn parse_user_manager_groups(status: &str, uid: u32, gid: u32) -> Option<Vec<u32>> {
    let parse_ids = |label: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(label))?
            .split_whitespace()
            .map(str::parse::<u32>)
            .collect::<Result<Vec<_>, _>>()
            .ok()
    };
    let uids = parse_ids("Uid:")?;
    let gids = parse_ids("Gid:")?;
    if uids.len() != 4
        || gids.len() != 4
        || uids.iter().any(|value| *value != uid)
        || gids.iter().any(|value| *value != gid)
    {
        return None;
    }
    status
        .lines()
        .find_map(|line| line.strip_prefix("Groups:"))?
        .split_whitespace()
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()
}

fn format_reference_agent_groups(groups: impl IntoIterator<Item = u32>) -> String {
    let mut groups = groups.into_iter().collect::<Vec<_>>();
    groups.sort_unstable();
    groups.dedup();
    let mut content = groups
        .into_iter()
        .map(|group| group.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    content.push('\n');
    content
}

pub(crate) fn reference_agent_wrapper_script(name: &str) -> String {
    let runner = REFERENCE_OBJECT_RUNNER;
    format!(
        r#"#!/bin/sh
# CortexFS generated object wrapper.
# cortexfs.object=agent
# cortexfs.name={name}
exec '{runner}' "$0" "$@"
"#
    )
}

pub(crate) fn reference_agent_policy(policy_subject: &str, name: &str) -> String {
    let model = reference_agent_model(name);
    let mut policy = format!(
        "allow {policy_subject} model:{model} use\n\
         allow {policy_subject} network:default connect\n\
         allow {policy_subject} tool:tsh execute\n\
         allow {policy_subject} tool:fs.read execute\n\
         allow {policy_subject} tool:fs.list execute\n\
         allow {policy_subject} tool:fs.stat execute\n\
         allow {policy_subject} tool:agent.update execute\n"
    );
    if reference_agent_can_write_source(name) {
        let _ignored = std::fmt::Write::write_fmt(
            &mut policy,
            format_args!(
                "allow {policy_subject} tool:fs.write execute\n\
                 allow {policy_subject} tool:fs.replace execute\n\
                 allow {policy_subject} tool:shell.exec execute\n\
                 allow {policy_subject} tool:bash execute\n"
            ),
        );
    }
    let mut channel_tools = HashSet::new();
    channel_tools.extend(
        cortexfs_channels::COMMON_CHANNEL_TOOLS
            .iter()
            .map(|tool| (*tool).to_owned()),
    );
    for channel in cortexfs_channels::CHANNEL_CATALOG {
        channel_tools.extend(channel.platform_tool_names());
        channel_tools.insert(format!("{}.invoke", channel.id));
    }
    let mut channel_tools = channel_tools.into_iter().collect::<Vec<_>>();
    channel_tools.sort();
    for tool in channel_tools {
        let _ignored = std::fmt::Write::write_fmt(
            &mut policy,
            format_args!("allow {policy_subject} tool:{tool} execute\n"),
        );
    }
    for child in reference_agent_children(name) {
        let _ignored = std::fmt::Write::write_fmt(
            &mut policy,
            format_args!(
                "allow {policy_subject} agent:{child} create\n\
                 allow {policy_subject} agent:{child} start\n\
                 allow {policy_subject} agent:{child} stop\n\
                 allow {policy_subject} agent:{child} read\n"
            ),
        );
    }
    policy
}

pub(crate) fn reference_agent_children(name: &str) -> Vec<&'static str> {
    if name == "architect" {
        return REFERENCE_AGENTS
            .iter()
            .filter(|agent| agent.parent == Some("agent:architect"))
            .map(|agent| agent.name)
            .collect();
    }
    Vec::new()
}

pub(crate) fn reference_agent_can_write_source(name: &str) -> bool {
    name == "executor"
}

pub(crate) fn reference_agent_model(name: &str) -> &'static str {
    REFERENCE_AGENTS
        .iter()
        .find(|agent| agent.name == name)
        .map_or(DEFAULT_MODEL_ALIAS, |agent| agent.model)
}

const SELF_ITERATION_PROMPT_LINE: &str = "\
You may iterate yourself: call `agent.update` through `tsh` to atomically replace your own `system.md` or `prompt.template.md`; the new prompt applies from your next run, and prompt text never grants authority.
";

pub(crate) fn reference_agent_system_prompt(name: &str) -> String {
    let mut prompt = match name {
        "architect" => "\
You are CortexFS agent `architect`.
Your human role name is Architect.
Act as the parent planner and architecture coordinator for the default agent tree.
Keep task decomposition explicit in session files; delegate implementation and verification to `executor`, and use `product-manager` to clarify user value, scope, and acceptance criteria.
Minimize coordination cost: merge small work, split only when implementation and review responsibilities are genuinely distinct.
Preserve the stable CortexFS ABI shape; do not add root namespaces, background schedulers, polling loops, watchers, or hot reload paths.
Prefer concrete files, command evidence, and current repository state over speculative architecture.
"
        .to_owned(),
        "product-manager" => "\
You are CortexFS agent `product-manager`.
You clarify the user problem before implementation: target users, desired outcome, scope, non-goals, risks, and acceptance criteria.
Keep requirements testable and concise. Do not edit source or prescribe implementation details; hand a complete product brief to `architect` and `executor`.
Return open questions and measurable success criteria before proposing extra scope.
"
        .to_owned(),
        "executor" => "\
You are CortexFS agent `executor`.
You are the implementation and verification agent in the default Architect -> executor flow.
The default startup surface is a writable project checkout mounted at `/workspace`; use `tsh` to call `fs.read`, `fs.replace`, `fs.write`, and `shell.exec` for reviewable changes and evidence.
Read the applicable rules and current workspace state before editing. Prefer the smallest atomic change, run focused formatter/check/lint/tests, and report exact commands and results.
Do not invent a result, overwrite unrelated user changes, or expand scope. Ask before destructive or ambiguous actions.
Preserve schedule handoff role and paths for `role=`, `plan=`, `handoff=`, `result=`, and `refs=` instead of creating a second coordination surface.
"
        .to_owned(),
        _ => format!("You are CortexFS agent `{name}`.\n"),
    };
    if matches!(name, "architect" | "executor" | "product-manager") {
        prompt.push_str(SELF_ITERATION_PROMPT_LINE);
    }
    prompt
}

pub(crate) fn ensure_reference_global_tools(root: &Path) -> Result<(), ReferenceTreeError> {
    for tool in REFERENCE_GLOBAL_TOOLS {
        install_executable_object_wrapper(
            root,
            ObjectClass::Tool,
            tool.name,
            tool.wrapper_target,
            &[
                ("name", tool.name),
                ("description", tool.description),
                ("schema", tool.schema),
                ("cap", tool.cap),
                ("policy", tool.policy),
                ("status", "idle"),
                ("log", ""),
            ],
        )
        .map_err(ReferenceTreeError::Object)?;
        if let Some(script) = reference_tool_stub_script(tool.name) {
            write_reference_text(&cortexfs_paths::tool_path(root, tool.name), &script)?;
            set_reference_executable(&cortexfs_paths::tool_path(root, tool.name))?;
        }
        if tool.name == "tsh" {
            write_reference_text(&cortexfs_paths::tool_config_path(root), DEFAULT_TSH_CONFIG)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod reference_model_tests {
    use super::*;
    use crate::reference::reconcile::reconcile_provider_model_tree;

    #[test]
    fn user_manager_status_groups_require_matching_identity() {
        let status = "Name:\tsystemd\nUid:\t1000\t1000\t1000\t1000\nGid:\t1000\t1000\t1000\t1000\nGroups:\t1000 985 998\n";

        assert_eq!(
            parse_user_manager_groups(status, 1000, 1000),
            Some(vec![1000, 985, 998])
        );
        assert_eq!(parse_user_manager_groups(status, 1001, 1000), None);
        assert_eq!(parse_user_manager_groups(status, 1000, 1001), None);
    }

    #[test]
    fn reference_agent_groups_are_sorted_deduplicated_and_terminated() {
        assert_eq!(
            format_reference_agent_groups([1002, 1000, 1001, 1002]),
            "1000\n1001\n1002\n"
        );
    }

    #[test]
    fn main_agent_alias_points_to_canonical_agent() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        ensure_reference_tree(root.path())
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;

        assert_eq!(
            fs::read_link(root.path().join("agent/main"))?,
            PathBuf::from("/ctx/agent/executor")
        );
        assert_eq!(
            fs::read_link(root.path().join("agent/main.sock"))?,
            PathBuf::from("/ctx/agent/executor.sock")
        );
        Ok(())
    }

    #[test]
    fn provider_models_are_materialized_into_reference_tree()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let config_dir = tempfile::tempdir()?;
        let cache_dir = tempfile::tempdir()?;
        fs::write(
            config_dir.path().join("api.test.json"),
            r#"{
  "name": "api.test",
  "base_url": "https://api.test/v1",
  "default_model": "gpt-5.6-terra",
  "enabled": true,
  "formats": ["openai.chat", "openai.responses"]
}
"#,
        )?;
        fs::write(
            cache_dir.path().join("api.test.models.json"),
            r#"{"models":["gpt-5.6-terra"]}"#,
        )?;

        create_reference_root(root.path())
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        ensure_reference_bin(root.path())
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        reconcile_provider_model_tree(root.path(), config_dir.path(), cache_dir.path())
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;

        assert!(root.path().join("model/api.test/gpt-5.6-terra").is_file());
        assert!(
            !root
                .path()
                .join("model/api.test/gpt-5.6-terra.d/hooks")
                .exists()
        );
        assert_eq!(
            fs::read_to_string(root.path().join("model/api.test/gpt-5.6-terra.d/id"))?,
            "api.test/gpt-5.6-terra\n"
        );
        Ok(())
    }

    #[test]
    fn runtime_model_aliases_point_to_materialized_reference_models()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        create_reference_root(root.path())
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        ensure_reference_bin(root.path())
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        ensure_reference_debug_model(root.path())
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        let config_dir = tempfile::tempdir()?;
        let cache_dir = tempfile::tempdir()?;
        fs::write(
            config_dir.path().join("api.test.json"),
            r#"{"name":"api.test","base_url":"https://api.test/v1","default_model":"gpt-main","models":["gpt-5.6-sol","turbo-fast","deep","code-pro","multimodal"]}"#,
        )?;
        let snapshot =
            reconcile_provider_model_tree(root.path(), config_dir.path(), cache_dir.path())
                .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        ensure_reference_model_aliases(root.path(), &snapshot)
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;

        assert_eq!(
            fs::read_link(root.path().join("model/main"))?,
            PathBuf::from("/ctx/model/api.test/gpt-main")
        );
        assert_eq!(
            fs::read_link(root.path().join("model/helper"))?,
            PathBuf::from("/ctx/model/api.test/gpt-5.6-sol")
        );
        for (alias, target) in [("fast", "turbo-fast"), ("code", "code-pro")] {
            assert_eq!(
                fs::read_link(root.path().join("model").join(alias))?,
                PathBuf::from(format!("/ctx/model/api.test/{target}"))
            );
        }
        Ok(())
    }

    #[test]
    fn runtime_model_aliases_fall_back_without_projected_provider_default()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        create_reference_root(root.path())
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        ensure_reference_bin(root.path())
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        ensure_reference_debug_model(root.path())
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        let config_dir = tempfile::tempdir()?;
        let cache_dir = tempfile::tempdir()?;
        fs::write(
            config_dir.path().join("missing.json"),
            r#"{"name":"missing","base_url":"https://missing.test/v1"}"#,
        )?;
        fs::write(
            config_dir.path().join("disabled.json"),
            r#"{"name":"disabled","base_url":"https://disabled.test/v1","default_model":"ignored","enabled":false}"#,
        )?;
        let snapshot =
            reconcile_provider_model_tree(root.path(), config_dir.path(), cache_dir.path())
                .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        ensure_reference_model_aliases(root.path(), &snapshot)
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;

        assert_eq!(
            fs::read_link(root.path().join("model/main"))?,
            PathBuf::from("/ctx/model/debug/echo")
        );
        assert_eq!(
            fs::read_link(root.path().join("model/helper"))?,
            PathBuf::from("/ctx/model/debug/echo")
        );
        for alias in MODEL_ALIASES {
            assert_eq!(
                fs::read_link(root.path().join("model").join(alias))?,
                PathBuf::from("/ctx/model/debug/echo")
            );
        }
        Ok(())
    }
}
