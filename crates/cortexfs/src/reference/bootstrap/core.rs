use super::*;

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
    ensure_reference_global_tools(root)?;
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
    ensure_reference_models(root)
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
        name: "coder",
        parent: Some("agent:architect"),
        model: DEFAULT_MODEL_ALIAS,
    },
    ReferenceAgentSpec {
        name: "reviewer",
        parent: Some("agent:architect"),
        model: DEFAULT_MODEL_ALIAS,
    },
    ReferenceAgentSpec {
        name: "worker",
        parent: Some("agent:architect"),
        model: DEFAULT_WORKER_MODEL,
    },
];
pub(crate) const REFERENCE_OBJECT_RUNNER: &str = "/ctx/bin/cortexfs-object-runner";

pub(crate) fn ensure_reference_models(root: &Path) -> Result<(), ReferenceTreeError> {
    ensure_reference_debug_model(root)?;
    let models = ensure_reference_provider_models_from(
        root,
        Path::new(SYSTEM_PROVIDER_CONFIG_DIR),
        Path::new(SYSTEM_PROVIDER_MODEL_CACHE_DIR),
    )?;
    ensure_reference_model_aliases(root, &models)
}

pub(crate) fn ensure_reference_debug_model(root: &Path) -> Result<(), ReferenceTreeError> {
    install_executable_object_wrapper(
        root,
        ObjectClass::Model,
        DEBUG_ECHO_MODEL,
        REFERENCE_OBJECT_RUNNER,
        &[
            ("id", DEBUG_ECHO_MODEL),
            ("driver", "default=debug\nexec=debug\nagent=debug"),
            ("cap", "chat\nstream"),
            ("effort", "auto"),
            ("limit", "unknown"),
            ("default", ""),
            ("fallback", ""),
            ("session", "none"),
            ("status", "idle"),
            ("log", ""),
        ],
    )
    .map(|_object| ())
    .map_err(ReferenceTreeError::Object)
}

pub(crate) fn ensure_reference_provider_models_from(
    root: &Path,
    config_dir: &Path,
    cache_dir: &Path,
) -> Result<Vec<ProjectedProviderModel>, ReferenceTreeError> {
    let models = projected_provider_models(config_dir, cache_dir)
        .map_err(|_error| ReferenceTreeError::CannotCreate)?;
    for model in &models {
        ensure_reference_provider_model(root, model)?;
    }
    Ok(models)
}

pub(crate) fn ensure_reference_provider_model(
    root: &Path,
    model: &ProjectedProviderModel,
) -> Result<(), ReferenceTreeError> {
    let name = format!("{}/{}", model.provider, model.model);
    let controls = MODEL_CONTROL_FILES
        .iter()
        .filter_map(|file| {
            provider_model_control_content(model, file).map(|content| (*file, content))
        })
        .collect::<Vec<_>>();
    let overrides = controls
        .iter()
        .map(|entry| (entry.0, entry.1.as_str()))
        .collect::<Vec<_>>();
    install_executable_object_wrapper(
        root,
        ObjectClass::Model,
        &name,
        REFERENCE_OBJECT_RUNNER,
        &overrides,
    )
    .map(|_object| ())
    .map_err(ReferenceTreeError::Object)
}

pub(crate) fn ensure_reference_model_aliases(
    root: &Path,
    models: &[ProjectedProviderModel],
) -> Result<(), ReferenceTreeError> {
    let main = reference_model_alias_target(root, DEFAULT_MODEL_ALIAS_TARGET, models, None);
    let helper = reference_model_alias_target(
        root,
        HELPER_MODEL_ALIAS_TARGET,
        models,
        Some("codex-auto-review"),
    );
    for (alias, target) in MODEL_ALIASES.iter().copied().map(|alias| {
        let target = match alias {
            DEFAULT_MODEL_ALIAS => main.clone(),
            HELPER_MODEL_ALIAS => helper.clone(),
            alias => capability_model_alias_target(alias, models).unwrap_or_else(|| main.clone()),
        };
        (alias, target)
    }) {
        ensure_reference_model_alias(&root.join("model").join(alias), Path::new(&target))?;
    }
    Ok(())
}

fn capability_model_alias_target(alias: &str, models: &[ProjectedProviderModel]) -> Option<String> {
    models
        .iter()
        .find(|model| match alias {
            "fast" => model_name_has_word(&model.model, "fast"),
            "reason" => model.cap.lines().any(|cap| cap.trim() == "reasoning"),
            "code" => ["code", "coder", "coding"]
                .iter()
                .any(|word| model_name_has_word(&model.model, word)),
            "vision" => model
                .cap
                .lines()
                .any(|cap| matches!(cap.trim(), "vision" | "image_input")),
            _ => false,
        })
        .map(|model| format!("/ctx/model/{}/{}", model.provider, model.model))
}

