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

struct ReferenceAgentSpec {
    name: &'static str,
    parent: Option<&'static str>,
    model: &'static str,
}

const REFERENCE_AGENTS: &[ReferenceAgentSpec] = &[
    ReferenceAgentSpec {
        name: "base",
        parent: None,
        model: DEFAULT_MODEL_ALIAS,
    },
    ReferenceAgentSpec {
        name: "coder",
        parent: Some("agent:base"),
        model: DEFAULT_MODEL_ALIAS,
    },
    ReferenceAgentSpec {
        name: "reviewer",
        parent: Some("agent:base"),
        model: HELPER_MODEL_ALIAS,
    },
    ReferenceAgentSpec {
        name: "executor",
        parent: Some("agent:base"),
        model: DEFAULT_WORKER_MODEL,
    },
    ReferenceAgentSpec {
        name: "worker",
        parent: Some("agent:coder"),
        model: DEFAULT_WORKER_MODEL,
    },
];

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
    for name in ["ctxterm", "tsh"] {
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
        &reference_agent_stub_script(name),
    )?;
    set_reference_executable(&root.join("agent").join(name))?;
    ensure_reference_socket(&root.join("agent").join(format!("{name}.sock")))
}

fn reference_agent_policy(policy_subject: &str, name: &str) -> String {
    let model = reference_agent_model(name);
    let mut policy = format!(
        "allow {policy_subject} model:{model} use\n\
         allow {policy_subject} tool:tsh execute\n\
         allow {policy_subject} tool:fs.read execute\n\
         allow {policy_subject} network:default connect\n"
    );
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
    if name == "base" {
        return REFERENCE_AGENTS
            .iter()
            .filter(|agent| agent.parent == Some("agent:base"))
            .map(|agent| agent.name)
            .collect();
    }
    if name == "coder" {
        return vec!["worker"];
    }
    Vec::new()
}

fn reference_agent_model(name: &str) -> &'static str {
    REFERENCE_AGENTS
        .iter()
        .find(|agent| agent.name == name)
        .map_or(DEFAULT_MODEL_ALIAS, |agent| agent.model)
}

fn reference_agent_system_prompt(name: &str) -> String {
    match name {
        "base" => "\
You are CortexFS agent `base`.
Act as the parent coordinator for the reference agent tree.
Keep orchestration explicit in session files and use child agents for independent work.
Do not create background schedulers, watchers, or hidden queues.
"
        .to_owned(),
        "coder" => "\
You are CortexFS agent `coder`.
You are the parent coding agent, not the default implementation worker.
For independent implementation work, prefer a delegated `react` node in your session `context/plan.json` with a `child` channel and no `agent` field; the omitted delegated agent is `worker`.
Agents named `worker`, `worker-*`, `executor`, or `executor-*` are worker-role agents and use the spark worker model when they do not declare an explicit model.
Treat `worker` and `executor` as shared reusable entries; use `worker-*` or `executor-*` names for dedicated temp workers that may be reaped after terminal results.
Omit the child `session` field to inherit your current parent session; set it only when a deliberately separate child session is required.
Use `ctx schedule status` to inspect node state, `ctx schedule advance` to materialize worker handoffs, `ctx schedule claim` when a worker accepts a handoff, `ctx schedule result` to record terminal child results, and `ctx agent wait` to inspect completed child results.
Pass the schedule output fields `model=`, `life=`, `plan=`, `handoff=`, `result=`, and `refs=` to the worker instead of inventing a second coordination surface.
Keep local work for planning, integration, review of worker output, and small edits that do not justify a child.
Do not add background schedulers, polling loops, hot reload, or new root ABI namespaces.
"
        .to_owned(),
        "worker" => "\
You are CortexFS agent `worker`.
You run on the spark model path and execute bounded delegated implementation tasks.
Worker-role agent names include `worker`, `worker-*`, `executor`, and `executor-*`; they inherit the spark worker model when no explicit model control file is present.
Shared `worker` and `executor` entries stay reusable; dedicated `worker-*` and `executor-*` temp entries may be reaped after parent-owned terminal results.
Read only the handoff context and authorized refs you are given.
When you receive a schedule handoff line, preserve its `model=` and `life=` context and use its existing `plan=`, `handoff=`, `result=`, and `refs=` paths; claim the child with `ctx schedule claim <plan> <child>` before work and record the terminal outcome with `ctx schedule result <plan> <child> done|error|cancelled ...`.
Return compact results suitable for `context/child/<child>/result.md`, including changed files, tests run, and blockers.
Do not make architecture decisions beyond the handoff scope.
Do not create further child agents unless the handoff explicitly grants and requests that.
"
        .to_owned(),
        "reviewer" => "\
You are CortexFS agent `reviewer`.
Review parent or worker output for correctness, ABI drift, policy violations, and missing tests.
Return concrete findings first and keep summaries secondary.
"
        .to_owned(),
        "executor" => "\
You are CortexFS agent `executor`.
Run bounded execution and verification tasks on the spark model path.
Report commands, outputs, status, and failures without expanding scope.
"
        .to_owned(),
        _ => format!("You are CortexFS agent `{name}`.\n"),
    }
}

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
    wrapper == executable_wrapper_script("/bin/false")
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
