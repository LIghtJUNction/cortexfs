fn run_model(name: &str, args: &[OsString]) -> Result<(), String> {
    let name = resolve_model_name(name)?;
    if name == "debug/echo" {
        let stdout = io::stdout();
        return run_echo_model(
            args.iter().map(|value| value.to_string_lossy()),
            stdout.lock(),
        )
        .map_err(|error| format!("echo model failed: {error}"));
    }
    let input = collect_input(args).map_err(|error| format!("cannot read input: {error}"))?;
    run_provider_model(&name, &input)
}

fn resolve_model_name(name: &str) -> Result<String, String> {
    if is_model_name(name) {
        return Ok(name.to_owned());
    }
    if !is_model_alias(name) {
        return Err(format!("invalid model reference: {name}"));
    }
    let ctx_root =
        env::var_os("CTX_ROOT").map_or_else(|| PathBuf::from(DEFAULT_CTX_ROOT), PathBuf::from);
    resolve_model_alias(&ctx_root, name)
}

fn resolve_model_alias(ctx_root: &Path, name: &str) -> Result<String, String> {
    let target = read_model_alias_target(ctx_root, name)
        .map_err(|_error| format!("missing model alias: {name}"))?;
    let Some(model) = target.strip_prefix("/ctx/model/") else {
        return Err(format!("invalid model alias target: {name}"));
    };
    if !is_model_name(model) {
        return Err(format!("invalid model alias target: {name}"));
    }
    Ok(model.to_owned())
}

fn is_model_alias(name: &str) -> bool {
    matches!(name, "main" | "helper")
}

#[cfg(test)]
fn resolved_model_path(ctx_root: &Path, model: &str) -> Result<PathBuf, String> {
    let name = resolved_model_name(ctx_root, model)?;
    Ok(ctx_root.join("model").join(name))
}

fn resolved_model_name(ctx_root: &Path, model: &str) -> Result<String, String> {
    Ok(if is_model_name(model) {
        model.to_owned()
    } else if is_model_alias(model) {
        resolve_model_alias(ctx_root, model)?
    } else {
        return Err(format!("invalid model reference: {model}"));
    })
}

fn run_provider_model(name: &str, input: &str) -> Result<(), String> {
    let run = env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned());
    let ctx_root =
        env::var_os("CTX_ROOT").map_or_else(|| PathBuf::from(DEFAULT_CTX_ROOT), PathBuf::from);
    let candidates = model_candidates(&ctx_root, name)?;
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    let mut last_error = None;
    for candidate in candidates {
        write_model_start(&mut stdout, &run, &candidate.name)
            .map_err(|error| format!("cannot write output: {error}"))?;
        let result = if candidate.name == "debug/echo" {
            write_model_delta(&mut stdout, &run, input)
                .map_err(|error| format!("cannot write output: {error}"))
                .map_err(ProviderCompletionError::no_fallback)
        } else {
            provider_chat_completion(&candidate.name, input, &run, &mut stdout)
        };
        match result {
            Ok(()) => {
                return write_tool_done(&mut stdout, &run, "ok")
                    .map_err(|error| format!("cannot write output: {error}"));
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
        .map_err(|error| format!("cannot write output: {error}"))?;
    Err(error)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ModelCandidate {
    name: String,
    path: PathBuf,
}

fn model_candidates(ctx_root: &Path, model: &str) -> Result<Vec<ModelCandidate>, String> {
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
    Ok(names
        .into_iter()
        .map(|name| ModelCandidate {
            path: ctx_root.join("model").join(&name),
            name,
        })
        .collect())
}

fn push_model_candidate_name(
    name: &str,
    names: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
) {
    if is_model_name(name) && seen.insert(name.to_owned()) {
        names.push(name.to_owned());
    }
}

fn model_fallback_chain(ctx_root: &Path, model: &str) -> Vec<String> {
    let Some((provider, name)) = model.split_once('/') else {
        return Vec::new();
    };
    let path = ctx_root
        .join("model")
        .join(provider)
        .join(format!("{name}.d"))
        .join("fallback");
    let Ok(content) = read_small_plain_text_file(&path, MAX_RUNNER_CONTROL_BYTES, "runner") else {
        return Vec::new();
    };
    let (fallback, report) = parse_model_fallback(&content);
    if report.is_ok() {
        fallback.models().to_vec()
    } else {
        Vec::new()
    }
}
