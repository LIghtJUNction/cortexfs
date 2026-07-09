use crate::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentProcess {
    pub(crate) name: String,
    pub(crate) parent: Option<String>,
    pub(crate) parent_session: Option<String>,
    pub(crate) parent_run: Option<String>,
    pub(crate) status: String,
    pub(crate) ppid: Option<String>,
    pub(crate) pid: Option<String>,
    pub(crate) model: String,
    pub(crate) life: String,
}

pub(crate) fn agent_ps(root: &Path) -> Result<(), CliError> {
    let processes = read_agent_processes(root)?;
    for line in render_agent_process_forest(&processes) {
        print_line(&line)?;
    }
    Ok(())
}

pub(crate) fn render_agent_process_forest(processes: &[AgentProcess]) -> Vec<String> {
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
pub(crate) fn render_agent_process_tree(
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
            "{prefix}{branch}{} [{}]{}{}{}{}{}{}",
            terminal_safe_text(&process.name),
            terminal_safe_text(&process.status),
            process_model_suffix(process),
            process_life_suffix(process),
            process_role_suffix(process),
            process_parent_suffix(process),
            process_id_suffix("ppid", process.ppid.as_deref()),
            process_id_suffix("pid", process.pid.as_deref())
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

pub(crate) fn read_agent_processes(root: &Path) -> Result<Vec<AgentProcess>, CliError> {
    let names = list_kind_names(root, ObjectClass::Agent)?;
    let mut processes = Vec::new();
    for name in names {
        let control = agent_control_dir(root, &name);
        let parent = read_agent_parent_ref(&control)?;
        let (status, pid) = live_agent_status_and_pid(&control)?;
        let (model, life) = read_agent_model_life_for_context(&control, "agent")?;
        processes.push(AgentProcess {
            name,
            parent: parent.as_ref().map(|parent| parent.agent.clone()),
            parent_session: parent.as_ref().and_then(|parent| parent.session.clone()),
            parent_run: parent.and_then(|parent| parent.run),
            status,
            ppid: None,
            pid,
            model,
            life,
        });
    }
    let parent_pids = processes
        .iter()
        .map(|process| {
            process.parent.as_ref().and_then(|parent| {
                processes
                    .iter()
                    .find(|candidate| candidate.name == *parent)
                    .and_then(|candidate| candidate.pid.clone())
            })
        })
        .collect::<Vec<_>>();
    for (process, ppid) in processes.iter_mut().zip(parent_pids) {
        process.ppid = ppid;
    }
    Ok(processes)
}

pub(crate) fn default_agent_process_model(name: &str) -> &'static str {
    default_agent_model_for_name(name)
}

pub(crate) fn agent_object_path(root: &Path, agent: &str) -> PathBuf {
    root.join("agent").join(agent)
}

pub(crate) fn agent_control_dir(root: &Path, agent: &str) -> PathBuf {
    agent_user_control_dir(root, agent)
        .filter(|control| is_plain_dir(control))
        .unwrap_or_else(|| agent_object_path(root, &format!("{agent}.d")))
}

pub(crate) fn agent_user_control_dir(root: &Path, agent: &str) -> Option<PathBuf> {
    Some(
        ctx_home(root)
            .ok()?
            .join("agent")
            .join(format!("{agent}.d")),
    )
}

pub(crate) fn is_plain_dir(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_dir() && !metadata.file_type().is_symlink())
}

pub(crate) fn agent_control_dirs(root: &Path) -> Result<Vec<(String, PathBuf)>, CliError> {
    let agent_root = agent_object_path(root, "");
    let entries = fs::read_dir(&agent_root).map_err(|error| {
        CliError::unavailable(format!("cannot read {}: {error}", agent_root.display()))
    })?;
    let mut controls = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            CliError::unavailable(format!("cannot read {}: {error}", agent_root.display()))
        })?;
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str().and_then(|name| name.strip_suffix(".d")) else {
            continue;
        };
        if is_object_name(name) {
            controls.push((name.to_owned(), entry.path()));
        }
    }
    Ok(controls)
}

pub(crate) fn live_agent_status_and_pid(
    control: &Path,
) -> Result<(String, Option<String>), CliError> {
    let status =
        read_agent_control_trimmed(control, "status")?.unwrap_or_else(|| "unknown".to_owned());
    let recorded_pid = read_agent_control_trimmed(control, "pid")?;
    let pid = live_agent_pid(recorded_pid.clone());
    let status =
        if recorded_pid.is_some() && pid.is_none() && matches!(status.as_str(), "ready" | "busy") {
            "dead".to_owned()
        } else {
            status
        };
    Ok((status, pid))
}

