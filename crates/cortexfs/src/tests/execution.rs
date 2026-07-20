use super::*;
use serde_json::Value;

fn reference_tree(name: &str) -> TestDir {
    let root = super::reference_tree(name);
    let models = crate::ensure_runtime_models(&root);
    assert!(models.is_ok(), "{models:?}");
    root
}

/// Builds an in-process agent executable socket runtime for direct execution tests.
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

fn json_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key)?.as_str()
}

fn parse_jsonl(text: &str) -> serde_json::Result<Vec<Value>> {
    text.lines().map(serde_json::from_str).collect()
}

fn done_statuses(frames: &[Value], run: &str) -> Result<Vec<String>, &'static str> {
    frames
        .iter()
        .filter(|value| {
            json_str(value, "type") == Some("done") && json_str(value, "run") == Some(run)
        })
        .map(|value| {
            json_str(value, "status")
                .map(str::to_owned)
                .ok_or("done missing status")
        })
        .collect()
}

fn set_stream_timeouts(stream: &UnixStream, seconds: u64) {
    let timeout = Some(Duration::from_secs(seconds));
    assert!(stream.set_read_timeout(timeout).is_ok());
    assert!(stream.set_write_timeout(timeout).is_ok());
}

fn response_run(response: &crate::SocketRuntimeResponse) -> Result<String, String> {
    for frame in response.frames() {
        let Ok(value) = serde_json::from_str::<Value>(frame) else {
            continue;
        };
        if json_str(&value, "type") == Some("start")
            && let Some(run) = json_str(&value, "run")
        {
            return Ok(run.to_owned());
        }
    }
    Err("runtime response has no start run".to_owned())
}

fn agent_envelope(run: &str) -> String {
    serde_json::json!({
        "schema": cortexfs_runtime_client::agent::AGENT_INVOCATION_SCHEMA,
        "run": run,
        "step": 0,
        "input": "",
        "history_messages": "",
        "tool_context": "",
        "observation": null
    })
    .to_string()
        + "\n"
}

mod cancel;
mod history;
mod messages;
mod policy;
mod sandbox;
mod settlement;
