use crate::*;

pub(crate) fn doctor(root: &Path) -> Result<(), CliError> {
    let mut ok = true;
    if doctor_is_dir(root) {
        print_line(&doctor_root_line("ok", root))?;
    } else {
        ok = false;
        print_line(&doctor_root_line("missing", root))?;
    }

    for entry in ROOT_ENTRIES {
        let path = root.join(entry);
        let entry_shape_matches = if entry == &"status" {
            doctor_is_file(&path)
        } else {
            doctor_is_dir(&path)
        };
        if entry_shape_matches {
            print_line(&format!("ok {entry}"))?;
        } else {
            ok = false;
            print_line(&format!("missing {entry}"))?;
        }
    }

    if doctor_is_dir(root) {
        for entry in read_dir_names(root)? {
            if !cortexfs::is_root_entry(&entry) {
                ok = false;
                print_line(&doctor_unexpected_entry_line(&entry))?;
            }
        }
    }

    ok &= doctor_objects(root)?;
    ok &= doctor_sessions(root)?;
    ok &= doctor_shared_queues(root)?;

    if ok {
        Ok(())
    } else {
        Err(CliError::unavailable("doctor found ABI problems"))
    }
}

pub(crate) fn doctor_objects(root: &Path) -> Result<bool, CliError> {
    let mut ok = true;
    for class in [ObjectClass::Model, ObjectClass::Agent, ObjectClass::Tool] {
        let dir = root.join(class.as_str());
        if !doctor_is_dir(&dir) {
            continue;
        }
        let names = object_names_for_doctor(&dir, class)?;
        for name in names {
            ok &= doctor_object(root, class, &name)?;
        }
    }
    Ok(ok)
}

