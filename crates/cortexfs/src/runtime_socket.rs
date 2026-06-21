use crate::execution::{FsExecutionPlane, default_execution_plane};
use crate::text::json_string;
use crate::{LOCAL_THREAD_SOCKET_PATH, RuntimeState};
use cortex_core::ThreadId;
use cortex_store::RequestId;
use cortexd::{LocalApiEndpoint, LocalApiRequest};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{UnixListener, UnixStream};
use tokio::task::JoinHandle;

static NEXT_SOCKET_REQUEST: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub struct DemoThreadSocket {
    path: PathBuf,
    task: JoinHandle<()>,
}

impl DemoThreadSocket {
    pub async fn start(runtime: Arc<Mutex<RuntimeState>>, owner_uid: u32) -> io::Result<Self> {
        let path = PathBuf::from(LOCAL_THREAD_SOCKET_PATH);
        prepare_socket_path(&path).await?;
        let listener = UnixListener::bind(&path)?;
        let task = tokio::spawn(async move {
            accept_thread_connections(listener, runtime, owner_uid).await;
        });
        Ok(Self { path, task })
    }

    pub async fn shutdown(self) {
        self.task.abort();
        let _ = self.task.await;
        let _ = tokio::fs::remove_file(self.path).await;
    }
}

async fn prepare_socket_path(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

async fn accept_thread_connections(
    listener: UnixListener,
    runtime: Arc<Mutex<RuntimeState>>,
    owner_uid: u32,
) {
    loop {
        let Ok((stream, _addr)) = listener.accept().await else {
            continue;
        };
        let runtime = Arc::clone(&runtime);
        tokio::spawn(async move {
            let _ = handle_thread_connection(stream, runtime, owner_uid).await;
        });
    }
}

async fn handle_thread_connection(
    mut stream: UnixStream,
    runtime: Arc<Mutex<RuntimeState>>,
    owner_uid: u32,
) -> io::Result<()> {
    let peer = stream.peer_cred()?;
    if peer.uid() != 0 && peer.uid() != owner_uid {
        stream
            .write_all(
                br#"{"type":"error","message":"permission denied"}
"#,
            )
            .await?;
        return Ok(());
    }

    let mut input = String::new();
    stream.read_to_string(&mut input).await?;
    let request_id = next_request_id();
    write_line(&mut stream, accepted_event(&request_id)).await?;
    let execution = {
        let runtime = Arc::clone(&runtime);
        let request_id = request_id.clone();
        let input = input.clone();
        tokio::task::spawn_blocking(move || execute_thread_turn(&runtime, &request_id, &input))
            .await
            .map_err(io::Error::other)?
    };
    match execution {
        Ok(message) => {
            write_line(&mut stream, delta_event(&message)).await?;
            write_line(&mut stream, message_event(&message)).await?;
        }
        Err(error) => {
            mark_thread_socket_error(&runtime, &request_id, &input, &error);
            write_line(&mut stream, error_event(&request_id, &error)).await?;
        }
    }
    write_line(&mut stream, done_event(&request_id)).await
}

fn execute_thread_turn(
    runtime: &Arc<Mutex<RuntimeState>>,
    request_id: &str,
    input: &str,
) -> Result<String, String> {
    let request = thread_request(input)?;
    let messages = {
        let mut runtime = runtime
            .lock()
            .map_err(|_error| "runtime lock poisoned".to_owned())?;
        runtime.ensure_thread_socket_session(&request.session, &request.scope, &request.cwd)?;
        let mut messages = messages_from_jsonl(&runtime.thread_socket_messages(&request.session));
        drop(runtime);
        messages.push(serde_json::json!({"role": "user", "content": request.content}));
        messages
    };
    mark_thread_socket_queued(runtime, &request.session, request_id);
    let mut plane =
        default_execution_plane().ok_or_else(|| "provider route unavailable".to_owned())?;
    let body = serde_json::json!({ "messages": messages }).to_string();
    let mut api_request = LocalApiRequest::new(
        RequestId::new(request_id.to_owned()),
        LocalApiEndpoint::ChatCompletions,
        body,
    );
    api_request = api_request
        .with_thread(ThreadId::new(request.session.clone()).map_err(|error| error.to_string())?);
    let response = handle_local_api(&mut plane, api_request).map_err(|error| error.to_string())?;
    let assistant = assistant_message(response.body());
    mark_thread_socket_drained(
        runtime,
        &request.session,
        request_id,
        &request.content,
        &assistant,
    );
    Ok(assistant)
}

fn handle_local_api(
    plane: &mut FsExecutionPlane,
    request: LocalApiRequest,
) -> Result<cortex_store::ApiResponse, cortexd::ExecutionError> {
    plane.handle_local_api(request)
}

#[cfg(test)]
fn thread_prompt(input: &str) -> Result<String, String> {
    Ok(thread_request(input)?.content)
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct SocketThreadRequest {
    session: String,
    scope: String,
    cwd: String,
    content: String,
}

fn thread_request(input: &str) -> Result<SocketThreadRequest, String> {
    let value = serde_json::from_str::<serde_json::Value>(input.trim())
        .map_err(|error| format!("invalid socket JSON: {error}"))?;
    if value.get("op").and_then(serde_json::Value::as_str) != Some("send") {
        return Err("unsupported thread socket op".to_owned());
    }
    let content = value
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(serde_json::Value::as_str)
        .filter(|content| !content.is_empty())
        .ok_or_else(|| "missing message.content".to_owned())?;
    let session = value
        .get("session")
        .and_then(serde_json::Value::as_str)
        .filter(|session| !session.is_empty())
        .unwrap_or("demo")
        .to_owned();
    let scope = value
        .get("scope")
        .and_then(serde_json::Value::as_str)
        .filter(|scope| !scope.is_empty())
        .unwrap_or("workspace")
        .to_owned();
    let cwd = value
        .get("cwd")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    Ok(SocketThreadRequest {
        session,
        scope,
        cwd,
        content: content.to_owned(),
    })
}

fn messages_from_jsonl(jsonl: &str) -> Vec<serde_json::Value> {
    jsonl
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|message| {
            message
                .get("role")
                .and_then(serde_json::Value::as_str)
                .is_some()
                && message
                    .get("content")
                    .and_then(serde_json::Value::as_str)
                    .is_some()
        })
        .collect()
}

fn assistant_message(body: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return body.trim().to_owned();
    };
    value
        .get("choices")
        .and_then(serde_json::Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or(body)
        .to_owned()
}

fn mark_thread_socket_queued(runtime: &Arc<Mutex<RuntimeState>>, session: &str, request_id: &str) {
    if let Ok(mut runtime) = runtime.lock() {
        runtime.mark_thread_session_socket_queued(session, request_id);
    }
}

fn mark_thread_socket_drained(
    runtime: &Arc<Mutex<RuntimeState>>,
    session: &str,
    request_id: &str,
    user: &str,
    assistant: &str,
) {
    if let Ok(mut runtime) = runtime.lock() {
        runtime.append_thread_session_socket_turn(session, request_id, user, assistant);
    }
}

fn mark_thread_socket_error(
    runtime: &Arc<Mutex<RuntimeState>>,
    request_id: &str,
    input: &str,
    error: &str,
) {
    let session =
        thread_request(input).map_or_else(|_error| "demo".to_owned(), |request| request.session);
    if let Ok(mut runtime) = runtime.lock() {
        runtime.mark_thread_session_socket_error(&session, request_id, error);
    }
}

async fn write_line(stream: &mut UnixStream, line: String) -> io::Result<()> {
    stream.write_all(line.as_bytes()).await?;
    stream.write_all(b"\n").await
}

fn next_request_id() -> String {
    let next = NEXT_SOCKET_REQUEST.fetch_add(1, Ordering::Relaxed);
    format!("thread-demo-{next:06}")
}

fn accepted_event(request_id: &str) -> String {
    format!(
        r#"{{"type":"accepted","request_id":{}}}"#,
        json_string(request_id)
    )
}

fn delta_event(content: &str) -> String {
    format!(r#"{{"type":"delta","content":{}}}"#, json_string(content))
}

fn message_event(content: &str) -> String {
    format!(
        r#"{{"type":"message","role":"assistant","content":{}}}"#,
        json_string(content)
    )
}

fn error_event(request_id: &str, message: &str) -> String {
    format!(
        r#"{{"type":"error","request_id":{},"message":{}}}"#,
        json_string(request_id),
        json_string(message)
    )
}

fn done_event(request_id: &str) -> String {
    format!(
        r#"{{"type":"done","request_id":{}}}"#,
        json_string(request_id)
    )
}

#[cfg(test)]
mod tests {
    use super::{assistant_message, thread_prompt};

    #[test]
    fn parses_thread_socket_send_prompt() -> Result<(), String> {
        assert_eq!(
            thread_prompt(r#"{"op":"send","message":{"role":"user","content":"hi"}}"#)?,
            "hi"
        );
        Ok(())
    }

    #[test]
    fn extracts_openai_chat_assistant_message() {
        assert_eq!(
            assistant_message(
                r#"{"choices":[{"message":{"role":"assistant","content":"hello"}}]}"#,
            ),
            "hello"
        );
    }
}
