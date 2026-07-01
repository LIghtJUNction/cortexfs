fn agent_status(root: &Path, name: &str) -> Result<(), CliError> {
    for line in agent_status_lines(root, name)? {
        print_line(&line)?;
    }
    Ok(())
}

fn agent_status_lines(root: &Path, name: &str) -> Result<Vec<String>, CliError> {
    require_cli_name("agent name", name)?;
    let control = agent_control_dir(root, name);
    let (status, live_pid) = live_agent_status_and_pid(&control)?;
    let (model, life) = read_agent_model_life_for_context(&control, "agent")?;
    let parent = read_agent_parent_ref(&control)?;
    let parent_live_pid = agent_parent_live_pid(root, parent.as_ref())?;
    let parent = parent
        .as_ref()
        .map_or_else(|| "-".to_owned(), agent_parent_ref_display);
    Ok(vec![
        terminal_safe_text(&status),
        format!(
            "model={}",
            terminal_safe_text(&model)
        ),
        format!("life={}", terminal_safe_text(&life)),
        format!(
            "role={}",
            agent_role_for_display(name)
        ),
        format!("parent={}", terminal_safe_text(&parent)),
        format!("children={}", agent_status_child_count(root, name)?),
        format!(
            "pid={}",
            terminal_safe_text(&live_pid.unwrap_or_else(|| "-".to_owned()))
        ),
        format!(
            "ppid={}",
            terminal_safe_text(&parent_live_pid.unwrap_or_else(|| "-".to_owned()))
        ),
        format!(
            "uid={}",
            terminal_safe_text(
                &read_optional_trimmed(&control.join("uid"))?.unwrap_or_else(|| "-".to_owned())
            )
        ),
        format!(
            "gid={}",
            terminal_safe_text(
                &read_optional_trimmed(&control.join("gid"))?.unwrap_or_else(|| "-".to_owned())
            )
        ),
        format!(
            "groups={}",
            terminal_safe_text(&agent_status_groups(&control)?)
        ),
        format!(
            "root={}",
            terminal_safe_text(
                &read_optional_trimmed(&control.join("root"))?.unwrap_or_else(|| "-".to_owned())
            )
        ),
        format!(
            "cwd={}",
            terminal_safe_text(
                &read_optional_trimmed(&control.join("cwd"))?.unwrap_or_else(|| "-".to_owned())
            )
        ),
    ])
}

fn agent_parent_live_pid(
    root: &Path,
    parent: Option<&AgentParentRef>,
) -> Result<Option<String>, CliError> {
    let Some(parent) = parent else {
        return Ok(None);
    };
    agent_live_pid(root, &parent.agent)
}

fn agent_live_pid(root: &Path, agent: &str) -> Result<Option<String>, CliError> {
    Ok(live_agent_status_and_pid(&agent_control_dir(root, agent))?.1)
}

fn agent_status_child_count(root: &Path, name: &str) -> Result<usize, CliError> {
    let mut count = 0usize;
    for (child_name, control) in agent_control_dirs(root)? {
        if child_name == name {
            continue;
        }
        if read_agent_parent_ref(&control)?.is_some_and(|parent| parent.agent == name)
            && live_agent_status_and_pid(&control)?.0 != "dead"
        {
            read_agent_model_life_for_context(&control, "agent")?;
            count += 1;
        }
    }
    Ok(count)
}

fn agent_status_groups(control: &Path) -> Result<String, CliError> {
    let Some(groups) = read_optional_trimmed(&control.join("groups"))? else {
        return Ok("-".to_owned());
    };
    let groups = groups.split_whitespace().collect::<Vec<_>>().join(" ");
    Ok(if groups.is_empty() {
        "-".to_owned()
    } else {
        groups
    })
}

fn agent_env(root: &Path, name: &str) -> Result<(), CliError> {
    for line in agent_env_lines(root, name)? {
        print_line(&line)?;
    }
    Ok(())
}

fn agent_env_lines(root: &Path, name: &str) -> Result<Vec<String>, CliError> {
    require_cli_name("agent name", name)?;
    let view = derive_agent_runtime_view(root, name)
        .map_err(|error| CliError::unavailable(format!("agent view {}: {name}", error.errno())))?;
    Ok(agent_sandbox_env(root, &view)
        .into_iter()
        .map(|(key, value)| format!("{}={}", terminal_safe_text(&key), terminal_safe_text(&value)))
        .collect())
}

fn agent_pack(root: &Path, name: &str, session: Option<&str>) -> Result<(), CliError> {
    let session_dir = agent_session_dir(root, name, session)?;
    let context = session_dir.join("context");
    for file in ["pack.md", "pack.json", "summary.md"] {
        let path = context.join(file);
        if path
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.is_file())
        {
            return cat_path(&path);
        }
    }
    Err(CliError::unavailable(format!(
        "missing context pack: {}",
        context.join("pack.md").display()
    )))
}

fn agent_prompt(root: &Path, name: &str) -> Result<(), CliError> {
    let prompt = build_agent_system_prompt(root, name, &current_time_unix().to_string())?;
    print_terminal_text(&prompt)
}

fn build_agent_system_prompt(
    root: &Path,
    name: &str,
    current_time_unix: &str,
) -> Result<String, CliError> {
    require_cli_name("agent name", name)?;
    let control = agent_control_dir(root, name);
    let agent_system = read_optional_trimmed(&control.join("system.md"))?.unwrap_or_default();
    let template = read_optional_trimmed(&control.join("prompt.template.md"))?
        .unwrap_or_else(|| DEFAULT_AGENT_PROMPT_TEMPLATE.to_owned());
    Ok(render_agent_system_prompt(
        name,
        &agent_system,
        &AgentPromptContext {
            template,
            rules: collect_agent_rules(),
            skills: collect_skill_metadata(skill_metadata_budget_from_env()),
            tool_injection: "(no repo structure, search result, or file content injected)"
                .to_owned(),
            history_messages: "(no historical messages injected)".to_owned(),
            current_time_unix: current_time_unix.to_owned(),
        },
    ))
}