fn model_name_has_word(model: &str, expected: &str) -> bool {
    model
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|word| word.eq_ignore_ascii_case(expected))
}

pub(crate) fn reference_model_alias_target(
    root: &Path,
    preferred: &str,
    models: &[ProjectedProviderModel],
    preferred_model: Option<&str>,
) -> String {
    if reference_model_target_exists(root, preferred) {
        return preferred.to_owned();
    }
    if let Some(model_name) = preferred_model
        && let Some(model) = models.iter().find(|model| model.model == model_name)
    {
        return format!("/ctx/model/{}/{}", model.provider, model.model);
    }
    models.first().map_or_else(
        || format!("/ctx/model/{DEBUG_ECHO_MODEL}"),
        |model| format!("/ctx/model/{}/{}", model.provider, model.model),
    )
}

pub(crate) fn reference_model_target_exists(root: &Path, target: &str) -> bool {
    let Some(model) = target.strip_prefix("/ctx/model/") else {
        return false;
    };
    fs::symlink_metadata(root.join("model").join(model)).is_ok_and(|metadata| metadata.is_file())
}

pub(crate) fn ensure_reference_docs(root: &Path) -> Result<(), ReferenceTreeError> {
    let docs = root.join("shared").join(MANUAL_SHARED_DIR);
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
            "status" => write_reference_text(&root.join("status"), "ready\n")?,
            directory => create_reference_dir(&root.join(directory))?,
        }
    }
    Ok(())
}

