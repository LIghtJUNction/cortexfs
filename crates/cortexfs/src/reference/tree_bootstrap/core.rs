/// Materializes the documented v1 reference tree under `root`.
///
/// This is a filesystem bootstrap helper for tests, local inspection, and
/// simple demos. It creates ABI-visible files, control directories, symlinks,
/// session skeletons, shared queue directories, and Unix socket path entries.
/// It does not start agents, models, MCP servers, providers, or a supervisor.
pub fn ensure_v1_reference_tree(root: &Path) -> Result<ReferenceTreeBootstrap, ReferenceTreeError> {
    create_reference_root(root)?;
    ensure_reference_bin(root)?;
    for agent in REFERENCE_AGENTS {
        ensure_reference_agent(root, agent.name, agent.parent)?;
    }
    remove_deprecated_reference_placeholder_tools(root)?;
    ensure_reference_global_tools(root)?;
    ensure_reference_docs(root)?;
    ensure_reference_home(root)?;
    remove_deprecated_reference_home_tool_aliases(root)?;
    migrate_reference_legacy_session_meta_models(root)?;
    Ok(ReferenceTreeBootstrap::new(root.to_path_buf()))
}

/// Materializes runtime-visible model wrappers for models projected from
/// provider configuration and cache files.
///
/// The FUSE projection can expose provider models virtually, but agent
/// sandboxes bind the backing source tree at `/ctx`. This helper keeps the
/// backing source tree aligned for runtime execution without making the pure
/// reference-tree bootstrap depend on host provider state.
pub fn ensure_v1_runtime_models(root: &Path) -> Result<(), ReferenceTreeError> {
    ensure_reference_models(root)
}

struct ReferenceAgentSpec {
    name: &'static str,
    parent: Option<&'static str>,
    model: &'static str,
}

const REFERENCE_AGENTS: &[ReferenceAgentSpec] = &[
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
        model: HELPER_MODEL_ALIAS,
    },
];
const REFERENCE_OBJECT_RUNNER: &str = "/ctx/bin/cortexfs-object-runner";

fn ensure_reference_models(root: &Path) -> Result<(), ReferenceTreeError> {
    ensure_reference_debug_model(root)?;
    ensure_reference_provider_models_from(
        root,
        Path::new(SYSTEM_PROVIDER_CONFIG_DIR),
        Path::new(SYSTEM_PROVIDER_MODEL_CACHE_DIR),
    )
}

