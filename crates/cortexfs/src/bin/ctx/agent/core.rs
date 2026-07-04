#[derive(Debug, Eq, PartialEq)]
enum AgentArgs {
    New(AgentNewArgs),
    Start(AgentStartArgs),
    Stop { name: String },
    Status { name: String },
    Env { name: String },
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

const MAX_AGENT_REPL_STDIN_BYTES: usize = 1024 * 1024;

#[derive(Debug, Eq, PartialEq)]
struct AgentNewArgs {
    name: String,
    temporary: bool,
    parent: Option<String>,
    label: Option<String>,
    models: Vec<String>,
    tools: Vec<String>,
    shared: Vec<AgentShared>,
    mounts: Vec<AgentMount>,
}

#[derive(Debug, Eq, PartialEq)]
struct AgentStartArgs {
    name: String,
    session: String,
    cwd: String,
    default_workspace: bool,
    mounts: Vec<AgentMount>,
}

#[derive(Debug, Eq, PartialEq)]
struct AgentShared {
    name: String,
    access: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AgentMount {
    source: String,
    target: String,
    mode: String,
}

const AGENT_SANDBOX_HOME: &str = "/home/agent";

fn agent_command(root: &Path, args: &AgentArgs) -> Result<ExitCode, CliError> {
    match *args {
        AgentArgs::New(ref args) => agent_new(root, args),
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

fn agent_new(root: &Path, args: &AgentNewArgs) -> Result<ExitCode, CliError> {
    let request = agent_new_request_json(args)?;
    if agent_lifecycle_tool_exists(root, "agent.create")? {
        return agent_lifecycle_tool(root, "agent.create", &request);
    }
    agent_new_host_fallback(root, args)
}

fn agent_stop(root: &Path, name: &str) -> Result<ExitCode, CliError> {
    if agent_lifecycle_tool_exists(root, "agent.stop")? {
        return agent_lifecycle_tool(root, "agent.stop", &agent_name_request_json(name));
    }
    agent_stop_host_fallback(root, name)
}
