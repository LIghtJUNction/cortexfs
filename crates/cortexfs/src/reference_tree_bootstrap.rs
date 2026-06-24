/// Materializes the documented v1 reference tree under `root`.
///
/// This is a filesystem bootstrap helper for tests, local inspection, and
/// simple demos. It creates ABI-visible files, control directories, symlinks,
/// session skeletons, shared queue directories, and Unix socket path entries.
/// It does not start agents, models, MCP servers, providers, or a supervisor.
pub fn ensure_v1_reference_tree(root: &Path) -> Result<ReferenceTreeBootstrap, ReferenceTreeError> {
    create_reference_root(root)?;
    ensure_reference_bin(root)?;
    ensure_reference_agent(root, "base", None)?;
    ensure_reference_agent(root, "coder", Some("agent:base"))?;
    ensure_reference_agent(root, "reviewer", Some("agent:base"))?;
    remove_deprecated_reference_placeholder_tools(root)?;
    ensure_reference_global_tools(root)?;
    ensure_reference_home(root)?;
    remove_deprecated_reference_home_tool_aliases(root)?;
    migrate_reference_legacy_session_meta_models(root)?;
    Ok(ReferenceTreeBootstrap::new(root.to_path_buf()))
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
        "#!/bin/sh\n# CortexFS reference-tree ctx placeholder.\nexec ctx \"$@\"\n",
    )?;
    set_reference_executable(&ctx)?;
    for name in ["ctxterm", "tsh"] {
        let path = root.join("bin").join(name);
        write_reference_text(
            &path,
            &format!("#!/bin/sh\n# CortexFS reference-tree {name} placeholder.\nexec {name} \"$@\"\n"),
        )?;
        set_reference_executable(&path)?;
    }
    remove_deprecated_reference_bin_te(root)?;
    Ok(())
}

fn remove_deprecated_reference_bin_te(root: &Path) -> Result<(), ReferenceTreeError> {
    let path = root.join("bin").join("te");
    let Ok(content) = fs::read_to_string(&path) else {
        return Ok(());
    };
    if content.contains("# CortexFS reference-tree te placeholder.") {
        fs::remove_file(path).map_err(|_error| ReferenceTreeError::CannotRemove)?;
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
        ("cwd", "/work\n".to_owned()),
        ("env", "CTX_ROOT=/ctx\n".to_owned()),
        ("path", "/ctx/tool:/ctx/home/1000/tool\n".to_owned()),
        ("mount", mount),
        ("model", format!("{DEFAULT_MODEL_ALIAS}\n")),
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
    let mut policy = format!(
        "allow {policy_subject} model:{DEFAULT_MODEL_ALIAS} use\n\
         allow {policy_subject} tool:tsh execute\n\
         allow {policy_subject} tool:fs.read execute\n"
    );
    if name == "base" {
        for child in ["coder", "reviewer"] {
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
    }
    policy
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
  input="$(cat)"
fi
model="$(tr -d '\n' < "$source_root/agent/{name}.d/model" 2>/dev/null || true)"
if [ -z "$model" ]; then
  model="main"
fi
if [ ! -x "$ctx_root/model/$model" ]; then
  printf '{{"type":"error","run":"%s","code":"ENOENT","message":"missing model"}}\n' "$run"
  printf '{{"type":"done","run":"%s","status":"error"}}\n' "$run"
  exit 1
fi
CTX_RUN_ID="$run" exec "$ctx_root/model/$model" "$input"
"#
    )
}

fn ensure_reference_global_tools(root: &Path) -> Result<(), ReferenceTreeError> {
    for tool in REFERENCE_GLOBAL_TOOLS {
        install_executable_object_wrapper(
            root,
            ObjectClass::Tool,
            tool.name,
            "/bin/false",
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
    if !executable.exists() && !control_dir.exists() {
        return Ok(());
    }
    if !is_deprecated_reference_placeholder_tool(&executable, &control_dir) {
        return Ok(());
    }
    match fs::remove_file(&executable) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_error) => return Err(ReferenceTreeError::CannotRemove),
    }
    match fs::remove_dir_all(&control_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_error) => return Err(ReferenceTreeError::CannotRemove),
    }
    Ok(())
}

