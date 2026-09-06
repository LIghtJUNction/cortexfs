use super::InteractionRequest;
impl InteractionRequest {
    #[expect(
        clippy::pattern_type_mismatch,
        reason = "borrowed fields keep validation allocation-free"
    )]
    pub fn validate(&self) -> Result<(), &'static str> {
        valid(self.request_id())?;
        self.session().map(valid).transpose()?;
        match self {
            Self::Input {
                scope,
                input,
                event,
                origin,
                cwd,
                workspace,
                ..
            } => {
                if !matches!(scope.as_str(), "private" | "shared" | "temp") {
                    return Err("scope");
                }
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
            Self::Resume { after, .. } => after.as_deref().map(valid).transpose().map(|_| ()),
            Self::Cancel { run, .. } => valid(run),
            Self::CommandResult { command_id, .. } => valid(command_id),
            Self::Status { .. } => Ok(()),
        }
    }
}
fn valid(value: &str) -> Result<(), &'static str> {
    (!value.is_empty() && value.len() <= 256 && !value.contains('\0'))
        .then_some(())
        .ok_or("field")
}
