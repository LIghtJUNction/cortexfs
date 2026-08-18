use super::InteractionRequest;

impl InteractionRequest {
    #[expect(
        clippy::pattern_type_mismatch,
        reason = "match ergonomics keep borrowed request fields readable"
    )]
    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Input {
                request_id,
                session,
                scope,
                input,
                event,
                origin,
                cwd,
                workspace,
            } => {
                valid(request_id)?;
                valid(session)?;
                valid(scope)?;
                if input.contains('\0') || origin.transport.is_empty() {
                    return Err("input");
                }
                if event.as_ref().is_some_and(|value| {
                    !value.is_object()
                        || serde_json::to_vec(value).is_ok_and(|bytes| bytes.len() > 64 * 1024)
                }) {
                    return Err("event");
                }
                cwd.as_deref().map(valid).transpose()?;
                workspace.as_deref().map(valid).transpose().map(|_| ())
            }
            Self::Resume {
                request_id,
                session,
                after,
            } => {
                valid(request_id)?;
                valid(session)?;
                after.as_deref().map(valid).transpose().map(|_| ())
            }
            Self::Status {
                request_id,
                session,
            } => valid(request_id).and_then(|()| valid(session)),
            Self::Cancel { request_id, run } => valid(request_id).and_then(|()| valid(run)),
            Self::CommandResult {
                request_id,
                session,
                command_id,
                ..
            } => {
                valid(request_id)?;
                valid(session)?;
                valid(command_id)
            }
        }
    }
}

fn valid(value: &str) -> Result<(), &'static str> {
    (!value.is_empty() && value.len() <= 256 && !value.contains('\0'))
        .then_some(())
        .ok_or("field")
}
