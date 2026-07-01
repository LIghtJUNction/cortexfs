#[derive(Clone, Debug, Eq, PartialEq)]
struct AgentProcess {
    name: String,
    parent: Option<String>,
    parent_session: Option<String>,
    status: String,
    pid: Option<String>,
    model: String,
    life: String,
}

fn agent_ps(root: &Path) -> Result<(), CliError> {
    let processes = read_agent_processes(root)?;
    for line in render_agent_process_forest(&processes) {
        print_line(&line)?;
    }
    Ok(())
}

fn render_agent_process_forest(processes: &[AgentProcess]) -> Vec<String> {
    let mut processes = processes.to_vec();
    processes.sort_by(|left, right| left.name.cmp(&right.name));
    let names = processes
        .iter()
        .map(|process| process.name.clone())
        .collect::<Vec<_>>();
    let mut rendered = Vec::new();
    let mut tree_renderer = AgentProcessTreeRenderer::new(&processes, &mut rendered);
    for process in &processes {
        if process
            .parent
            .as_ref()
            .is_none_or(|parent| !names.contains(parent))
        {
            tree_renderer.render(process, "", true, true);
        }
    }
    for process in &processes {
        if !tree_renderer.rendered_names.contains(&process.name) {
            tree_renderer.render(process, "", true, true);
        }
    }
    rendered
}

#[cfg(test)]
fn render_agent_process_tree(
    process: &AgentProcess,
    processes: &[AgentProcess],
    prefix: &str,
    last: bool,
    root: bool,
    rendered: &mut Vec<String>,
) {
    AgentProcessTreeRenderer::new(processes, rendered).render(process, prefix, last, root);
}

struct AgentProcessTreeRenderer<'a> {
    processes: &'a [AgentProcess],
    rendered: &'a mut Vec<String>,
    rendered_names: HashSet<String>,
}

impl<'a> AgentProcessTreeRenderer<'a> {
    fn new(processes: &'a [AgentProcess], rendered: &'a mut Vec<String>) -> Self {
        Self {
            processes,
            rendered,
            rendered_names: HashSet::new(),
        }
    }

    fn render(&mut self, process: &AgentProcess, prefix: &str, last: bool, root: bool) {
        if !self.rendered_names.insert(process.name.clone()) {
            return;
        }
        let branch = if root {
            ""
        } else if last {
            "`- "
        } else {
            "+- "
        };
        self.rendered.push(format!(
            "{prefix}{branch}{} [{}]{}{}{}{}{}",
            terminal_safe_text(&process.name),
            terminal_safe_text(&process.status),
            process_model_suffix(process),
            process_life_suffix(process),
            process_role_suffix(process),
            process_parent_session_suffix(process),
            process.pid.as_ref().map_or_else(String::new, |pid| format!(
                " pid={}",
                terminal_safe_text(pid)
            ))
        ));

        let next_prefix = if root {
            String::new()
        } else if last {
            format!("{prefix}   ")
        } else {
            format!("{prefix}|  ")
        };
        let children = self
            .processes
            .iter()
            .filter(|candidate| candidate.parent.as_deref() == Some(process.name.as_str()))
            .collect::<Vec<_>>();
        let child_count = children.len();
        for (index, child) in children.into_iter().enumerate() {
            self.render(child, &next_prefix, index + 1 == child_count, false);
        }
    }
}

fn read_agent_processes(root: &Path) -> Result<Vec<AgentProcess>, CliError> {
    let names = list_kind_names(root, ObjectClass::Agent)?;
    let mut processes = Vec::new();
    for name in names {
        let control = root.join("agent").join(format!("{name}.d"));
        let parent = read_agent_parent_ref(&control)?;
        let (status, pid) = live_agent_status_and_pid(&control)?;
        let model = read_agent_model_for_context(&control, "agent")?;
        let life = read_agent_life_for_context(&control, "agent")?;
        processes.push(AgentProcess {
            name,
            parent: parent.as_ref().map(|parent| parent.agent.clone()),
            parent_session: parent.and_then(|parent| parent.session),
            status,
            pid,
            model,
            life,
        });
    }
    Ok(processes)
}

fn default_agent_process_model(name: &str) -> &'static str {
    default_agent_model_for_name(name)
}

