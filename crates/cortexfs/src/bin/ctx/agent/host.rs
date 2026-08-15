use crate::*;

pub(crate) fn agent_new_host_fallback(
    root: &Path,
    args: &AgentNewArgs,
) -> Result<ExitCode, CliError> {
    agent_new_request_json(args)?;
    if args.models.len() > 1 {
        return Err(CliError::usage(
            "host agent creation fallback accepts at most one --model",
        ));
    }
    let uid = current_uid_text().map_err(CliError::unavailable)?;
    let groups = current_supplementary_groups_control()?;
    let model = args.models.first().map_or_else(
        || default_agent_model_for_name(&args.name).to_owned(),
        Clone::clone,
    );
    let subject = agent_new_policy_subject(args);
    let label = args.label.as_deref().map_or_else(
        || format!("user_u:agent_r:{subject}:s0"),
        |label| {
            if label.contains(':') {
                label.to_owned()
            } else {
                format!("user_u:agent_r:{label}:s0")
            }
        },
    );
    let life = if args.temporary { "temp" } else { "owned" };
    let parent = args
        .parent
        .clone()
        .unwrap_or_else(|| "agent:architect".to_owned());
    let agent_home = format!("/ctx/home/{uid}/agent/{}", args.name);
    let mount = agent_new_mount_control(&uid, &args.name, &args.mounts);
    let policy = agent_new_policy(&subject, &model, &args.tools);
    let permissions = cortexfs::AgentPermissions::for_tools(args.tools.iter().map(String::as_str));
    let system = args.instructions.clone().unwrap_or_else(|| {
        format!(
            "\
You are CortexFS agent `{}`.
Use available `tsh` tools for implementation work. For clear coding requests, do not stop at a plan: inspect the workspace, make the smallest safe edit, run focused verification, and report exact files and commands.
Open-ended project iteration requests are clear coding requests. When the user says `iterate this project`, `bootstrap`, `self-improve`, `improve this project`, or asks to make the project better without a narrower target, do not ask what to do. Inspect project rules, git status, and relevant files; choose one small safe improvement; verify it; report evidence.
Ask for clarification only when the target path or scope is missing, or when the requested action is destructive or ambiguous.
",
            args.name
        )
    });
    let meta = agent_profile_meta_json(args.description.as_deref());
    let root_control = format!("{agent_home}/root");
    let path = format!("/ctx/tool:/ctx/home/{uid}/tool");
    let overrides = vec![
        ("owner", uid.clone()),
        ("uid", uid.clone()),
        ("gid", uid.clone()),
        ("groups", groups),
        ("perm", permissions.control().trim_end().to_owned()),
        ("label", label),
        ("iso", "shared".to_owned()),
        ("parent", parent),
        ("life", life.to_owned()),
        ("root", root_control),
        ("cwd", "/workspace".to_owned()),
        ("env", "CTX_ROOT=/ctx".to_owned()),
        ("path", path),
        ("mount", mount),
        ("model", model.clone()),
        ("tools", args.tools.join("\n")),
        ("system.md", system),
        (
            "prompt.template.md",
            DEFAULT_AGENT_PROMPT_TEMPLATE.to_owned(),
        ),
        ("policy", policy),
        ("status", "idle".to_owned()),
        ("pid", String::new()),
        ("log", String::new()),
        ("meta.json", meta),
    ];
    let override_refs = overrides
        .iter()
        .map(|entry| (entry.0, entry.1.as_str()))
        .collect::<Vec<_>>();
    let executable = cortexfs::executable_wrapper_script(
        ObjectClass::Agent,
        &args.name,
        cortexfs::CORTEXFS_OBJECT_RUNNER,
    );
    cortexfs::agent::create::create_agent_files(
        root,
        &uid,
        &args.name,
        &executable,
        &override_refs,
    )
    .map_err(|error| match error {
        cortexfs::agent::create::AgentCreateError::InvalidInput => {
            CliError::usage(format!("invalid agent create request: {}", args.name))
        }
        cortexfs::agent::create::AgentCreateError::AlreadyExists => {
            CliError::unavailable(format!("agent already exists: {}", args.name))
        }
        cortexfs::agent::create::AgentCreateError::CannotCreate => {
            CliError::unavailable(format!("cannot create agent: {}", args.name))
        }
        cortexfs::agent::create::AgentCreateError::RollbackConflict(conflict) => {
            CliError::unavailable(format!(
                "agent create rollback conflict: {} {}",
                args.name,
                cortexfs::agent::create::format_agent_rollback_conflict(&conflict)
            ))
        }
    })?;
    print_line(&format!(
        "agent {} created model={}",
        terminal_safe_text(&args.name),
        terminal_safe_text(&model)
    ))?;
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn current_supplementary_groups_control() -> Result<String, CliError> {
    let mut groups = nix::unistd::getgroups()
        .map_err(|error| {
            CliError::unavailable(format!("cannot read supplementary groups: {error}"))
        })?
        .into_iter()
        .map(nix::unistd::Gid::as_raw)
        .collect::<Vec<_>>();
    groups.sort_unstable();
    groups.dedup();
    Ok(groups
        .into_iter()
        .map(|gid| gid.to_string())
        .collect::<Vec<_>>()
        .join("\n"))
}

pub(crate) fn agent_new_policy_subject(args: &AgentNewArgs) -> String {
    args.label.as_deref().map_or_else(
        || format!("{}_t", args.name),
        |label| {
            label
                .split(':')
                .nth(2)
                .filter(|value| is_object_name(value))
                .map_or_else(|| label.to_owned(), ToOwned::to_owned)
        },
    )
}

pub(crate) fn agent_new_mount_control(uid: &str, name: &str, mounts: &[AgentMount]) -> String {
    let mut lines = vec![
        "/ctx\t/ctx\tro\trbind,nosuid,nodev".to_owned(),
        format!("/ctx/home/{uid}/agent/{name}\t/home/agent\trw\trbind,nosuid,nodev"),
    ];
    for mount in mounts {
        lines.push(format!(
            "{}\t{}\t{}\trbind,nosuid,nodev",
            mount.source, mount.target, mount.mode
        ));
    }
    lines.join("\n")
}

pub(crate) fn agent_new_policy(subject: &str, model: &str, tools: &[String]) -> String {
    let mut policy = format!(
        "allow {subject} model:{model} use\n\
         allow {subject} tool:tsh execute\n\
         allow {subject} network:default connect"
    );
    for tool in tools {
        let _ignored = std::fmt::Write::write_fmt(
            &mut policy,
            format_args!("\nallow {subject} tool:{tool} execute"),
        );
    }
    policy
}
