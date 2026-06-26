fn file_check(root: &Path, path: &str) -> Result<(), CliError> {
    let resolved = resolve_abi_path(root, path)?;
    let abi_path = classify_input_path(root, path)?;
    let parsed = parse_abi_path(&abi_path);
    let shape = parsed.stable_type();
    if shape == "ctx.unknown" {
        return file_type(root, path);
    }

    if file_check_policy_or_mount(parsed, &resolved)? {
        return Ok(());
    }

    if parsed.is_tool_schema() {
        let content = read_file_to_string(&resolved)?;
        let report = inspect_tool_schema_json(&content);
        return check_report("tool schema", report.is_ok(), || {
            format_tool_schema_issues(report.issues())
        });
    }

    if matches!(parsed, AbiPathKind::SharedQueueRoot { .. }) {
        let report = inspect_shared_queue_layout(&resolved);
        return check_report("shared queue", report.is_ok(), || {
            format_shared_queue_layout_issues(report.issues())
        });
    }

    if parsed.model_control_file() == Some("cap") {
        let content = read_file_to_string(&resolved)?;
        let report = inspect_model_capabilities(&content);
        return check_report("model capabilities", report.is_ok(), || {
            format_model_capability_issues(report.issues())
        });
    }

    if parsed.model_control_file() == Some("effort") {
        let content = read_file_to_string(&resolved)?;
        return if ModelEffort::parse(&content).is_some() {
            print_line("ok")
        } else {
            Err(CliError::usage("invalid model control effort"))
        };
    }

    if parsed.model_control_file() == Some("fallback") {
        let content = read_file_to_string(&resolved)?;
        let (_fallback, report) = parse_model_fallback(&content);
        return check_report("model fallback", report.is_ok(), || {
            format_model_fallback_issues(report.issues())
        });
    }

    if file_check_model_driver(parsed, &resolved)? {
        return Ok(());
    }

    if let Some(kind) = parsed.agent_control_kind() {
        let content = read_file_to_string(&resolved)?;
        let report = inspect_agent_control(kind, &content);
        return check_report("agent control", report.is_ok(), || {
            format_agent_control_issues(report.issues())
        });
    }

    if let Some(kind) = parsed.session_index_kind() {
        if kind == SessionIndexKind::ByCwd
            && fs::symlink_metadata(&resolved)
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(CliError::usage(
                "invalid session index: by-cwd entry is a symlink",
            ));
        }
        let content = read_file_to_string(&resolved)?;
        let report = inspect_session_index(kind, &content);
        return check_report("session index", report.is_ok(), || {
            format_session_index_issues(report.issues())
        });
    }

    if let Some(kind) = parsed.session_control_kind() {
        let content = read_file_to_string(&resolved)?;
        let report = inspect_session_control(kind, &content);
        return check_report("session control", report.is_ok(), || {
            format_session_control_issues(report.issues())
        });
    }

    if file_check_jsonl_content(parsed, &resolved)? {
        return Ok(());
    }

    if parsed.is_context_pack() {
        let content = read_file_to_string(&resolved)?;
        let report = inspect_context_pack_json(&content);
        return check_report("context pack", report.is_ok(), || {
            format_context_pack_issues(report.issues())
        });
    }

    if shape == "ctx.session.dir" && parsed.is_session_instance() {
        let report = inspect_session_layout(&resolved);
        return check_report("session layout", report.is_ok(), || {
            format_session_layout_issues(report.issues())
        });
    }

    if let Some((class, name)) = parsed.executable_object() {
        let report = inspect_object_layout(root, class, &name);
        return check_report("object layout", report.is_ok(), || {
            format_object_layout_issues(report.issues())
        });
    }

    print_line(shape)
}

fn check_report(
    label: &str,
    is_ok: bool,
    format_issues: impl FnOnce() -> String,
) -> Result<(), CliError> {
    if is_ok {
        return print_line("ok");
    }
    Err(CliError::usage(format!(
        "invalid {label}: {}",
        format_issues()
    )))
}

fn file_check_policy_or_mount(parsed: AbiPathKind<'_>, resolved: &Path) -> Result<bool, CliError> {
    if parsed.control_file() == Some("policy") {
        let content = read_file_to_string(resolved)?;
        PolicyV0::parse(&content)
            .map_err(|error| CliError::usage(format!("invalid policy: {error:?}")))?;
        print_line("ok")?;
        return Ok(true);
    }

    if parsed.control_file() == Some("mount") {
        let content = read_file_to_string(resolved)?;
        MountTable::parse(&content)
            .map_err(|error| CliError::usage(format!("invalid mount: {error:?}")))?;
        print_line("ok")?;
        return Ok(true);
    }

    Ok(false)
}

fn file_check_jsonl_content(parsed: AbiPathKind<'_>, resolved: &Path) -> Result<bool, CliError> {
    if matches!(
        parsed,
        AbiPathKind::SessionFile {
            file: "messages.jsonl",
            ..
        }
    ) {
        let content = read_file_to_string(resolved)?;
        let report = inspect_message_stream_jsonl(&content);
        return check_report("message stream", report.is_ok(), || {
            format_message_stream_issues(report.issues())
        })
        .map(|()| true);
    }

    if matches!(
        parsed,
        AbiPathKind::SessionFile {
            file: "events.jsonl",
            ..
        }
    ) {
        let content = read_file_to_string(resolved)?;
        let report = inspect_event_stream_jsonl(&content);
        return check_report("event stream", report.is_ok(), || {
            format_event_stream_issues(report.issues())
        })
        .map(|()| true);
    }

    if let Some(kind) = parsed.context_jsonl_kind() {
        let content = read_file_to_string(resolved)?;
        let report = inspect_context_jsonl(kind, &content);
        return check_report("context jsonl", report.is_ok(), || {
            format_context_jsonl_issues(report.issues())
        })
        .map(|()| true);
    }

    Ok(false)
}

fn file_check_model_driver(parsed: AbiPathKind<'_>, resolved: &Path) -> Result<bool, CliError> {
    if parsed.model_control_file() != Some("driver") {
        return Ok(false);
    }

    let content = read_file_to_string(resolved)?;
    match parse_model_driver_routes(&content) {
        Ok(_) => {
            print_line("ok")?;
            Ok(true)
        }
        Err(error) => Err(CliError::usage(format!(
            "invalid model driver routes: {}",
            format_model_driver_route_error(&error)
        ))),
    }
}

fn format_model_fallback_issues(issues: &[ModelFallbackIssue]) -> String {
    issues
        .iter()
        .map(|issue| match *issue {
            ModelFallbackIssue::InvalidLine { line, ref value } => {
                format!("line {line}: invalid model {value}")
            }
            ModelFallbackIssue::DuplicateModel { line, ref value } => {
                format!("line {line}: duplicate model {value}")
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}