pub(crate) fn ensure_reference_bin(root: &Path) -> Result<(), ReferenceTreeError> {
    let ctx = root.join("bin").join("ctx");
    write_reference_text(
        &ctx,
        "#!/bin/sh\n# CortexFS reference-tree ctx placeholder.\nexec /usr/bin/ctx \"$@\"\n",
    )?;
    set_reference_executable(&ctx)?;
    for name in ["ctxterm", "tsh", "cortexfs-object-runner"] {
        let path = root.join("bin").join(name);
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
    install_executable_object_wrapper(
        root,
        ObjectClass::Agent,
        name,
        crate::support::command::FALSE,
        &[],
    )
        .map_err(ReferenceTreeError::Object)?;
    let control = root.join("agent").join(format!("{name}.d"));
    let label = format!("user_u:agent_r:{name}_t:s0\n");
    let home_root = format!("/ctx/home/1000/agent/{name}/root\n");
    let policy_subject = format!("{name}_t");
    let policy = reference_agent_policy(&policy_subject, name);
    let mount = format!(
        "/ctx\t/ctx\tro\trbind,nosuid,nodev\n/ctx/home/1000/agent/{name}\t/home/agent\trw\trbind,nosuid,nodev\n"
    );
    let overrides = [
        ("owner", "1000\n".to_owned()),
        ("uid", "1000\n".to_owned()),
        ("gid", "1000\n".to_owned()),
        ("groups", groups.to_owned()),
        ("label", label),
        ("iso", "shared\n".to_owned()),
        (
            "parent",
            parent.map_or_else(|| "\n".to_owned(), |value| format!("{value}\n")),
        ),
        ("life", "owned\n".to_owned()),
        ("root", home_root),
        ("cwd", "/workspace\n".to_owned()),
        ("env", "CTX_ROOT=/ctx\n".to_owned()),
        ("path", "/ctx/tool:/ctx/home/1000/tool\n".to_owned()),
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
        write_reference_text(&control.join(file), &content)?;
    }
    write_reference_text(
        &root.join("agent").join(name),
        &reference_agent_wrapper_script(name),
    )?;
    set_reference_executable(&root.join("agent").join(name))?;
    let uid = read_reference_owner_id(&control.join("uid"))?;
    let gid = read_reference_owner_id(&control.join("gid"))?;
    ensure_reference_socket(&root.join("agent").join(format!("{name}.sock")), uid, gid)?;
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
         allow {policy_subject} tool:tsh execute\n\
         allow {policy_subject} tool:fs.read execute\n"
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
    } else if name == "executor" {
        let _ignored = std::fmt::Write::write_fmt(
            &mut policy,
            format_args!(
                "allow {policy_subject} tool:shell.exec execute\n\
                 allow {policy_subject} tool:bash execute\n"
            ),
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
    matches!(name, "coder" | "worker")
}

pub(crate) fn reference_agent_model(name: &str) -> &'static str {
    REFERENCE_AGENTS
        .iter()
        .find(|agent| agent.name == name)
        .map_or(DEFAULT_MODEL_ALIAS, |agent| agent.model)
}

pub(crate) fn reference_agent_system_prompt(name: &str) -> String {
    match name {
        "architect" => "\
You are CortexFS agent `architect`.
Your human role name is Architect.
Act as the parent planner and architecture coordinator for the default agent tree.
Keep task decomposition explicit in session files; delegate implementation to `coder` as the primary implementer, simple bounded execution to `worker`, and independent verification to `reviewer`.
Minimize coordination cost: merge small work, split only when implementation and review responsibilities are genuinely distinct.
Preserve the stable CortexFS ABI shape; do not add root namespaces, background schedulers, polling loops, watchers, or hot reload paths.
Prefer concrete files, command evidence, and current repository state over speculative architecture.
"
        .to_owned(),
        "coder" => "\
You are CortexFS agent `coder`.
You are the implementation agent in the default Architect -> coder/reviewer flow.
The default startup surface is a writable project checkout mounted at `/workspace`; use `tsh` to call `fs.read`, `fs.replace`, `fs.write`, and `shell.exec` for reviewable source changes and verification.
Prefer exact surgical edits through `fs.replace`; use atomic full-file writes through `fs.write` only when that is clearer; use shell commands for real build, test, and git evidence; never invent a result that was not observed.
For clear coding requests, do not stop at a plan; implement the requested change directly through `tsh`, then report changed files and exact verification results.
When available, run the touched project's formatter, static check, lint, and focused tests before claiming success.
Ask for clarification only when the target path or scope is missing, or when the requested action is destructive or ambiguous.
Open-ended project iteration requests are clear coding requests. When the user asks to iterate, bootstrap, self-improve, or make the project better without narrower target, do not ask what to do. Use available `tsh` tools to inspect the applicable project rules, current workspace state, and relevant files; choose one small safe improvement that moves the repository toward the requested goal; edit it; run focused verification; report exact files and commands. If no safe edit is available, run a focused health check and report the blocker with evidence.
Before source edits, inspect the applicable `/workspace` rules and current workspace state; never overwrite, revert, delete, or reformat unrelated user changes.
Never run destructive git commands such as `git reset --hard`, `git checkout --`, or `git clean` unless the user explicitly requests that exact operation.
If verification fails, report the failing command and stderr/stdout instead of claiming success.
Keep local work focused on implementation. Leave architecture decisions and independent review to `architect` and `reviewer`.
Do not add background schedulers, polling loops, hot reload, or new root ABI namespaces.
"
        .to_owned(),
        "worker" => "\
You are CortexFS agent `worker`.
You run on the spark model path and execute bounded delegated implementation tasks.
When the handoff authorizes source work, operate in `/workspace` with `tsh` tools: read before editing, write files atomically, run focused verification, and report exact command evidence.
Before editing, inspect authorized rules and current workspace state; do not overwrite unrelated user changes.
Worker-role agent names include `worker` and `worker-*`; they inherit the spark worker model when no explicit model control file is present.
The shared `worker` entry stays reusable; dedicated `worker-*` temp entries may be reaped after parent-owned terminal results.
Read only the handoff context and authorized refs you are given.
When you receive a schedule handoff line, preserve its `model=`, `life=`, and `role=` context and use its existing `plan=`, `handoff=`, `result=`, and `refs=` paths; claim the child with `ctx schedule claim <plan> <child>` before work and record the terminal outcome with `ctx schedule result <plan> <child> done|error|cancelled ...`.
Return compact results suitable for `context/child/<child>/result.md`, including changed files, tests run, and blockers.
Do not make architecture decisions beyond the handoff scope.
Do not create further child agents unless the handoff explicitly grants and requests that.
"
        .to_owned(),
        "reviewer" => "\
You are CortexFS agent `reviewer`.
You are the independent review agent in the default Architect -> coder/reviewer flow.
Review implementation output for correctness, ABI drift, policy violations, missing tests, and unnecessary complexity.
Return concrete findings first, ordered by severity, with file and command evidence when available.
Do not edit source unless a later explicit policy grants write authority.
"
        .to_owned(),
        "executor" => "\
You are CortexFS agent `executor`.
Run bounded execution and verification tasks on the spark model path.
Shared `worker` and `executor` entries stay reusable; dedicated `worker-*` and `executor-*` temp entries may be reaped after parent-owned terminal results.
Preserve schedule handoff role and paths for `role=`, `plan=`, `handoff=`, `result=`, and `refs=` instead of creating a second coordination surface.
Report commands, outputs, status, and failures without expanding scope.
"
        .to_owned(),
        _ => format!("You are CortexFS agent `{name}`.\n"),
    }
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
            write_reference_text(&root.join("tool").join(tool.name), script)?;
            set_reference_executable(&root.join("tool").join(tool.name))?;
        }
        if tool.name == "tsh" {
            write_reference_text(
                &root.join("tool").join("tsh.d").join("config"),
                DEFAULT_TSH_CONFIG,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod reference_model_tests {
    use super::*;

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
  "default_model": "gpt-5.4-mini",
  "enabled": true,
  "formats": ["openai.chat", "openai.responses"]
}
"#,
        )?;
        fs::write(
            cache_dir.path().join("api.test.models.json"),
            r#"{"models":["gpt-5.4-mini"]}"#,
        )?;

        create_reference_root(root.path())
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        ensure_reference_bin(root.path())
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        ensure_reference_provider_models_from(root.path(), config_dir.path(), cache_dir.path())
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;

        assert!(root.path().join("model/api.test/gpt-5.4-mini").is_file());
        assert!(
            !root
                .path()
                .join("model/api.test/gpt-5.4-mini.d/hooks")
                .exists()
        );
        assert_eq!(
            fs::read_to_string(root.path().join("model/api.test/gpt-5.4-mini.d/id"))?,
            "api.test/gpt-5.4-mini\n"
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
        let models = vec![
            ProjectedProviderModel {
                provider: "api.test".to_owned(),
                model: "gpt-main".to_owned(),
                base_url: "https://api.test/v1".to_owned(),
                driver: "default=openai-chat".to_owned(),
                cap: "chat\nstream".to_owned(),
                effort: "auto".to_owned(),
                fallback: String::new(),
                limit: ModelContextLimit::Unknown,
            },
            ProjectedProviderModel {
                provider: "api.test".to_owned(),
                model: "codex-auto-review".to_owned(),
                base_url: "https://api.test/v1".to_owned(),
                driver: "default=openai-chat".to_owned(),
                cap: "chat\nstream".to_owned(),
                effort: "auto".to_owned(),
                fallback: String::new(),
                limit: ModelContextLimit::Unknown,
            },
            ProjectedProviderModel {
                provider: "api.test".to_owned(),
                model: "turbo-fast".to_owned(),
                base_url: "https://api.test/v1".to_owned(),
                driver: "default=openai-chat".to_owned(),
                cap: "chat\nstream".to_owned(),
                effort: "auto".to_owned(),
                fallback: String::new(),
                limit: ModelContextLimit::Unknown,
            },
            ProjectedProviderModel {
                provider: "api.test".to_owned(),
                model: "deep".to_owned(),
                base_url: "https://api.test/v1".to_owned(),
                driver: "default=openai-chat".to_owned(),
                cap: "chat\nreasoning".to_owned(),
                effort: "auto".to_owned(),
                fallback: String::new(),
                limit: ModelContextLimit::Unknown,
            },
            ProjectedProviderModel {
                provider: "api.test".to_owned(),
                model: "code-pro".to_owned(),
                base_url: "https://api.test/v1".to_owned(),
                driver: "default=openai-chat".to_owned(),
                cap: "chat\nstream".to_owned(),
                effort: "auto".to_owned(),
                fallback: String::new(),
                limit: ModelContextLimit::Unknown,
            },
            ProjectedProviderModel {
                provider: "api.test".to_owned(),
                model: "multimodal".to_owned(),
                base_url: "https://api.test/v1".to_owned(),
                driver: "default=openai-chat".to_owned(),
                cap: "chat\nvision".to_owned(),
                effort: "auto".to_owned(),
                fallback: String::new(),
                limit: ModelContextLimit::Unknown,
            },
        ];
        for model in &models {
            ensure_reference_provider_model(root.path(), model)
                .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        }
        ensure_reference_model_aliases(root.path(), &models)
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;

        assert_eq!(
            fs::read_link(root.path().join("model/main"))?,
            PathBuf::from("/ctx/model/api.test/gpt-main")
        );
        assert_eq!(
            fs::read_link(root.path().join("model/helper"))?,
            PathBuf::from("/ctx/model/api.test/codex-auto-review")
        );
        for (alias, target) in [
            ("fast", "turbo-fast"),
            ("reason", "deep"),
            ("code", "code-pro"),
            ("vision", "multimodal"),
        ] {
            assert_eq!(
                fs::read_link(root.path().join("model").join(alias))?,
                PathBuf::from(format!("/ctx/model/api.test/{target}"))
            );
        }
        Ok(())
    }

    #[test]
    fn runtime_model_aliases_fall_back_to_debug_model_without_provider_models()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        create_reference_root(root.path())
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        ensure_reference_bin(root.path())
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        ensure_reference_debug_model(root.path())
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        ensure_reference_model_aliases(root.path(), &[])
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