fn is_deprecated_reference_placeholder_tool(executable: &Path, control_dir: &Path) -> bool {
    let Ok(wrapper) = fs::read_to_string(executable) else {
        return false;
    };
    let Ok(description) = fs::read_to_string(control_dir.join("description")) else {
        return false;
    };
    wrapper == executable_wrapper_script("/bin/false")
        && description.trim_end_matches('\n') == "CortexFS reference-tree tool"
}

struct ReferenceToolSpec {
    name: &'static str,
    description: &'static str,
    schema: &'static str,
    cap: &'static str,
    policy: &'static str,
}

const REFERENCE_GLOBAL_TOOLS: &[ReferenceToolSpec] = &[
    ReferenceToolSpec {
        name: "tsh",
        description: "CortexFS tool shell. Resolve and run tools through CTX_PATH.",
        schema: r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "tsh input",
  "description": "Run a CortexFS tool by name through CTX_PATH.",
  "type": "object",
  "additionalProperties": true
}"#,
        cap: "tsh",
        policy: "allow base_t tool:tsh execute\nallow coder_t tool:tsh execute\nallow reviewer_t tool:tsh execute",
    },
    ReferenceToolSpec {
        name: "bash",
        description: "Interactive bash tool for agents running inside ctxterm/tsh.",
        schema: r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "bash tool input",
  "description": "Launch bash as a CortexFS tool.",
  "type": "object",
  "additionalProperties": true
}"#,
        cap: "bash",
        policy: "",
    },
    ReferenceToolSpec {
        name: "tmux",
        description: "Interactive tmux tool for background terminal tasks.",
        schema: r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "tmux tool input",
  "description": "Launch tmux as a CortexFS tool.",
  "type": "object",
  "additionalProperties": true
}"#,
        cap: "tmux",
        policy: "",
    },
    ReferenceToolSpec {
        name: "zellij",
        description: "Interactive zellij tool for background terminal tasks.",
        schema: r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "zellij tool input",
  "description": "Launch zellij as a CortexFS tool.",
  "type": "object",
  "additionalProperties": true
}"#,
        cap: "zellij",
        policy: "",
    },
    ReferenceToolSpec {
        name: "fs.read",
        description: "Read a UTF-8 text file from the agent-visible filesystem.",
        schema: r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "fs.read input",
  "description": "Read one UTF-8 text file visible to the tool process.",
  "type": "object",
  "additionalProperties": false,
  "required": ["path"],
  "properties": {
    "path": {
      "type": "string",
      "description": "Path to a UTF-8 text file visible to the tool process."
    }
  }
}"#,
        cap: "fs.read",
        policy: "allow base_t tool:fs.read execute\nallow coder_t tool:fs.read execute\nallow reviewer_t tool:fs.read execute",
    },
    ReferenceToolSpec {
        name: "fs.write",
        description: "Write UTF-8 text to a file path visible to the tool process.",
        schema: r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "fs.write input",
  "description": "Write UTF-8 text to one path visible to the tool process.",
  "type": "object",
  "additionalProperties": false,
  "required": ["path", "content"],
  "properties": {
    "path": {
      "type": "string",
      "description": "Path to write."
    },
    "content": {
      "type": "string",
      "description": "UTF-8 content to write."
    }
  }
}"#,
        cap: "fs.write",
        policy: "",
    },
    ReferenceToolSpec {
        name: "shell.exec",
        description: "Run a shell command in the tool process environment and return stdout/stderr.",
        schema: r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "shell.exec input",
  "description": "Run one shell command in the tool process environment.",
  "type": "object",
  "additionalProperties": false,
  "required": ["cmd"],
  "properties": {
    "cmd": {
      "type": "string",
      "description": "Command line passed to sh -c."
    }
  }
}"#,
        cap: "shell.exec",
        policy: "",
    },
];

const DEPRECATED_REFERENCE_PLACEHOLDER_TOOLS: &[&str] = &[
    "mcp.github.search_issues",
    "agent.create",
    "agent.start",
    "agent.stop",
];

fn reference_tool_stub_script(name: &str) -> Option<&'static str> {
    match name {
        "tsh" => Some(reference_exec_named_tool_script("tsh")),
        "bash" => Some(reference_exec_named_tool_script("bash")),
        "tmux" => Some(reference_exec_named_tool_script("tmux")),
        "zellij" => Some(reference_exec_named_tool_script("zellij")),
        "fs.read" => Some(reference_fs_read_stub_script()),
        "fs.write" => Some(reference_fs_write_stub_script()),
        "shell.exec" => Some(reference_shell_exec_stub_script()),
        _ => None,
    }
}

