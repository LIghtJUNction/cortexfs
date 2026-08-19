use super::*;

pub(crate) fn run_model(name: &str, args: &[OsString]) -> Result<(), ExecError> {
    let name = resolve_model_name(name)?;
    if name == "debug/echo" {
        let stdout = io::stdout();
        return run_echo_model(
            args.iter().map(|value| value.to_string_lossy()),
            stdout.lock(),
        )
        .map_err(|error| ExecError::new(format!("echo model failed: {error}")));
    }
    let input =
        collect_input(args).map_err(|error| ExecError::with_io("cannot read input", &error))?;
    run_provider_model(&name, &input)
}

pub(crate) fn resolve_model_name(name: &str) -> Result<String, ExecError> {
    if is_model_name(name) {
        return Ok(name.to_owned());
    }
    if !is_model_alias(name) {
        return Err(ExecError::new(format!("invalid model reference: {name}")));
    }
    let ctx_root = env::var_os("CTX_ROOT").map_or_else(cortexfs_paths::ctx_root, PathBuf::from);
    resolve_model_alias(&ctx_root, name)
}

pub(crate) fn resolve_model_alias(ctx_root: &Path, name: &str) -> Result<String, ExecError> {
    let target = read_model_alias_target(ctx_root, name)
        .map_err(|_error| ExecError::new(format!("missing model alias: {name}")))?;
    let model_root = format!(
        "{}/",
        cortexfs_paths::model_root_path(&cortexfs_paths::ctx_root()).display()
    );
    let Some(model) = target.strip_prefix(&model_root) else {
        return Err(ExecError::new(format!(
            "invalid model alias target: {name}"
        )));
    };
    if !is_model_name(model) {
        return Err(ExecError::new(format!(
            "invalid model alias target: {name}"
        )));
    }
    Ok(model.to_owned())
}

#[cfg(test)]
pub(crate) fn resolved_model_path(ctx_root: &Path, model: &str) -> Result<PathBuf, ExecError> {
    let name = resolved_model_name(ctx_root, model)?;
    let (provider, model) = name
        .split_once('/')
        .ok_or_else(|| ExecError::new(format!("invalid model reference: {name}")))?;
    Ok(cortexfs_paths::model_path(ctx_root, provider, model))
}

pub(crate) fn resolved_model_name(ctx_root: &Path, model: &str) -> Result<String, ExecError> {
    Ok(if is_model_name(model) {
        model.to_owned()
    } else if is_model_alias(model) {
        resolve_model_alias(ctx_root, model)?
    } else {
        return Err(ExecError::new(format!("invalid model reference: {model}")));
    })
}

pub(crate) fn run_provider_model(name: &str, input: &str) -> Result<(), ExecError> {
    let run = env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned());
    let ctx_root = env::var_os("CTX_ROOT").map_or_else(cortexfs_paths::ctx_root, PathBuf::from);
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    let agent_window = match parse_agent_window_environment(
        env::var_os("CTX_AGENT").as_deref(),
        env::var_os("CTX_AGENT_WINDOW_SETTING").as_deref(),
    ) {
        Ok(value) => value,
        Err(error) => {
            write_tool_error(&mut stdout, &run, "EINVAL", error.message())
                .map_err(|write_error| ExecError::with_io("cannot write output", &write_error))?;
            return Err(error);
        }
    };
    let candidates = model_candidates(&ctx_root, name)?;
    let mut last_error = None;
    let agent_window = agent_window.map(|(agent, setting)| {
        let system = env::var("CTX_AGENT_SYSTEM").unwrap_or_default();
        let context = AgentPromptContext::from_env();
        let skills = env::var("CTX_AGENT_SKILLS").unwrap_or_default();
        (agent, setting, system, context, skills)
    });
    for candidate in candidates {
        write_model_start(&mut stdout, &run, &candidate.name)
            .map_err(|error| ExecError::with_io("cannot write output", &error))?;
        if let Some((ref agent, setting, ref system, ref context, ref skills)) = agent_window
            && let Err((code, message)) = admit_provider_candidate(
                &candidate.name,
                &ProviderCandidateAdmission {
                    ctx_root: &ctx_root,
                    setting,
                    agent,
                    system,
                    context,
                    skills,
                    input,
                },
            )
        {
            writeln!(
                stdout,
                "{}",
                serde_json::json!({
                    "type": "error",
                    "run": run,
                    "code": code,
                    "message": message,
                    "model": candidate.name,
                    "recoverable": true
                })
            )
            .and_then(|()| stdout.flush())
            .map_err(|error| ExecError::with_io("cannot write output", &error))?;
            last_error = Some(message);
            continue;
        }
        let result = if candidate.name == "debug/echo" {
            write_model_delta(&mut stdout, &run, input)
                .map_err(|error| ExecError::with_io("cannot write output", &error))
                .map_err(|error| ProviderCompletionError::no_fallback(error.message().to_owned()))
        } else {
            provider_chat_completion(&candidate.name, input, &run, &mut stdout)
        };
        match result {
            Ok(()) => {
                return write_tool_done(&mut stdout, &run, "ok")
                    .map_err(|error| ExecError::with_io("cannot write output", &error));
            }
            Err(error) => {
                let can_fallback = error.can_fallback;
                last_error = Some(error.message);
                if !can_fallback {
                    break;
                }
            }
        }
    }
    let error = last_error.unwrap_or_else(|| format!("missing model: {name}"));
    write_tool_error(&mut stdout, &run, "EIO", &error)
        .map_err(|error| ExecError::with_io("cannot write output", &error))?;
    Err(ExecError::new(error))
}

