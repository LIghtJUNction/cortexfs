fn agent_tool_timeout_seconds() -> u64 {
    agent_tool_timeout_seconds_from_env(|name| env::var(name).ok())
}

fn agent_model_timeout_seconds() -> u64 {
    agent_model_timeout_seconds_from_env(|name| env::var(name).ok())
}

fn agent_tool_timeout_seconds_from_env(get_env: impl Fn(&str) -> Option<String>) -> u64 {
    get_env("CTX_AGENT_TOOL_TIMEOUT_SECONDS")
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| (1..=MAX_AGENT_TOOL_TIMEOUT_SECONDS).contains(value))
        .unwrap_or(AGENT_TOOL_TIMEOUT_SECONDS)
}

fn agent_model_timeout_seconds_from_env(get_env: impl Fn(&str) -> Option<String>) -> u64 {
    get_env("CTX_AGENT_MODEL_TIMEOUT_SECONDS")
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| (1..=MAX_AGENT_MODEL_TIMEOUT_SECONDS).contains(value))
        .unwrap_or(AGENT_MODEL_TIMEOUT_SECONDS)
}