fn live_agent_status_and_pid(control: &Path) -> Result<(String, Option<String>), CliError> {
    let status =
        read_agent_control_trimmed(control, "status")?.unwrap_or_else(|| "unknown".to_owned());
    let recorded_pid = read_agent_control_trimmed(control, "pid")?;
    let pid = live_agent_pid(recorded_pid.clone());
    let status = if recorded_pid.is_some()
        && pid.is_none()
        && matches!(status.as_str(), "ready" | "busy")
    {
        "dead".to_owned()
    } else {
        status
    };
    Ok((status, pid))
}

fn live_agent_pid(pid: Option<String>) -> Option<String> {
    let pid = pid?;
    if pid.bytes().all(|byte| byte.is_ascii_digit()) && Path::new("/proc").join(&pid).is_dir() {
        Some(pid)
    } else {
        None
    }
}

fn process_model_suffix(process: &AgentProcess) -> String {
    if process.model == "main" {
        String::new()
    } else {
        format!(" model={}", terminal_safe_text(&process.model))
    }
}

fn process_life_suffix(process: &AgentProcess) -> String {
    if process.life == "owned" {
        String::new()
    } else {
        format!(" life={}", terminal_safe_text(&process.life))
    }
}

fn process_role_suffix(process: &AgentProcess) -> String {
    if is_worker_agent_name(&process.name) { " role=worker".to_owned() } else { String::new() }
}

fn process_parent_session_suffix(process: &AgentProcess) -> String {
    process.parent_session.as_ref().map_or_else(String::new, |session| {
        format!(" parent_session={}", terminal_safe_text(session))
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AgentParentRef {
    agent: String,
    session: Option<String>,
}

fn read_agent_parent_ref(control: &Path) -> Result<Option<AgentParentRef>, CliError> {
    let Some(parent) = read_agent_control_trimmed(control, "parent")? else {
        return Ok(None);
    };
    parse_agent_parent_ref(&parent).map(Some).ok_or_else(|| {
        let agent = control
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".d"))
            .unwrap_or("agent");
        CliError::usage(format!("invalid agent parent for {agent}: {parent}"))
    })
}

fn read_agent_life_for_context(control: &Path, context: &str) -> Result<String, CliError> {
    let life = read_agent_control_trimmed(control, "life")?.unwrap_or_else(|| "owned".to_owned());
    if cortexfs::ChildLifecycle::parse(&life).is_err() {
        return Err(CliError::usage(format!(
            "invalid {context} life for {}: {life}",
            control_agent_name(control)
        )));
    }
    Ok(life)
}

fn read_agent_model_for_context(control: &Path, context: &str) -> Result<String, CliError> {
    let model = read_agent_control_trimmed(control, "model")?
        .unwrap_or_else(|| default_agent_process_model(control_agent_name(control)).to_owned());
    if !(is_model_name(&model) || matches!(model.as_str(), "main" | "helper")) {
        return Err(CliError::usage(format!(
            "invalid {context} model for {}: {model}",
            control_agent_name(control)
        )));
    }
    Ok(model)
}

fn control_agent_name(control: &Path) -> &str {
    control
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(".d"))
        .unwrap_or("agent")
}

fn parse_agent_parent_ref(value: &str) -> Option<AgentParentRef> {
    let mut fields = value.split_whitespace();
    let agent = fields.next()?.strip_prefix("agent:")?;
    if !is_object_name(agent) {
        return None;
    }
    let mut session = None;
    let mut run = false;
    for word in fields {
        let (kind, name) = word.split_once(':')?;
        match kind {
            "session" if is_object_name(name) && session.is_none() => {
                session = Some(name.to_owned());
            }
            "run" if is_object_name(name) && !run => {
                run = true;
            }
            _ => return None,
        }
    }
    Some(AgentParentRef {
        agent: agent.to_owned(),
        session,
    })
}

fn read_agent_control_trimmed(control: &Path, file: &str) -> Result<Option<String>, CliError> {
    let path = control.join(file);
    match fs::symlink_metadata(&path) {
        Ok(_metadata) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(CliError::unavailable(format!(
                "cannot stat {}: {error}",
                path.display()
            )));
        }
    }
    let content = read_file_to_string(&path)?;
    let value = content.trim().to_owned();
    Ok((!value.is_empty()).then_some(value))
}
