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
    let model = args.models.first().map_or_else(
        || default_agent_process_model(&args.name).to_owned(),
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
    let agent_path = agent_object_path(root, &args.name);
    let control = agent_control_dir(root, &args.name);
    if agent_path.exists() || control.exists() {
        return Err(CliError::unavailable(format!(
            "agent already exists: {}",
            args.name
        )));
    }

    let agent_home = format!("/ctx/home/{uid}/agent/{}", args.name);
    let mount = agent_new_mount_control(&uid, &args.name, &args.mounts);
    let policy = agent_new_policy(&subject, &model, &args.tools);
    let system = format!(
        "\
You are CortexFS agent `{}`.
Use available `tsh` tools for implementation work. For clear coding requests, do not stop at a plan: inspect the workspace, make the smallest safe edit, run focused verification, and report exact files and commands.
Open-ended project iteration requests are clear coding requests. When the user says `迭代本项目`, `bootstrap`, `self-improve`, `improve this project`, or asks to make the project better without narrower target, do not ask what to do. Inspect project rules, git status, and relevant files; choose one small safe improvement; verify it; report evidence.
Ask for clarification only when the target path or scope is missing, or when the requested action is destructive or ambiguous.
",
        args.name
    );
    let root_control = format!("{agent_home}/root");
    let path = format!("/ctx/tool:/ctx/home/{uid}/tool");
    let overrides = vec![
        ("owner", uid.clone()),
        ("uid", uid.clone()),
        ("gid", uid.clone()),
        ("groups", uid.clone()),
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
        ("system.md", system),
        (
            "prompt.template.md",
            DEFAULT_AGENT_PROMPT_TEMPLATE.to_owned(),
        ),
        ("policy", policy),
        ("status", "idle".to_owned()),
        ("pid", String::new()),
        ("log", String::new()),
        ("meta.json", "{}".to_owned()),
    ];
    let override_refs = overrides
        .iter()
        .map(|entry| (entry.0, entry.1.as_str()))
        .collect::<Vec<_>>();
    install_executable_object_wrapper(
        root,
        ObjectClass::Agent,
        &args.name,
        "/bin/false",
        &override_refs,
    )
    .map_err(|error| CliError::unavailable(format!("cannot create agent: {}", error.errno())))?;
    write_agent_host_stub(&agent_path, &args.name)?;
    ensure_agent_home_skeleton(root, &uid, &args.name)?;
    print_line(&format!(
        "agent {} created model={}",
        terminal_safe_text(&args.name),
        terminal_safe_text(&model)
    ))?;
    Ok(ExitCode::SUCCESS)
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

pub(crate) fn write_agent_host_stub(path: &Path, name: &str) -> Result<(), CliError> {
    fs::write(path, agent_host_stub_script(name)).map_err(|error| {
        CliError::unavailable(format!("cannot write {}: {error}", path.display()))
    })?;
    let mut permissions = fs::metadata(path)
        .map_err(|error| CliError::unavailable(format!("cannot stat {}: {error}", path.display())))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .map_err(|error| CliError::unavailable(format!("cannot chmod {}: {error}", path.display())))
}

pub(crate) fn agent_host_stub_script(name: &str) -> String {
    let default_model = default_agent_process_model(name);
    format!(
        r#"#!/bin/sh
# CortexFS host-created agent stub. Runtime startup is still explicit.
source_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
ctx_root="${{CTX_ROOT:-/ctx}}"
run="${{CTX_RUN_ID:-r1}}"
input="$*"
if [ -z "$input" ]; then
  input="$(/usr/bin/cat)"
fi
model="$(/usr/bin/tr -d '\n' < "$source_root/agent/{name}.d/model" 2>/dev/null || true)"
if [ -z "$model" ]; then
  model="{default_model}"
fi
CTX_AGENT="{name}" \
CTX_AGENT_SYSTEM="$(/usr/bin/cat "$source_root/agent/{name}.d/system.md" 2>/dev/null || true)" \
CTX_AGENT_PROMPT_TEMPLATE="$(/usr/bin/cat "$source_root/agent/{name}.d/prompt.template.md" 2>/dev/null || true)" \
CTX_RUN_ID="$run" \
exec "$ctx_root/model/$model" "$input"
"#
    )
}

pub(crate) fn ensure_agent_home_skeleton(
    root: &Path,
    uid: &str,
    name: &str,
) -> Result<(), CliError> {
    let home = root.join("home").join(uid).join("agent").join(name);
    for dir in [
        home.join("root"),
        home.join("session").join("index").join("by-cwd"),
        home.join("session").join("index").join("by-hash"),
        home.join("session").join("index").join("by-uuid"),
        home.join("data"),
        home.join("cache"),
        home.join("log"),
    ] {
        fs::create_dir_all(&dir).map_err(|error| {
            CliError::unavailable(format!("cannot create {}: {error}", dir.display()))
        })?;
    }
    Ok(())
}
