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
         allow {policy_subject} tool:tsh execute\n"
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
    wrapper_target: &'static str,
    description: &'static str,
    schema: &'static str,
    cap: &'static str,
    policy: &'static str,
}

const REFERENCE_GLOBAL_TOOLS: &[ReferenceToolSpec] = &[
    ReferenceToolSpec {
        name: "tsh",
        wrapper_target: "/bin/false",
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
        name: "tsh.config",
        wrapper_target: CORTEXFS_OBJECT_RUNNER,
        description: "Read or update persistent tsh runtime configuration.",
        schema: r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "tsh.config input",
  "description": "Read or update tsh.d/config.",
  "type": "object",
  "additionalProperties": true
}"#,
        cap: "tsh.config",
        policy: "allow base_t tool:tsh.config execute\nallow coder_t tool:tsh.config execute\nallow reviewer_t tool:tsh.config execute",
    },
];

const DEFAULT_TSH_CONFIG: &str = "\
max_loaded_tools=64
cache_capacity=32
window_percent=1
";

const DEPRECATED_REFERENCE_PLACEHOLDER_TOOLS: &[&str] = &[
    "mcp.github.search_issues",
    "agent.create",
    "agent.start",
    "agent.stop",
];

fn reference_tool_stub_script(name: &str) -> Option<&'static str> {
    match name {
        "tsh" => Some(reference_exec_named_tool_script("tsh")),
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
        _ => "",
    }
}
