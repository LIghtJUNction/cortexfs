use super::*;

pub(crate) fn authorize_agent_model_use(
    agent_dir: &Path,
    requested_model: &str,
    primary_model: &str,
    selected_model: &str,
) -> Result<(), ExecError> {
    let label =
        read_small_plain_text_file(&agent_dir.join("label"), MAX_RUNNER_CONTROL_BYTES, "runner")
            .map_err(|error| ExecError::with_io("cannot read agent label", &error))?;
    let subject = policy_subject_from_label(label.trim())
        .ok_or_else(|| ExecError::new("invalid agent label"))?;
    let policy_text = read_small_plain_text_file(
        &agent_dir.join("policy"),
        MAX_RUNNER_CONTROL_BYTES,
        "runner",
    )
    .map_err(|error| ExecError::with_io("cannot read agent policy", &error))?;
    let policy =
        PolicyV0::parse(&policy_text).map_err(|_error| ExecError::new("invalid agent policy"))?;
    match authorize_model_use(
        requested_model,
        primary_model,
        selected_model,
        ModelUseAuthority::new(subject, &policy),
    ) {
        Ok(()) => Ok(()),
        Err(ModelUseDenial::PrimaryFallback) => Err(ExecError::new(format!(
            "agent policy denies requested model:{requested_model} use via selected primary:{selected_model}"
        ))),
        Err(ModelUseDenial::Selected) => Err(ExecError::new(format!(
            "agent policy denies model:{selected_model} use"
        ))),
    }
}

pub(crate) fn agent_debug_timing_start_unix_ms() -> Option<u128> {
    if env::var("CTX_AGENT_DEBUG_TIMING").ok().as_deref() != Some("1") {
        return None;
    }
    env::var("CTX_AGENT_DEBUG_START_UNIX_MS")
        .ok()
        .and_then(|value| value.parse::<u128>().ok())
}

pub(crate) fn current_unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
}

pub(crate) fn write_agent_debug_timing(
    stdout: &mut impl Write,
    config: &AgentModelRunConfig,
    stage: &str,
) -> Result<(), ExecError> {
    let Some(start_unix_ms) = config.debug_timing_start_unix_ms else {
        return Ok(());
    };
    let elapsed_ms = current_unix_millis().saturating_sub(start_unix_ms);
    let frame = serde_json::json!({
        "type": "debug",
        "stage": stage,
        "elapsed_ms": elapsed_ms
    });
    writeln!(stdout, "{frame}")
        .and_then(|()| stdout.flush())
        .map_err(|error| ExecError::new(format!("cannot write output: {error}")))
}

pub(crate) struct AgentModelRunOutcome {
    pub(crate) frames: Vec<String>,
    pub(crate) success: bool,
    pub(crate) streamed: bool,
}
