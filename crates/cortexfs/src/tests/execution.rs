use super::*;

fn direct_agent_runtime<'a>(
    root: &'a Path,
    view: &'a crate::AgentRuntimeView,
    session_root: &'a Path,
    executable: &'a Path,
) -> AgentExecutableSocketRuntime<'a> {
    AgentExecutableSocketRuntime {
        ctx_root: root,
        source_root: root,
        identity: view.identity(),
        env: view.env(),
        session_root,
        default_cwd: "/work",
        model: Some("debug/echo"),
        network_allowed: false,
        agent_name: "coder",
        agent_executable: executable,
        execution: AgentExecutableSocketExecution::Direct,
    }
}

mod history;
mod messages;
mod policy;
mod sandbox;