pub(crate) fn parse_agent_window_environment(
    agent: Option<&OsStr>,
    setting: Option<&OsStr>,
) -> Result<Option<(String, AgentWindowSetting)>, ExecError> {
    let Some(agent) = agent else {
        return Ok(None);
    };
    let agent = agent
        .to_str()
        .filter(|value| is_object_name(value))
        .ok_or_else(|| ExecError::new("invalid CTX_AGENT for Agent model call"))?;
    let setting = setting
        .and_then(OsStr::to_str)
        .and_then(AgentWindowSetting::parse_value)
        .ok_or_else(|| ExecError::new("missing or invalid CTX_AGENT_WINDOW_SETTING"))?;
    Ok(Some((agent.to_owned(), setting)))
}

pub(crate) struct ProviderCandidateAdmission<'a> {
    pub(crate) ctx_root: &'a Path,
    pub(crate) setting: AgentWindowSetting,
    pub(crate) agent: &'a str,
    pub(crate) system: &'a str,
    pub(crate) context: &'a AgentPromptContext,
    pub(crate) skills: &'a str,
    pub(crate) input: &'a str,
}

pub(crate) fn admit_provider_candidate(
    model: &str,
    admission: &ProviderCandidateAdmission<'_>,
) -> Result<(), (&'static str, String)> {
    let budget = candidate_window_budget(admission.ctx_root, model, admission.setting)
        .map_err(|error| ("EINVAL", format!("model candidate {model}: {error}")))?;
    let skill_limit = budget.map_or(MAX_SKILL_METADATA_CHARS, |value| {
        value.total_chars().saturating_mul(2).saturating_div(100)
    });
    if admission.skills.len() > skill_limit {
        return Err((
            "E2BIG",
            format!("model candidate {model}: skill metadata exceeds {skill_limit} bytes"),
        ));
    }
    let Some(budget) = budget else {
        return Ok(());
    };
    let messages = agent_provider_messages(
        admission.input,
        admission.agent,
        admission.system,
        admission.context,
    );
    let bytes = serde_json::to_vec(&messages)
        .map_err(|error| ("E2BIG", format!("cannot serialize agent prompt: {error}")))?;
    if bytes.len() > budget.input_chars() {
        return Err((
            "E2BIG",
            format!("model candidate {model}: agent prompt exceeds context window"),
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ModelCandidate {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
}

pub(crate) fn model_candidates(
    ctx_root: &Path,
    model: &str,
) -> Result<Vec<ModelCandidate>, ExecError> {
    let primary = resolved_model_name(ctx_root, model)?;
    let mut names = Vec::new();
    let mut seen = std::collections::HashSet::new();
    push_model_candidate_name(&primary, &mut names, &mut seen);
    for fallback in model_fallback_chain(ctx_root, &primary) {
        push_model_candidate_name(&fallback, &mut names, &mut seen);
        if names.len() >= MAX_MODEL_FALLBACK_CANDIDATES {
            break;
        }
    }
    names
        .into_iter()
        .map(|name| {
            let path = cortexfs_paths::model_reference_path(ctx_root, &name)
                .ok_or_else(|| ExecError::new("invalid model candidate"))?;
            Ok(ModelCandidate { name, path })
        })
        .collect()
}

pub(crate) fn push_model_candidate_name(
    name: &str,
    names: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
) {
    if is_model_name(name) && seen.insert(name.to_owned()) {
        names.push(name.to_owned());
    }
}

pub(crate) fn model_fallback_chain(ctx_root: &Path, model: &str) -> Vec<String> {
    let path = cortexfs_paths::model_route_path(ctx_root);
    let Ok(content) = read_small_plain_text_file(&path, MAX_RUNNER_CONTROL_BYTES, "runner") else {
        return Vec::new();
    };
    parse_model_transport_route_table(&content)
        .ok()
        .and_then(|table| table.model_fallback_chain(model).map(<[String]>::to_vec))
        .unwrap_or_default()
}

pub(crate) fn model_default_base_url(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let line = line
            .split_once('#')
            .map_or(line, |(value, _comment)| value)
            .trim();
        let value = line.strip_prefix("base_url=")?.trim();
        (!value.is_empty()).then_some(value.to_owned())
    })
}
