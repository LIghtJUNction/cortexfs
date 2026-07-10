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
    },
    Repl {
        name: String,
        session: Option<String>,
        raw: bool,
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

pub(crate) const MAX_AGENT_REPL_STDIN_BYTES: usize = 1024 * 1024;

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
        } => agent_send(root, name, session.as_deref(), input, raw, false),
        AgentArgs::Repl {
            ref name,
            ref session,
            raw,
        } => agent_repl(root, name, session.as_deref(), raw),
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
    if agent_lifecycle_tool_exists(root, "agent.create")? {
        return agent_lifecycle_tool(root, "agent.create", &request);
    }
    agent_new_host_fallback(root, args)
}

pub(crate) fn agent_stop(root: &Path, name: &str) -> Result<ExitCode, CliError> {
    if agent_lifecycle_tool_exists(root, "agent.stop")? {
        return agent_lifecycle_tool(root, "agent.stop", &agent_name_request_json(name));
    }
    agent_stop_host_fallback(root, name)
}