fn ensure_reference_debug_model(root: &Path) -> Result<(), ReferenceTreeError> {
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

fn ensure_reference_provider_models_from(
    root: &Path,
    config_dir: &Path,
    cache_dir: &Path,
) -> Result<(), ReferenceTreeError> {
    let models =
        projected_provider_models(config_dir, cache_dir).map_err(|_error| ReferenceTreeError::CannotCreate)?;
    for model in models {
        ensure_reference_provider_model(root, &model)?;
    }
    Ok(())
}

fn ensure_reference_provider_model(
    root: &Path,
    model: &ProjectedProviderModel,
) -> Result<(), ReferenceTreeError> {
    let name = format!("{}/{}", model.provider, model.model);
    let controls = MODEL_CONTROL_FILES
        .iter()
        .filter_map(|file| provider_model_control_content(model, file).map(|content| (*file, content)))
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

fn ensure_reference_docs(root: &Path) -> Result<(), ReferenceTreeError> {
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

fn create_reference_root(root: &Path) -> Result<(), ReferenceTreeError> {
    for entry in ROOT_ENTRIES {
        match *entry {
            "status" => write_reference_text(&root.join("status"), "ready\n")?,
            directory => create_reference_dir(&root.join(directory))?,
        }
    }
    Ok(())
}

fn ensure_reference_bin(root: &Path) -> Result<(), ReferenceTreeError> {
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
    remove_deprecated_reference_bin_te(root)?;
    Ok(())
}

fn remove_deprecated_reference_bin_te(root: &Path) -> Result<(), ReferenceTreeError> {
    let path = root.join("bin").join("te");
    let Ok(content) = read_reference_tree_small_text(&path) else {
        return Ok(());
    };
    if content.contains("# CortexFS reference-tree te placeholder.") {
        remove_reference_entry(&path).map_err(|_error| ReferenceTreeError::CannotRemove)?;
    }
    Ok(())
}

fn ensure_reference_agent(
    root: &Path,
    name: &str,
    parent: Option<&str>,
) -> Result<(), ReferenceTreeError> {
    install_executable_object_wrapper(root, ObjectClass::Agent, name, "/bin/false", &[])
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
        ("groups", "1000\n".to_owned()),
        ("label", label),
        ("iso", "shared\n".to_owned()),
        ("parent", parent.map_or_else(|| "\n".to_owned(), |value| format!("{value}\n"))),
        ("life", "owned\n".to_owned()),
        ("root", home_root),
        ("cwd", "/workspace\n".to_owned()),
        ("env", "CTX_ROOT=/ctx\n".to_owned()),
        ("path", "/ctx/tool:/ctx/home/1000/tool\n".to_owned()),
        ("mount", mount),
        ("model", format!("{}\n", reference_agent_model(name))),
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
    ensure_reference_socket(&root.join("agent").join(format!("{name}.sock")))
}

fn reference_agent_wrapper_script(name: &str) -> String {
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

fn reference_agent_policy(policy_subject: &str, name: &str) -> String {
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
                 allow {policy_subject} tool:shell.exec execute\n"
            ),
        );
    } else if name == "executor" {
        let _ignored = std::fmt::Write::write_fmt(
            &mut policy,
            format_args!("allow {policy_subject} tool:shell.exec execute\n"),
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

fn reference_agent_children(name: &str) -> Vec<&'static str> {
    if name == "architect" {
        return REFERENCE_AGENTS
            .iter()
            .filter(|agent| agent.parent == Some("agent:architect"))
            .map(|agent| agent.name)
            .collect();
    }
    Vec::new()
}

fn reference_agent_can_write_source(name: &str) -> bool {
    name == "coder"
}

fn reference_agent_model(name: &str) -> &'static str {
    REFERENCE_AGENTS
        .iter()
        .find(|agent| agent.name == name)
        .map_or(DEFAULT_MODEL_ALIAS, |agent| agent.model)
}

fn reference_agent_system_prompt(name: &str) -> String {
    match name {
        "architect" => "\
You are CortexFS agent `architect`.
Your human role name is Architect.
Act as the parent planner and architecture coordinator for the default agent tree.
Keep task decomposition explicit in session files; delegate implementation to `coder` and verification to `reviewer`.
Minimize coordination cost: merge small work, split only when implementation and review responsibilities are genuinely distinct.
Preserve the CortexFS v1 ABI shape; do not add root namespaces, background schedulers, polling loops, watchers, or hot reload paths.
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
Before source edits, inspect `/workspace` rules with `find /workspace -name AGENTS.md -print`, read the nearest applicable `AGENTS.md` files, and check `git status --short`; never overwrite, revert, delete, or reformat unrelated user changes.
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
Before editing, inspect authorized rules and `git status --short`; do not overwrite unrelated user changes.
Worker-role agent names include `worker`, `worker-*`, `executor`, and `executor-*`; they inherit the spark worker model when no explicit model control file is present.
Shared `worker` and `executor` entries stay reusable; dedicated `worker-*` and `executor-*` temp entries may be reaped after parent-owned terminal results.
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

#[expect(
    dead_code,
    reason = "legacy reference stub retained while object-runner wrapper rollout settles"
)]
fn reference_agent_stub_script(name: &str) -> String {
    format!(
        r#"#!/bin/sh
# CortexFS reference-tree agent stub. The selected model is a file ABI choice.
source_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
ctx_root="${{CTX_ROOT:-/ctx}}"
run="${{CTX_RUN_ID:-r1}}"
input="$*"
if [ -z "$input" ]; then
  input="$(/usr/bin/cat)"
fi
model="$(/usr/bin/tr -d '\n' < "$source_root/agent/{name}.d/model" 2>/dev/null || true)"
if [ -z "$model" ]; then
  model="main"
fi
case "$model" in
  */*/*|/*|../*|*/../*|*/..|*//*) model="" ;;
  */*) ;;
  main|helper)
    target="$(/usr/bin/readlink "$ctx_root/model/$model" 2>/dev/null || true)"
    case "$target" in
      /ctx/model/*/*) model="${{target#/ctx/model/}}" ;;
      *) model="" ;;
    esac
    ;;
  *) model="" ;;
esac
case "$model" in
  */*/*|/*|../*|*/../*|*/..|*//*) model="" ;;
  ?*/*?) ;;
  *) model="" ;;
esac
if [ -z "$model" ] || [ ! -x "$ctx_root/model/$model" ]; then
  printf '{{"type":"error","run":"%s","code":"ENOENT","message":"missing model"}}\n' "$run"
  printf '{{"type":"done","run":"%s","status":"error"}}\n' "$run"
  exit 1
fi
history="${{CTX_AGENT_HISTORY_MESSAGES:-}}"
if [ -z "$history" ] || [ "$history" = "(no historical messages injected)" ]; then
  session="${{CTX_SESSION:-default}}"
  case "$session" in
    */*|.*|*..*) session="" ;;
  esac
  history_file="$source_root/home/1000/agent/{name}/session/$session/messages.jsonl"
  if [ -n "$session" ] && [ -r "$history_file" ]; then
    history="$(/usr/bin/tail -n 40 "$history_file" 2>/dev/null || true)"
  fi