fn reference_exec_named_tool_script(name: &'static str) -> &'static str {
    match name {
        "tsh" => {
            r#"#!/bin/sh
# CortexFS reference-tree tsh tool.
exec tsh "$@"
"#
        }
        "bash" => {
            r#"#!/bin/sh
# CortexFS reference-tree bash tool.
exec bash "$@"
"#
        }
        "tmux" => {
            r#"#!/bin/sh
# CortexFS reference-tree tmux tool.
exec tmux "$@"
"#
        }
        "zellij" => {
            r#"#!/bin/sh
# CortexFS reference-tree zellij tool.
exec zellij "$@"
"#
        }
        _ => "",
    }
}

fn reference_fs_read_stub_script() -> &'static str {
    r#"#!/bin/sh
# CortexFS reference-tree fs.read stub.
run="$CTX_RUN_ID"
if [ -z "$run" ]; then
  run="r1"
fi
input="$*"
if [ -z "$input" ]; then
  input="$(cat)"
fi
path="$(printf '%s' "$input" | sed -n 's/.*"path"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
if [ -z "$path" ]; then
  path="$input"
fi
printf '{"type":"start","run":"%s","tool":"fs.read"}\n' "$run"
if [ ! -f "$path" ]; then
  printf '{"type":"error","run":"%s","code":"ENOENT","message":"file not found"}\n' "$run"
  printf '{"type":"done","run":"%s","status":"error"}\n' "$run"
  exit 2
fi
content="$(cat "$path")"
json_text="$(printf '%s' "$content" | sed 's/\\/\\\\/g; s/"/\\"/g')"
printf '{"type":"message","run":"%s","role":"tool","content":[{"type":"text","text":"%s"}]}\n' "$run" "$json_text"
printf '{"type":"done","run":"%s","status":"ok"}\n' "$run"
"#
}

fn reference_fs_write_stub_script() -> &'static str {
    r#"#!/bin/sh
# CortexFS reference-tree fs.write stub.
run="$CTX_RUN_ID"
if [ -z "$run" ]; then
  run="r1"
fi
input="$*"
if [ -z "$input" ]; then
  input="$(cat)"
fi
path="$(printf '%s' "$input" | sed -n 's/.*"path"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
content="$(printf '%s' "$input" | sed -n 's/.*"content"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
printf '{"type":"start","run":"%s","tool":"fs.write"}\n' "$run"
if [ -z "$path" ]; then
  printf '{"type":"error","run":"%s","code":"EINVAL","message":"missing path"}\n' "$run"
  printf '{"type":"done","run":"%s","status":"error"}\n' "$run"
  exit 2
fi
if ! printf '%s' "$content" > "$path"; then
  printf '{"type":"error","run":"%s","code":"EACCES","message":"write failed"}\n' "$run"
  printf '{"type":"done","run":"%s","status":"error"}\n' "$run"
  exit 13
fi
printf '{"type":"message","run":"%s","role":"tool","content":[{"type":"text","text":"written"}]}\n' "$run"
printf '{"type":"done","run":"%s","status":"ok"}\n' "$run"
"#
}

fn reference_shell_exec_stub_script() -> &'static str {
    r#"#!/bin/sh
# CortexFS reference-tree shell.exec stub.
run="$CTX_RUN_ID"
if [ -z "$run" ]; then
  run="r1"
fi
input="$*"
if [ -z "$input" ]; then
  input="$(cat)"
fi
cmd="$(printf '%s' "$input" | sed -n 's/.*"cmd"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
if [ -z "$cmd" ]; then
  cmd="$input"
fi
printf '{"type":"start","run":"%s","tool":"shell.exec"}\n' "$run"
output="$(sh -c "$cmd" 2>&1)"
status="$?"
json_text="$(printf '%s' "$output" | sed 's/\\/\\\\/g; s/"/\\"/g')"
printf '{"type":"message","run":"%s","role":"tool","content":[{"type":"text","text":"%s"}]}\n' "$run" "$json_text"
if [ "$status" -eq 0 ]; then
  printf '{"type":"done","run":"%s","status":"ok"}\n' "$run"
else
  printf '{"type":"error","run":"%s","code":"EIO","message":"command failed"}\n' "$run"
  printf '{"type":"done","run":"%s","status":"error"}\n' "$run"
  exit 1
fi
"#
}
