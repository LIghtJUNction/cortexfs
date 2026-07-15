use crate::*;

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum AgentArgs {
    New(AgentNewArgs),
    Apply {
        name: String,
        from: String,
    },
    Start(AgentStartArgs),
    Stop {
        name: String,
    },
    Status {
        name: String,
    },
    Env {
        name: String,
    },
    Ps,
    Send {
        name: String,
        session: Option<String>,
        input: String,
        raw: bool,
        approvals: Vec<String>,
    },
    Chat {
        name: String,
        session: Option<String>,
        raw: bool,
        approvals: Vec<String>,
    },
    Resume {
        name: String,
        session: Option<String>,
        raw: bool,
    },
    History {
        name: String,
        session: Option<String>,
    },
    Output {
        name: String,
        session: Option<String>,
    },
    Pack {
        name: String,
        session: Option<String>,
    },
    Trajectory {
        name: String,
        session: Option<String>,
    },
    SessionGc(AgentSessionGcArgs),
    SessionSelect {
        name: String,
        target: String,
        from: String,
    },
    Prompt {
        name: String,
    },
    Tools {
        name: String,
    },
    Children {
        name: String,
        session: Option<String>,
    },
    Wait {
        name: String,
        session: Option<String>,
        child: String,
    },
    Cancel {
        name: String,
        session: Option<String>,
        run: Option<String>,
        raw: bool,
    },
    Watch {
        name: String,
        session: Option<String>,
    },
    Attach {
        name: String,
        session: Option<String>,
    },
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct AgentNewArgs {
    pub(crate) name: String,
    pub(crate) temporary: bool,
    pub(crate) parent: Option<String>,
    pub(crate) label: Option<String>,
    pub(crate) models: Vec<String>,
    pub(crate) tools: Vec<String>,
    pub(crate) shared: Vec<AgentShared>,
    pub(crate) mounts: Vec<AgentMount>,
    /// Optional persona text materialised into `agent/<name>.d/system.md`.
    pub(crate) instructions: Option<String>,
    /// Optional description materialised into `agent/<name>.d/meta.json`.
    pub(crate) description: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct AgentStartArgs {
    pub(crate) name: String,
    pub(crate) session: String,
    pub(crate) cwd: String,
    pub(crate) default_workspace: bool,
    pub(crate) mounts: Vec<AgentMount>,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct AgentSessionGcArgs {
    pub(crate) name: String,
    pub(crate) dry_run: bool,
    pub(crate) yes: bool,
    pub(crate) delete: bool,
    pub(crate) keep: Vec<String>,
    pub(crate) patterns: Vec<String>,
    pub(crate) older_than_days: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentShared {
    pub(crate) name: String,
    pub(crate) access: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentMount {
    pub(crate) source: String,
    pub(crate) target: String,
    pub(crate) mode: String,
}

pub(crate) const AGENT_SANDBOX_HOME: &str = "/home/agent";

pub(crate) fn agent_command(root: &Path, args: &AgentArgs) -> Result<ExitCode, CliError> {
    match *args {
        AgentArgs::New(ref args) => agent_new(root, args),
        AgentArgs::Apply { ref name, ref from } => {
            require_cli_name("agent name", name)?;
            agent_apply(root, name, Path::new(from))
        }
        AgentArgs::Start(ref args) => agent_start(root, args),
        AgentArgs::Stop { ref name } => {
            require_cli_name("agent name", name)?;
            agent_stop(root, name)
        }
        AgentArgs::Status { ref name } => {
            require_cli_name("agent name", name)?;
            success(agent_status(root, name))
        }
        AgentArgs::Env { ref name } => {
            require_cli_name("agent name", name)?;
            success(agent_env(root, name))
        }
        AgentArgs::Ps => success(agent_ps(root)),
        AgentArgs::Send {
            ref name,
            ref session,
            ref input,
            raw,
            ref approvals,
        } => agent_send(
            root,
            name,
            AgentSend {
                session: session.as_deref(),
                input,
                raw,
                debug: false,
                approvals,
            },
        ),
        AgentArgs::Chat {
            ref name,
            ref session,
            raw,
            ref approvals,
        } => agent_chat(root, name, session.as_deref(), raw, approvals),
        AgentArgs::Resume {
            ref name,
            ref session,
            raw,
        } => agent_resume(root, name, session.as_deref(), raw),
        AgentArgs::History {
            ref name,
            ref session,
        } => success(history(root, name, session.as_deref())),
        AgentArgs::Output {
            ref name,
            ref session,
        } => success(latest(root, name, session.as_deref())),
        AgentArgs::Pack {
            ref name,
            ref session,
        } => success(agent_pack(root, name, session.as_deref())),
        AgentArgs::Trajectory {
            ref name,
            ref session,
        } => success(agent_trajectory(root, name, session.as_deref())),
        AgentArgs::SessionGc(ref args) => success(agent_session_gc(root, args)),
        AgentArgs::SessionSelect {
            ref name,
            ref target,
            ref from,
        } => success(agent_session_select(root, name, target, from)),
        AgentArgs::Prompt { ref name } => success(agent_prompt(root, name)),
        AgentArgs::Tools { ref name } => success(agent_tools(root, name)),
        AgentArgs::Children {
            ref name,
            ref session,
        } => success(agent_children(root, name, session.as_deref())),
        AgentArgs::Wait {
            ref name,
            ref session,
            ref child,
        } => agent_wait(root, name, session.as_deref(), child),
        AgentArgs::Cancel {
            ref name,
            ref session,
            ref run,
            raw,
        } => agent_cancel(root, name, session.as_deref(), run.as_deref(), raw),
        AgentArgs::Watch {
            ref name,
            ref session,
        } => agent_terminal(root, name, session.as_deref(), false),
        AgentArgs::Attach {
            ref name,
            ref session,
        } => agent_terminal(root, name, session.as_deref(), true),
    }
}

pub(crate) fn agent_new(root: &Path, args: &AgentNewArgs) -> Result<ExitCode, CliError> {
    let request = agent_new_request_json(args)?;
    if agent_lifecycle_tool_selected(root, agent_runtime_context_matches(root))? {
        return agent_lifecycle_tool(root, "agent.create", &request);
    }
    agent_new_host_fallback(root, args)
}

pub(crate) fn agent_lifecycle_tool_selected(
    root: &Path,
    runtime_context_matches: bool,
) -> Result<bool, CliError> {
    Ok(runtime_context_matches && agent_lifecycle_tool_exists(root, "agent.create")?)
}

pub(crate) fn agent_runtime_context_matches(root: &Path) -> bool {
    let Ok(agent) = env::var("CTX_AGENT") else {
        return false;
    };
    let Ok(session) = env::var("CTX_SESSION") else {
        return false;
    };
    let Ok(run) = env::var("CTX_RUN_ID") else {
        return false;
    };
    let Some(source) = env::var_os("CTX_SOURCE").map(PathBuf::from) else {
        return false;
    };
    let Some(ctx_root) = env::var_os("CTX_ROOT").map(PathBuf::from) else {
        return false;
    };
    agent_runtime_context_matches_values(root, &source, &ctx_root, &agent, &session, &run)
}

pub(crate) fn agent_runtime_context_matches_values(
    root: &Path,
    source: &Path,
    ctx_root: &Path,
    agent: &str,
    session: &str,
    run: &str,
) -> bool {
    if !is_object_name(agent)
        || !is_object_name(session)
        || !is_object_name(run)
        || ctx_root != root
        || !source.is_absolute()
    {
        return false;
    }
    let Ok(view) = derive_agent_runtime_view(source, agent) else {
        return false;
    };
    let source_control = source.join("agent").join(format!("{agent}.d"));
    let projected_control = root.join("agent").join(format!("{agent}.d"));
    for file in [
        "owner", "uid", "gid", "groups", "label", "iso", "root", "cwd", "env", "path", "mount",
        "model", "policy", "parent", "life",
    ] {
        let source_value = fs::read_to_string(source_control.join(file));
        let projected_value = fs::read_to_string(projected_control.join(file));
        if !matches!((source_value, projected_value), (Ok(left), Ok(right)) if left == right) {
            return false;
        }
    }
    let session_dir = source
        .join("home")
        .join(view.owner().to_string())
        .join("agent")
        .join(agent)
        .join("session")
        .join(session);
    session_dir
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.is_dir())
        && fs::read_to_string(session_dir.join("current_run"))
            .is_ok_and(|current| current.trim() == run)
}

pub(crate) fn agent_stop(root: &Path, name: &str) -> Result<ExitCode, CliError> {
    require_cli_name("agent name", name)?;
    let socket = agent_socket_path(root, name)?;
    let request = format!("{}\n", serde_json::json!({ "op": "stop", "agent": name }));
    stream_socket_request(&socket, &request)
}