fi
if [ -n "$history" ] && [ "$history" != "(no historical messages injected)" ]; then
  input="$(/usr/bin/printf '%s\n%s\n\n%s\n%s\n' "Conversation history:" "$history" "Current user input:" "$input")"
fi
CTX_AGENT="{name}" \
CTX_AGENT_SYSTEM="$(/usr/bin/cat "$source_root/agent/{name}.d/system.md" 2>/dev/null || true)" \
CTX_AGENT_PROMPT_TEMPLATE="$(/usr/bin/cat "$source_root/agent/{name}.d/prompt.template.md" 2>/dev/null || true)" \
CTX_RUN_ID="$run" \
exec "$ctx_root/model/$model" "$input"
"#
    )
}

fn ensure_reference_global_tools(root: &Path) -> Result<(), ReferenceTreeError> {
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

fn remove_deprecated_reference_placeholder_tools(root: &Path) -> Result<(), ReferenceTreeError> {
    for tool in DEPRECATED_REFERENCE_PLACEHOLDER_TOOLS {
        remove_deprecated_reference_placeholder_tool(root, tool)?;
    }
    Ok(())
}

fn remove_deprecated_reference_placeholder_tool(
    root: &Path,
    name: &str,
) -> Result<(), ReferenceTreeError> {
    let executable = root.join("tool").join(name);
    let control_dir = root.join("tool").join(format!("{name}.d"));
    if fs::symlink_metadata(&executable).is_err() && fs::symlink_metadata(&control_dir).is_err() {
        return Ok(());
    }
    if !is_deprecated_reference_placeholder_tool(&executable, &control_dir) {
        return Ok(());
    }
    remove_reference_entry(&executable).map_err(|_error| ReferenceTreeError::CannotRemove)?;
    remove_deprecated_reference_placeholder_control_dir(&control_dir)?;
    Ok(())
}

fn remove_deprecated_reference_placeholder_control_dir(
    control_dir: &Path,
) -> Result<(), ReferenceTreeError> {
    let control_dir_file =
        open_reference_dir(control_dir).map_err(|_error| ReferenceTreeError::CannotRemove)?;
    for file in TOOL_CONTROL_FILES {
        nix::unistd::unlinkat(
            &control_dir_file,
            *file,
            nix::unistd::UnlinkatFlags::NoRemoveDir,
        )
        .map_err(|_error| ReferenceTreeError::CannotRemove)?;
    }
    remove_deprecated_reference_placeholder_hook_dir(&control_dir_file)?;
    let Some(parent) = control_dir.parent() else {
        return Err(ReferenceTreeError::CannotRemove);
    };
    let parent_dir = open_reference_dir(parent).map_err(|_error| ReferenceTreeError::CannotRemove)?;
    let name = control_dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(ReferenceTreeError::CannotRemove)?;
    nix::unistd::unlinkat(&parent_dir, name, nix::unistd::UnlinkatFlags::RemoveDir)
        .map_err(|_error| ReferenceTreeError::CannotRemove)
}

fn remove_deprecated_reference_placeholder_hook_dir(
    control_dir_file: &fs::File,
) -> Result<(), ReferenceTreeError> {
    let Ok(hook_dir_fd) = nix::fcntl::openat(
        control_dir_file,
        OBJECT_HOOK_DIR,
        nix::fcntl::OFlag::O_DIRECTORY
            | nix::fcntl::OFlag::O_RDONLY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    ) else {
        return Ok(());
    };
    let hook_dir = fs::File::from(hook_dir_fd);
    for phase in OBJECT_HOOK_PHASE_DIRS {
        nix::unistd::unlinkat(&hook_dir, *phase, nix::unistd::UnlinkatFlags::RemoveDir)
            .map_err(|_error| ReferenceTreeError::CannotRemove)?;
    }
    nix::unistd::unlinkat(
        control_dir_file,
        OBJECT_HOOK_DIR,
        nix::unistd::UnlinkatFlags::RemoveDir,
    )
    .map_err(|_error| ReferenceTreeError::CannotRemove)
}

fn is_deprecated_reference_placeholder_tool(executable: &Path, control_dir: &Path) -> bool {
    if !control_dir
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.is_dir())
    {
        return false;
    }
    let Ok(wrapper) = read_reference_tree_small_text(executable) else {
        return false;
    };
    let Ok(description) = read_reference_tree_small_text(&control_dir.join("description")) else {
        return false;
    };
    ((wrapper.contains("# CortexFS generated object wrapper.\n")
        && wrapper.contains("exec '/bin/false' \"$0\" \"$@\"\n"))
        || wrapper == "#!/bin/sh\n# CortexFS generated object wrapper.\nexec '/bin/false' \"$0\" \"$@\"\n")
        && description.trim_end_matches('\n') == "CortexFS reference-tree tool"
        && deprecated_placeholder_tool_control_dir_is_exact(control_dir)
}