pub(crate) fn object_names_for_doctor(
    dir: &Path,
    class: ObjectClass,
) -> Result<Vec<String>, CliError> {
    if class != ObjectClass::Model {
        return Ok(read_dir_names(dir)?
            .into_iter()
            .filter(|name| is_object_name(name))
            .collect());
    }

    let mut names = Vec::new();
    for provider in read_dir_names(dir)? {
        if !is_object_name(&provider) {
            continue;
        }
        let provider_dir = dir.join(&provider);
        if !doctor_is_dir(&provider_dir) {
            continue;
        }
        for entry in read_dir_names(&provider_dir)? {
            let model = entry
                .strip_suffix(".d")
                .or_else(|| entry.strip_suffix(".sock"))
                .unwrap_or(&entry);
            let name = format!("{provider}/{model}");
            if is_model_name(&name) && !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names.sort();
    Ok(names)
}

pub(crate) fn doctor_object(root: &Path, class: ObjectClass, name: &str) -> Result<bool, CliError> {
    let report = inspect_object_layout(root, class, name);
    if report.is_ok() {
        print_line(&doctor_object_line("ok", class, name))?;
        Ok(true)
    } else {
        print_line(&doctor_invalid_object_line(
            class,
            name,
            &format_object_layout_issues(report.issues()),
        ))?;
        Ok(false)
    }
}

pub(crate) fn doctor_sessions(root: &Path) -> Result<bool, CliError> {
    let mut ok = true;
    for (label, session_dir) in discover_session_dirs(root)? {
        let report = inspect_session_layout(&session_dir);
        ok &= print_doctor_report(
            &label,
            report.is_ok(),
            &format_session_layout_issues(report.issues()),
        )?;
    }
    Ok(ok)
}

pub(crate) fn doctor_shared_queues(root: &Path) -> Result<bool, CliError> {
    let mut ok = true;
    let shared = root.join("shared");
    if !doctor_is_dir(&shared) {
        return Ok(ok);
    }
    for space in read_dir_names(&shared)? {
        if !is_object_name(&space) {
            continue;
        }
        let queue = shared.join(&space).join("queue");
        if !doctor_exists(&queue) {
            continue;
        }
        let report = inspect_shared_queue_layout(&queue);
        ok &= print_doctor_report(
            &format!("shared/{space}/queue"),
            report.is_ok(),
            &format_shared_queue_layout_issues(report.issues()),
        )?;
    }
    Ok(ok)
}

pub(crate) fn print_doctor_report(label: &str, ok: bool, issues: &str) -> Result<bool, CliError> {
    if ok {
        print_line(&doctor_report_line("ok", label, None))?;
    } else {
        print_line(&doctor_report_line("invalid", label, Some(issues)))?;
    }
    Ok(ok)
}

pub(crate) fn doctor_root_line(status: &str, root: &Path) -> String {
    format!(
        "{} root {}",
        status,
        terminal_safe_text(&root.display().to_string())
    )
}

pub(crate) fn doctor_unexpected_entry_line(entry: &str) -> String {
    format!("unexpected {}", terminal_safe_text(entry))
}

pub(crate) fn doctor_object_line(status: &str, class: ObjectClass, name: &str) -> String {
    format!("{} {}/{}", status, class.as_str(), terminal_safe_text(name))
}

pub(crate) fn doctor_invalid_object_line(class: ObjectClass, name: &str, issues: &str) -> String {
    format!(
        "invalid {}/{}: {}",
        class.as_str(),
        terminal_safe_text(name),
        terminal_safe_text(issues)
    )
}

pub(crate) fn doctor_report_line(status: &str, label: &str, issues: Option<&str>) -> String {
    let label = terminal_safe_text(label);
    issues.map_or_else(
        || format!("{status} {label}"),
        |issues| format!("{} {}: {}", status, label, terminal_safe_text(issues)),
    )
}

pub(crate) fn discover_session_dirs(root: &Path) -> Result<Vec<(String, PathBuf)>, CliError> {
    let mut sessions = Vec::new();
    collect_home_sessions(root, &mut sessions)?;
    collect_shared_sessions(root, &mut sessions)?;
    sessions.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(sessions)
}

pub(crate) fn collect_home_sessions(
    root: &Path,
    sessions: &mut Vec<(String, PathBuf)>,
) -> Result<(), CliError> {
    let home = root.join("home");
    if !doctor_is_dir(&home) {
        return Ok(());
    }
    for uid in read_dir_names(&home)? {
        collect_space_sessions(&home.join(&uid), &format!("home/{uid}"), sessions)?;
    }
    Ok(())
}

pub(crate) fn collect_shared_sessions(
    root: &Path,
    sessions: &mut Vec<(String, PathBuf)>,
) -> Result<(), CliError> {
    let shared = root.join("shared");
    if !doctor_is_dir(&shared) {
        return Ok(());
    }
    for space in read_dir_names(&shared)? {
        if !is_object_name(&space) {
            continue;
        }
        collect_space_sessions(&shared.join(&space), &format!("shared/{space}"), sessions)?;
    }
    Ok(())
}

pub(crate) fn collect_space_sessions(
    root: &Path,
    label_prefix: &str,
    sessions: &mut Vec<(String, PathBuf)>,
) -> Result<(), CliError> {
    collect_agent_sessions(
        &root.join("agent"),
        &format!("{label_prefix}/agent"),
        sessions,
    )?;
    collect_model_sessions(
        &root.join("model"),
        &format!("{label_prefix}/model"),
        sessions,
    )
}

pub(crate) fn collect_agent_sessions(
    agent_root: &Path,
    label_prefix: &str,
    sessions: &mut Vec<(String, PathBuf)>,
) -> Result<(), CliError> {
    if !doctor_is_dir(agent_root) {
        return Ok(());
    }
    for agent in read_dir_names(agent_root)? {
        if !is_object_name(&agent) {
            continue;
        }
        collect_named_sessions(
            &agent_root.join(&agent).join("session"),
            &format!("{label_prefix}/{agent}/session"),
            sessions,
        )?;
    }
    Ok(())
}

pub(crate) fn collect_model_sessions(
    model_root: &Path,
    label_prefix: &str,
    sessions: &mut Vec<(String, PathBuf)>,
) -> Result<(), CliError> {
    if !doctor_is_dir(model_root) {
        return Ok(());
    }
    for provider in read_dir_names(model_root)? {
        if !is_object_name(&provider) {
            continue;
        }
        let provider_root = model_root.join(&provider);
        if !doctor_is_dir(&provider_root) {
            continue;
        }
        for model_dir in read_dir_names(&provider_root)? {
            let Some(model) = model_dir.strip_suffix(".d") else {
                continue;
            };
            let model_name = format!("{provider}/{model}");
            if !is_model_name(&model_name) {
                continue;
            }
            collect_named_sessions(
                &provider_root.join(&model_dir).join("session"),
                &format!("{label_prefix}/{provider}/{model_dir}/session"),
                sessions,
            )?;
        }
    }
    Ok(())
}

pub(crate) fn collect_named_sessions(
    session_root: &Path,
    label_prefix: &str,
    sessions: &mut Vec<(String, PathBuf)>,
) -> Result<(), CliError> {
    if !doctor_is_dir(session_root) {
        return Ok(());
    }
    for session in read_dir_names(session_root)? {
        if session == "index" || !is_object_name(&session) {
            continue;
        }
        sessions.push((
            format!("{label_prefix}/{session}"),
            session_root.join(session),
        ));
    }
    Ok(())
}

pub(crate) fn doctor_is_file(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.is_file())
}

pub(crate) fn doctor_is_dir(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.is_dir())
}

pub(crate) fn doctor_exists(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}