pub(crate) fn live_agent_pid(pid: Option<String>) -> Option<String> {
    let pid = pid?;
    if pid.bytes().all(|byte| byte.is_ascii_digit()) && Path::new("/proc").join(&pid).is_dir() {
        Some(pid)
    } else {
        None
    }
}

pub(crate) fn process_model_suffix(process: &AgentProcess) -> String {
    if process.model == "main" {
        String::new()
    } else {
        format!(" model={}", terminal_safe_text(&process.model))
    }
}

pub(crate) fn process_life_suffix(process: &AgentProcess) -> String {
    if process.life == "owned" {
        String::new()
    } else {
        format!(" life={}", terminal_safe_text(&process.life))
    }
}

pub(crate) fn process_role_suffix(process: &AgentProcess) -> String {
    if agent_role_for_display(&process.name) == "worker" {
        " role=worker".to_owned()
    } else {
        String::new()
    }
}

pub(crate) fn process_parent_suffix(process: &AgentProcess) -> String {
    let mut suffix = String::new();
    if let Some(session) = process.parent_session.as_ref() {
        suffix.push_str(" parent_session=");
        suffix.push_str(&terminal_safe_text(session));
    }
    if let Some(run) = process.parent_run.as_ref() {
        suffix.push_str(" parent_run=");
        suffix.push_str(&terminal_safe_text(run));
    }
    suffix
}

pub(crate) fn process_id_suffix(label: &str, pid: Option<&str>) -> String {
    pid.map_or_else(String::new, |pid| {
        format!(" {label}={}", terminal_safe_text(pid))
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentParentRef {
    pub(crate) agent: String,
    pub(crate) session: Option<String>,
    pub(crate) run: Option<String>,
}

pub(crate) fn agent_parent_ref_display(parent: &AgentParentRef) -> String {
    let mut display = format!("agent:{}", parent.agent);
    if let Some(session) = parent.session.as_ref() {
        display.push_str(" session:");
        display.push_str(session);
    }
    if let Some(run) = parent.run.as_ref() {
        display.push_str(" run:");
        display.push_str(run);
    }
    display
}

pub(crate) fn agent_parent_ref_matches(
    parent: &AgentParentRef,
    agent: &str,
    session: Option<&str>,
    run: Option<&str>,
) -> bool {
    parent.agent == agent
        && parent
            .session
            .as_deref()
            .is_none_or(|parent_session| Some(parent_session) == session)
        && run.is_none_or(|expected| parent.run.as_deref() == Some(expected))
}

pub(crate) fn read_agent_parent_ref(control: &Path) -> Result<Option<AgentParentRef>, CliError> {
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

pub(crate) fn read_agent_life_for_context(
    control: &Path,
    context: &str,
) -> Result<String, CliError> {
    let life = read_agent_control_trimmed(control, "life")?.unwrap_or_else(|| "owned".to_owned());
    if cortexfs::ChildLifecycle::parse(&life).is_err() {
        return Err(CliError::usage(format!(
            "invalid {context} life for {}: {life}",
            control_agent_name(control)
        )));
    }
    Ok(life)
}

pub(crate) fn read_agent_model_life_for_context(
    control: &Path,
    context: &str,
) -> Result<(String, String), CliError> {
    Ok((
        read_agent_model_for_context(control, context)?,
        read_agent_life_for_context(control, context)?,
    ))
}

pub(crate) fn read_agent_model_for_context(
    control: &Path,
    context: &str,
) -> Result<String, CliError> {
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

pub(crate) fn control_agent_name(control: &Path) -> &str {
    control
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(".d"))
        .unwrap_or("agent")
}

pub(crate) fn parse_agent_parent_ref(value: &str) -> Option<AgentParentRef> {
    let mut fields = value.split_whitespace();
    let agent = fields.next()?.strip_prefix("agent:")?;
    if !is_object_name(agent) {
        return None;
    }
    let mut session = None;
    let mut run = None;
    for word in fields {
        let (kind, name) = word.split_once(':')?;
        match kind {
            "session" if is_object_name(name) && session.is_none() => {
                session = Some(name.to_owned());
            }
            "run" if is_object_name(name) && run.is_none() => {
                run = Some(name.to_owned());
            }
            _ => return None,
        }
    }
    Some(AgentParentRef {
        agent: agent.to_owned(),
        session,
        run,
    })
}

pub(crate) fn read_agent_control_trimmed(
    control: &Path,
    file: &str,
) -> Result<Option<String>, CliError> {
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
