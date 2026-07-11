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
    let socket = root.join("agent").join(format!("{}.sock", args.name));
    let home = ctx_home(root)?.join("agent").join(&args.name);
    let paths = HostAgentCreatePaths {
        agent: agent_path,
        control,
        socket,
        home,
    };
    require_host_agent_paths_absent(
        &args.name,
        [
            paths.agent.as_path(),
            paths.control.as_path(),
            paths.socket.as_path(),
            paths.home.as_path(),
        ],
    )?;

    let agent_home = format!("/ctx/home/{uid}/agent/{}", args.name);
    let mount = agent_new_mount_control(&uid, &args.name, &args.mounts);
    let policy = agent_new_policy(&subject, &model, &args.tools);
    let system = args.instructions.clone().unwrap_or_else(|| {
        format!(
            "\
You are CortexFS agent `{}`.
Use available `tsh` tools for implementation work. For clear coding requests, do not stop at a plan: inspect the workspace, make the smallest safe edit, run focused verification, and report exact files and commands.
Open-ended project iteration requests are clear coding requests. When the user says `迭代本项目`, `bootstrap`, `self-improve`, `improve this project`, or asks to make the project better without narrower target, do not ask what to do. Inspect project rules, git status, and relevant files; choose one small safe improvement; verify it; report evidence.
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
        ("meta.json", meta),
    ];
    let override_refs = overrides
        .iter()
        .map(|entry| (entry.0, entry.1.as_str()))
        .collect::<Vec<_>>();
    create_host_agent_files(root, args, &override_refs, &uid, &paths)?;
    print_line(&format!(
        "agent {} created model={}",
        terminal_safe_text(&args.name),
        terminal_safe_text(&model)
    ))?;
    Ok(ExitCode::SUCCESS)
}

struct HostAgentCreatePaths {
    agent: PathBuf,
    control: PathBuf,
    socket: PathBuf,
    home: PathBuf,
}

fn create_host_agent_files(
    root: &Path,
    args: &AgentNewArgs,
    overrides: &[(&str, &str)],
    uid: &str,
    paths: &HostAgentCreatePaths,
) -> Result<(), CliError> {
    create_agent_host_parent(&root.join("agent"))?;
    let home_parent = paths
        .home
        .parent()
        .ok_or_else(|| CliError::unavailable("agent home path has no parent"))?;
    create_agent_host_parent(home_parent)?;
    let mut created = HostAgentCreateState::default();
    let result = (|| {
        cortexfs::support::plain::create_plain_dir_exclusive(&paths.control, 0o755).map_err(
            |error| {
                CliError::unavailable(format!(
                    "cannot create {}: {error}",
                    paths.control.display()
                ))
            },
        )?;
        created.control = true;
        install_executable_object_wrapper(
            root,
            ObjectClass::Agent,
            &args.name,
            "/bin/false",
            overrides,
        )
        .map_err(|error| {
            CliError::unavailable(format!("cannot create agent: {}", error.errno()))
        })?;
        write_agent_host_stub(&paths.agent, &args.name)?;
        let socket_created =
            cortexfs::support::plain::ensure_socket_placeholder(&paths.socket, 0o777).map_err(
                |error| {
                    CliError::unavailable(format!(
                        "cannot create agent socket {}: {error}",
                        paths.socket.display()
                    ))
                },
            )?;
        if !socket_created {
            return Err(CliError::unavailable(format!(
                "agent already exists: {}",
                args.name
            )));
        }
        created.socket = true;
        require_plain_socket_inode(&paths.socket)?;
        cortexfs::support::plain::create_plain_dir_exclusive(&paths.home, 0o755).map_err(
            |error| {
                CliError::unavailable(format!("cannot create {}: {error}", paths.home.display()))
            },
        )?;
        created.home = true;
        ensure_agent_home_skeleton(root, uid, &args.name)
    })();
    if let Err(error) = result {
        rollback_host_agent_create(paths, created);
        return Err(error);
    }
    Ok(())
}

#[derive(Clone, Copy, Default)]
struct HostAgentCreateState {
    control: bool,
    socket: bool,
    home: bool,
}

fn require_host_agent_paths_absent<'a>(
    name: &str,
    paths: impl IntoIterator<Item = &'a Path>,
) -> Result<(), CliError> {
    for path in paths {
        match fs::symlink_metadata(path) {
            Ok(_metadata) => {
                return Err(CliError::unavailable(format!(
                    "agent already exists: {name}"
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(CliError::unavailable(format!(
                    "cannot inspect {}: {error}",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

fn create_agent_host_parent(path: &Path) -> Result<(), CliError> {
    create_plain_directory(
        path,
        0o755,
        "agent parent is not a plain directory",
        "agent parent contains a non-directory entry",
        "invalid agent parent directory name",
    )
    .map_err(|error| CliError::unavailable(format!("cannot create {}: {error}", path.display())))
}

fn require_plain_socket_inode(path: &Path) -> Result<(), CliError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        CliError::unavailable(format!(
            "cannot inspect agent socket {}: {error}",
            path.display()
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_socket() {
        return Err(CliError::unavailable(format!(
            "agent socket is not a plain socket inode: {}",
            path.display()
        )));
    }
    Ok(())
}

fn rollback_host_agent_create(paths: &HostAgentCreatePaths, created: HostAgentCreateState) {
    if created.home {
        let _ignored = fs::remove_dir_all(&paths.home);
    }
    if created.socket {
        let _ignored = fs::remove_file(&paths.socket);
    }
    if created.control {
        let _ignored = fs::remove_file(&paths.agent);
        let _ignored = fs::remove_dir_all(&paths.control);
    }
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
    atomic_replace_text_with_mode(path, &agent_host_stub_script(name), 0o755)
        .map_err(|error| CliError::unavailable(format!("cannot write {}: {error}", path.display())))
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
