use super::*;

fn reference_tree(name: &str) -> TestDir {
    let root = super::reference_tree(name);
    let models = crate::ensure_v1_runtime_models(&root);
    assert!(models.is_ok(), "{models:?}");
    root
}

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

fn response_run(response: &crate::SocketRuntimeResponse) -> Result<String, String> {
    for frame in response.frames() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(frame) else {
            continue;
        };
        if value.get("type").and_then(serde_json::Value::as_str) == Some("start")
            && let Some(run) = value.get("run").and_then(serde_json::Value::as_str)
        {
            return Ok(run.to_owned());
        }
    }
    Err("runtime response has no start run".to_owned())
}

mod history;
mod messages;
mod policy;
mod sandbox;
