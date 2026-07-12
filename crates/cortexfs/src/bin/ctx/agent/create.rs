use crate::*;

pub(crate) fn agent_new_request_json(args: &AgentNewArgs) -> Result<String, CliError> {
    require_cli_name("agent name", &args.name)?;
    if let Some(label) = args.label.as_deref()
        && label.trim().is_empty()
    {
        return Err(CliError::usage("agent label must not be empty"));
    }
    if let Some(parent) = args.parent.as_deref()
        && parse_agent_parent_ref(parent).is_none()
    {
        return Err(CliError::usage(format!("invalid agent parent: {parent}")));
    }
    for model in &args.models {
        if !is_model_name(model) {
            return Err(CliError::usage(format!("invalid model name: {model}")));
        }
    }
    let mut seen_tools = std::collections::BTreeSet::new();
    for tool in &args.tools {
        require_cli_name("tool name", tool)?;
        if tool == "tsh" || !seen_tools.insert(tool) {
            return Err(CliError::usage(format!(
                "invalid or duplicate direct tool: {tool}"
            )));
        }
    }
    for shared in &args.shared {
        require_cli_name("shared name", &shared.name)?;
        if !matches!(shared.access.as_str(), "read" | "write") {
            return Err(CliError::usage(format!(
                "invalid shared access for {}: {}",
                shared.name, shared.access
            )));
        }
    }
    for mount in &args.mounts {
        require_agent_mount(mount)?;
    }

    let mut fields = vec![format!("\"name\":{}", json_string(&args.name))];
    if args.temporary {
        fields.push("\"life\":\"temp\"".to_owned());
    }
    if let Some(parent) = args.parent.as_deref() {
        fields.push(format!("\"parent\":{}", json_string(parent)));
    }
    if let Some(label) = args.label.as_deref() {
        fields.push(format!("\"label\":{}", json_string(label)));
    }
    if !args.models.is_empty() {
        fields.push(format!("\"model\":[{}]", json_string_list(&args.models)));
    }
    if !args.tools.is_empty() {
        fields.push(format!("\"tools\":[{}]", json_string_list(&args.tools)));
    }
    if !args.shared.is_empty() {
        fields.push(format!("\"shared\":{}", agent_shared_json(&args.shared)));
    }
    if !args.mounts.is_empty() {
        fields.push(format!("\"mount\":{}", agent_mounts_json(&args.mounts)));
    }
    if let Some(instructions) = args.instructions.as_deref() {
        fields.push(format!("\"instructions\":{}", json_string(instructions)));
    }
    if let Some(description) = args.description.as_deref() {
        fields.push(format!("\"description\":{}", json_string(description)));
    }
    Ok(format!("{{{}}}", fields.join(",")))
}

pub(crate) fn agent_shared_json(shared: &[AgentShared]) -> String {
    let fields = shared
        .iter()
        .map(|entry| {
            format!(
                "{}:[{}]",
                json_string(&entry.name),
                json_string(&entry.access)
            )
        })
        .collect::<Vec<_>>();
    format!("{{{}}}", fields.join(","))
}

pub(crate) fn agent_mounts_json(mounts: &[AgentMount]) -> String {
    let items = mounts
        .iter()
        .map(|mount| {
            format!(
                "[{},{},{}]",
                json_string(&mount.source),
                json_string(&mount.target),
                json_string(&mount.mode)
            )
        })
        .collect::<Vec<_>>();
    format!("[{}]", items.join(","))
}

pub(crate) fn json_string_list(values: &[String]) -> String {
    values
        .iter()
        .map(|value| json_string(value))
        .collect::<Vec<_>>()
        .join(",")
}
