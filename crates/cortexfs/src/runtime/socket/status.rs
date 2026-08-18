use super::*;
use crate::runtime::state::RuntimeState;

/// Returns a bounded, non-secret status projection for one durable session.
pub(crate) fn handle_socket_status(
    session_root: &Path,
    model: Option<&str>,
    session: &str,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    if !is_object_name(session) {
        return Err(SocketRuntimeError::InvalidSessionName);
    }
    let session_dir = session_root.join(session);
    let state = read_state(&session_dir, model)?;
    let mut value =
        serde_json::to_value(state).map_err(|_error| SocketRuntimeError::CannotReadEvents)?;
    let object = value
        .as_object_mut()
        .ok_or(SocketRuntimeError::CannotReadEvents)?;
    object.insert("type".to_owned(), serde_json::json!("status"));
    object.insert("session".to_owned(), serde_json::json!(session));
    Ok(SocketRuntimeResponse::new(vec![value.to_string()]))
}

fn read_state(session_dir: &Path, model: Option<&str>) -> Result<RuntimeState, SocketRuntimeError> {
    match support::plain::read_small_text_file(
        &session_dir.join("state.json"),
        MAX_SOCKET_RUNTIME_SMALL_FILE_BYTES,
    ) {
        Ok(state) => serde_json::from_str::<RuntimeState>(&state)
            .map_err(|_error| SocketRuntimeError::CannotReadEvents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match support::plain::read_small_text_file(
                &session_dir.join("state"),
                MAX_SOCKET_RUNTIME_SMALL_FILE_BYTES,
            ) {
                Ok(state) => {
                    let status = match state.trim() {
                        "idle" | "active" | "running" | "done" | "error" | "cancelled" => {
                            state.trim()
                        }
                        _ => "unknown",
                    };
                    let mut value = RuntimeState::idle(model, "");
                    status.clone_into(&mut value.status);
                    status.clone_into(&mut value.phase);
                    Ok(value)
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(RuntimeState::idle(model, ""))
                }
                Err(_error) => Err(SocketRuntimeError::CannotReadEvents),
            }
        }
        Err(_error) => Err(SocketRuntimeError::CannotReadEvents),
    }
}