fn deprecated_placeholder_tool_control_dir_is_exact(control_dir: &Path) -> bool {
    let Ok(control_dir_file) = open_reference_dir(control_dir) else {
        return false;
    };
    let Ok(entries) = fs::read_dir(reference_tree_proc_fd_path(&control_dir_file)) else {
        return false;
    };
    let mut seen = Vec::new();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            return false;
        };
        if !TOOL_CONTROL_FILES.contains(&file_name) {
            if file_name == OBJECT_HOOK_DIR && deprecated_placeholder_hook_dir_is_exact(control_dir)
            {
                continue;
            }
            return false;
        }
        let Ok(stat) = nix::sys::stat::fstatat(
            &control_dir_file,
            file_name,
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        ) else {
            return false;
        };
        if stat.st_mode & libc::S_IFMT != libc::S_IFREG {
            return false;
        }
        seen.push(file_name.to_owned());
    }
    TOOL_CONTROL_FILES
        .iter()
        .all(|required| seen.iter().any(|file| file == required))
}

fn deprecated_placeholder_hook_dir_is_exact(control_dir: &Path) -> bool {
    let hook_dir = control_dir.join(OBJECT_HOOK_DIR);
    if !hook_dir
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
    {
        return false;
    }
    let Ok(hook_dir_file) = open_reference_dir(&hook_dir) else {
        return false;
    };
    let Ok(entries) = fs::read_dir(reference_tree_proc_fd_path(&hook_dir_file)) else {
        return false;
    };
    let mut seen = Vec::new();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            return false;
        };
        if !OBJECT_HOOK_PHASE_DIRS.contains(&file_name) {
            return false;
        }
        let Ok(stat) = nix::sys::stat::fstatat(
            &hook_dir_file,
            file_name,
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        ) else {
            return false;
        };
        if stat.st_mode & libc::S_IFMT != libc::S_IFDIR {
            return false;
        }
        seen.push(file_name.to_owned());
    }
    OBJECT_HOOK_PHASE_DIRS
        .iter()
        .all(|required| seen.iter().any(|file| file == required))
}

fn reference_tree_proc_fd_path(directory: &fs::File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()))
}

fn read_reference_tree_small_text(path: &Path) -> std::io::Result<String> {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_REFERENCE_SESSION_META_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "reference tree file is not a bounded regular file",
        ));
    }
    let len = usize::try_from(metadata.len()).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("file is too large to read: {error}"),
        )
    })?;
    let mut content = vec![0; len];
    file.read_exact(&mut content)?;
    String::from_utf8(content)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.utf8_error()))
}

#[cfg(test)]
mod reference_model_tests {
    use super::*;

    #[test]
    fn provider_models_are_materialized_into_reference_tree(
    ) -> Result<(), Box<dyn std::error::Error>> {
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
            root.path()
                .join("model/api.test/gpt-5.4-mini.d/hooks/pre.d")
                .is_dir()
        );
        assert_eq!(
            fs::read_to_string(root.path().join("model/api.test/gpt-5.4-mini.d/id"))?,
            "api.test/gpt-5.4-mini\n"
        );
        Ok(())
    }
}
