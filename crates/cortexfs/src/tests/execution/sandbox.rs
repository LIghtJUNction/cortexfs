fn assert_provider_egress_args(args: &[String], host_dir: &str) {
    assert!(args.contains(&"--unshare-net".to_owned()));
    assert!(contains_arg_triplet(
        args,
        "--ro-bind",
        host_dir,
        crate::runtime::egress::PROVIDER_EGRESS_SANDBOX_PATH
    ));
    assert!(contains_arg_triplet(
        args,
        "--setenv",
        crate::runtime::egress::PROVIDER_EGRESS_DIR_ENV,
        crate::runtime::egress::PROVIDER_EGRESS_SANDBOX_PATH
    ));
}

fn assert_control_environment(args: &[String]) {
    assert!(contains_arg_triplet(
        args,
        "--setenv",
        "CTX_CONTROL_SOCKET",
        "/run/cortexfs/control.sock"
    ));
    let control_keys = args
        .windows(3)
        .filter_map(|entry| match *entry {
            [ref flag, ref key, _] if flag == "--setenv" && key.starts_with("CTX_CONTROL_") => {
                Some(key.as_str())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(control_keys, ["CTX_CONTROL_SOCKET"]);
}

fn assert_agent_sandbox_args(args: &[String], root: &Path, session_root: &Path) {
    let agent_home = session_root.parent().unwrap_or(session_root);
    assert_eq!(args.first().map(String::as_str), Some("--clearenv"));
    assert!(args.contains(&"--unshare-net".to_owned()));
    assert!(args.contains(&"--unshare-pid".to_owned()));
    assert!(contains_arg_pair(args, "--tmpfs", "/tmp"));
    assert!(contains_arg_pair(args, "--ro-bind", "/usr"));
    assert!(contains_arg_pair(args, "--dir", "/workspace"));
    assert!(contains_arg_pair(args, "--perms", "0755"));
    assert!(contains_arg_triplet(
        args,
        "--ro-bind-data",
        "9",
        "/run/cortexfs/agent-executable"
    ));
    assert!(contains_arg_triplet(
        args,
        "--bind-fd",
        "10",
        &agent_home.display().to_string()
    ));
    assert!(contains_arg_triplet(args, "--bind-fd", "11", "/home/agent"));
    assert!(!args.iter().any(|arg| arg == "- user: hi"));
    assert!(!args.iter().any(|arg| arg == "workspace context"));
    assert!(contains_arg_triplet(
        args,
        "--setenv",
        "CTX_PROVIDER_CONFIG_DIR",
        &root.join("shared/providers.d").display().to_string()
    ));
    assert_provider_egress_args(args, "/run/cortexfs/egress-run-1");
    assert!(!args.iter().any(|arg| arg == "/host/providers.d"));
    for secret in [
        "CTX_PROVIDER_SECRET_FD",
        "CTX_PROVIDER_SECRET_PATH",
        "CTX_PROVIDER_SECRET_PROVIDER",
        "CTX_PROVIDER_SECRET_SLOT",
        "/run/user/1000/cortexfs/credentials/coder-default",
    ] {
        assert!(!args.iter().any(|arg| arg == secret));
    }
    assert!(contains_arg_pair(args, "--chdir", "/workspace"));
    assert!(!contains_arg_triplet(args, "--bind", "/repo", "/workspace"));
    assert!(contains_arg_triplet(
        args,
        "--ro-bind",
        root.to_str().unwrap_or_default(),
        "/ctx"
    ));
    assert!(contains_arg_triplet(
        args,
        "--ro-bind",
        root.to_str().unwrap_or_default(),
        root.to_str().unwrap_or_default()
    ));
    assert_eq!(
        args.get(args.len().saturating_sub(2)),
        Some(&"/run/cortexfs/agent-executable".to_owned())
    );
    assert_eq!(args.last().map(String::as_str), Some("hi"));
}

#[test]
fn agent_executable_socket_direct_does_not_inherit_provider_secrets() {
    let root = reference_tree("agent-direct-secrets");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        r#"#!/bin/sh
IFS= read -r envelope || exit 2
if [ -n "$CTX_PROVIDER_SECRET_VALUE$CTX_PROVIDER_SECRET_PROVIDER$CTX_PROVIDER_SECRET_SLOT$CTX_PROVIDER_SECRET_FD$CTX_PROVIDER_SECRET_PATH" ]; then
  printf '{"type":"delta","run":"%s","text":"leaked:%s:%s:%s:%s:%s"}\n' "$CTX_RUN_ID" "$CTX_PROVIDER_SECRET_VALUE" "$CTX_PROVIDER_SECRET_PROVIDER" "$CTX_PROVIDER_SECRET_SLOT" "$CTX_PROVIDER_SECRET_FD" "$CTX_PROVIDER_SECRET_PATH"
  exit 0
fi
printf '{"type":"delta","run":"%s","text":"secret-not-inherited"}\n' "$CTX_RUN_ID"
"#,
    );
    set_file_mode(&agent_executable, 0o755);
    let mut env = view.env().to_vec();
    env.extend([
        (
            "CTX_PROVIDER_SECRET_VALUE".to_owned(),
            "value-canary".to_owned(),
        ),
        (
            "CTX_PROVIDER_SECRET_PROVIDER".to_owned(),
            "provider-canary".to_owned(),
        ),
        (
            "CTX_PROVIDER_SECRET_SLOT".to_owned(),
            "slot-canary".to_owned(),
        ),
        ("CTX_PROVIDER_SECRET_FD".to_owned(), "42".to_owned()),
        (
            "CTX_PROVIDER_SECRET_PATH".to_owned(),
            "/secret/path-canary".to_owned(),
        ),
    ]);

    let pair = UnixStream::pair();
    let (mut client, mut socket) = ok!(pair);
    assert!(
        client
            .write_all(
                br#"{"op":"send","id":"msg-1","session":"default","input":"hi"}
"#,
            )
            .is_ok()
    );
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let outcome = serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        AgentExecutableSocketRuntime {
            ctx_root: &root,
            source_root: &root,
            identity: view.identity(),
            env: &env,
            session_root: &session_root,
            default_cwd: "/work",
            model: Some("debug/echo"),
            network_allowed: false,
            agent_name: "coder",
            agent_executable: &agent_executable,
            execution: AgentExecutableSocketExecution::Direct,
        },
    );
    let outcome = ok!(outcome);
    assert!(outcome.jsonl().contains("secret-not-inherited"));
    assert!(!outcome.jsonl().contains("leaked:"));
    for canary in [
        "value-canary",
        "provider-canary",
        "slot-canary",
        "/secret/path-canary",
    ] {
        assert!(!outcome.jsonl().contains(canary));
    }
}

#[test]
fn agent_executable_socket_bwrap_args_apply_agent_sandbox() {
    let root = reference_tree("agent-executable-socket-runtime-bwrap-args");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent").join("coder");
    let mut env = view.env().to_vec();
    env.push((
        "CTX_PROVIDER_CONFIG_DIR".to_owned(),
        "/host/providers.d".to_owned(),
    ));
    env.push(("CTX_PROVIDER_SECRET_FD".to_owned(), "9".to_owned()));
    env.push((
        "CTX_PROVIDER_SECRET_PATH".to_owned(),
        "/run/user/1000/cortexfs/credentials/coder-default".to_owned(),
    ));
    env.push((
        "CTX_PROVIDER_SECRET_PROVIDER".to_owned(),
        "openai".to_owned(),
    ));
    env.push(("CTX_PROVIDER_SECRET_SLOT".to_owned(), "default".to_owned()));
    let secret_value = "provider-secret".to_owned();
    env.push(("CTX_PROVIDER_SECRET_VALUE".to_owned(), secret_value));
    let runtime = AgentExecutableSocketRuntime {
        ctx_root: &root,
        source_root: &root,
        identity: view.identity(),
        env: &env,
        session_root: &session_root,
        default_cwd: "/workspace",
        model: Some("debug/echo"),
        network_allowed: false,
        agent_name: "coder",
        agent_executable: &agent_executable,
        execution: AgentExecutableSocketExecution::Bwrap {
            program: Path::new("/usr/bin/bwrap"),
            mount_table: view.mount_table(),
            control_dir: None,
        },
    };
    let environment = [("CTX_AGENT".to_owned(), "coder".to_owned())];
    let control_environment = [(
        "CTX_CONTROL_SOCKET".to_owned(),
        "/run/cortexfs/control.sock".to_owned(),
    )];
    let args = agent_executable_socket_bwrap_args(&BwrapAgentExecutableArgs {
        runtime,
        mount_table: view.mount_table(),
        cwd: "/workspace",
        debug: None,
        input: "hi",
        agent_executable_fd: 9,
        agent_home_source_fd: 10,
        agent_home_sandbox_fd: 11,
        agent_home: session_root.parent().unwrap_or(&session_root),
        environment: &environment,
        control_socket: Some(Path::new("/run/cortexfs/control/source.sock")),
        control_environment: Some(&control_environment),
        control_gate: None,
        provider_egress: Some(Path::new("/run/cortexfs/egress-run-1")),
    });
    assert_agent_sandbox_args(&args, &root, &session_root);
    assert!(contains_arg_triplet(
        &args,
        "--setenv",
        "CTX_AGENT",
        "coder"
    ));
    assert_control_environment(&args);
    let opened = ok!(open_agent_executable_no_follow(&agent_executable));
    let request = AgentExecutableRunRequest {
        run_id: "run-1",
        cancellation_id: "cancel-1",
        session: "default",
        cwd: Some("/workspace"),
        input: "hi",
        history_messages: "",
        tool_context: "",
        debug: None,
    };
    let (command, agent_executable_fd) = ok!(agent_executable_socket_command(
        runtime,
        &opened,
        request,
        0,
        None,
        Some(Path::new("/run/cortexfs/egress-run-1")),
    ));
    drop(agent_executable_fd);
    let command_env: Vec<_> = command
        .get_envs()
        .filter_map(|(name, _value)| name.to_str())
        .collect();
    for secret_name in [
        "CTX_PROVIDER_SECRET_FD",
        "CTX_PROVIDER_SECRET_PATH",
        "CTX_PROVIDER_SECRET_PROVIDER",
        "CTX_PROVIDER_SECRET_SLOT",
        "CTX_PROVIDER_SECRET_VALUE",
    ] {
        assert!(!command_env.contains(&secret_name));
    }
    assert!(
        !command
            .get_args()
            .filter_map(|arg| arg.to_str())
            .any(|arg| arg == "provider-secret")
    );
}

#[test]
fn agent_executable_socket_bwrap_executes_opened_inode_after_path_replacement() {
    let mut prepared = ok!(prepared_bwrap_command(
        "agent-bwrap-opened-inode",
        "#!/bin/sh\nprintf A\n",
        &[],
    ));
    let replacement = prepared.root.join("agent/replacement");
    write_text_file(&replacement, "#!/bin/sh\nprintf B\n");
    set_file_mode(&replacement, 0o755);
    assert!(fs::rename(replacement, &prepared.agent_executable).is_ok());

    let output = ok!(prepared.output());
    assert!(output.status.success(), "bwrap failed: {output:?}");
    assert_eq!(output.stdout, b"A");
}

#[test]
fn agent_executable_socket_bwrap_does_not_inherit_provider_secret_env() {
    let initial_env = [(
        "CTX_PROVIDER_SECRET_VALUE".to_owned(),
        "provider-secret".to_owned(),
    )];
    let mut prepared = ok!(prepared_bwrap_command(
        "agent-bwrap-provider-secret-env",
        "#!/bin/sh\nprintf %s \"${CTX_PROVIDER_SECRET_VALUE:-secret-not-inherited}\"\n",
        &initial_env,
    ));
    let output = ok!(prepared.output());
    assert!(output.status.success(), "bwrap failed: {output:?}");
    assert_eq!(output.stdout, b"secret-not-inherited");
}

struct PreparedBwrapCommand {
    root: TestDir,
    agent_executable: PathBuf,
    command: std::process::Command,
    inherited_fds: Option<Vec<crate::runtime::socket::InheritedFd>>,
}

impl PreparedBwrapCommand {
    fn output(&mut self) -> std::io::Result<std::process::Output> {
        let inherited_fds = &self.inherited_fds;
        let output = self.command.output();
        let _ = inherited_fds;
        output
    }
}

fn prepared_bwrap_command(
    case: &str,
    script: &str,
    initial_env: &[(String, String)],
) -> Result<PreparedBwrapCommand, std::io::Error> {
    let root = reference_tree(case);
    let session_root = agent_session_root(&root, "coder");
    let view = derive_agent_runtime_view(&root, "coder")
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let agent_executable = root.join("agent/coder");
    write_text_file(&agent_executable, script);
    set_file_mode(&agent_executable, 0o755);
    let opened = open_agent_executable_no_follow(&agent_executable)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let mut env = view.env().to_vec();
    env.extend_from_slice(initial_env);
    let mut runtime = direct_agent_runtime(&root, &view, &session_root, &agent_executable);
    runtime.default_cwd = "/";
    runtime.env = &env;
    runtime.execution = AgentExecutableSocketExecution::Bwrap {
        program: Path::new("/usr/bin/bwrap"),
        mount_table: view.mount_table(),
        control_dir: None,
    };
    let request = AgentExecutableRunRequest {
        run_id: "run-1",
        cancellation_id: "run-1",
        session: "default",
        cwd: Some("/"),
        input: "hi",
        history_messages: "",
        tool_context: "",
        debug: None,
    };
    let (command, inherited_fds) =
        agent_executable_socket_command(runtime, &opened, request, 0, None, None)
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    Ok(PreparedBwrapCommand {
        root,
        agent_executable,
        command,
        inherited_fds,
    })
}

#[test]
fn provider_egress_is_borrowed_across_agent_steps_and_cleans_up_after_run() {
    let root = reference_tree("agent-bwrap-egress-spawn-failure");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent/coder");
    write_text_file(&agent_executable, "#!/bin/sh\nexit 0\n");
    set_file_mode(&agent_executable, 0o755);
    let model_control = root.join("model/fixture/chat.d");
    assert!(fs::create_dir_all(&model_control).is_ok());
    write_text_file(
        &model_control.join("default"),
        "base_url=http://127.0.0.1:9/v1\n",
    );
    let control_dir = root.join("runtime");
    assert!(fs::create_dir_all(&control_dir).is_ok());
    let runtime = AgentExecutableSocketRuntime {
        ctx_root: &root,
        source_root: &root,
        identity: view.identity(),
        env: view.env(),
        session_root: &session_root,
        default_cwd: "/workspace",
        model: Some("fixture/chat"),
        network_allowed: false,
        agent_name: "coder",
        agent_executable: &agent_executable,
        execution: AgentExecutableSocketExecution::Bwrap {
            program: Path::new("/definitely/missing/bwrap"),
            mount_table: view.mount_table(),
            control_dir: Some(&control_dir),
        },
    };
    let provider_egress = ok!(crate::runtime::egress::ProviderEgress::create(
        &control_dir,
        &root,
        "fixture/chat",
        view.env(),
        view.identity().uid(),
        view.identity().gid(),
        "run-1",
    ));
    let host_dir = provider_egress.host_dir().to_path_buf();
    let (client, mut socket) = ok!(UnixStream::pair());
    assert!(client.shutdown(Shutdown::Write).is_ok());
    let envelope = agent_envelope("run-1");

    let result = crate::runtime::socket::exec::run_agent_executable_streaming(
        &mut socket,
        runtime,
        AgentExecutableRunRequest {
            run_id: "run-1",
            cancellation_id: "run-1",
            session: "default",
            cwd: None,
            input: "hi",
            history_messages: "",
            tool_context: "",
            debug: None,
        },
        &envelope,
        0,
        None,
        Some(&provider_egress),
    );

    assert_eq!(result, Err(SocketRuntimeError::CannotRunAgent));
    assert!(host_dir.exists());
    let (client, mut socket) = ok!(UnixStream::pair());
    assert!(client.shutdown(Shutdown::Write).is_ok());
    let result = crate::runtime::socket::exec::run_agent_executable_streaming(
        &mut socket,
        runtime,
        AgentExecutableRunRequest {
            run_id: "run-1",
            cancellation_id: "run-1",
            session: "default",
            cwd: None,
            input: "hi",
            history_messages: "",
            tool_context: "",
            debug: None,
        },
        &envelope,
        1,
        None,
        Some(&provider_egress),
    );
    assert_eq!(result, Err(SocketRuntimeError::CannotRunAgent));
    assert_eq!(provider_egress.host_dir(), host_dir);
    drop(provider_egress);
    assert!(!control_dir.join("egress-run-1").exists());
}

#[test]
fn debug_alias_does_not_create_provider_egress() {
    let root = reference_tree("agent-bwrap-debug-no-egress");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent/coder");
    write_text_file(&agent_executable, "#!/bin/sh\nexit 0\n");
    set_file_mode(&agent_executable, 0o755);
    let control_dir = root.join("runtime");
    assert!(fs::create_dir_all(&control_dir).is_ok());
    let (client, mut socket) = ok!(UnixStream::pair());
    assert!(client.shutdown(Shutdown::Write).is_ok());
    let envelope = agent_envelope("run-1");

    let result = crate::runtime::socket::exec::run_agent_executable_streaming(
        &mut socket,
        AgentExecutableSocketRuntime {
            ctx_root: &root,
            source_root: &root,
            identity: view.identity(),
            env: view.env(),
            session_root: &session_root,
            default_cwd: "/workspace",
            model: Some("main"),
            network_allowed: false,
            agent_name: "coder",
            agent_executable: &agent_executable,
            execution: AgentExecutableSocketExecution::Bwrap {
                program: Path::new("/definitely/missing/bwrap"),
                mount_table: view.mount_table(),
                control_dir: Some(&control_dir),
            },
        },
        AgentExecutableRunRequest {
            run_id: "run-1",
            cancellation_id: "run-1",
            session: "default",
            cwd: None,
            input: "hi",
            history_messages: "",
            tool_context: "",
            debug: None,
        },
        &envelope,
        0,
        None,
        None,
    );

    assert_eq!(result, Err(SocketRuntimeError::CannotRunAgent));
    assert!(!control_dir.join("egress-run-1").exists());
}

#[test]
fn agent_executable_socket_bwrap_ignores_request_workspace()
-> Result<(), Box<dyn std::error::Error>> {
    let root = reference_tree("agent-bwrap-workspace");
    let session_root = agent_session_root(&root, "coder");
    let view = derive_agent_runtime_view(&root, "coder")
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        r#"#!/bin/sh
workspace_env=absent
workspace_mount=absent
workspace_context=neutral
IFS= read -r envelope || exit 2
tool_context="$(printf '%s' "$envelope" | jq -r '.tool_context')"
if [ -n "${CTX_WORKSPACE+x}" ]; then workspace_env=leaked; fi
if [ -e /workspace/etc/passwd ]; then workspace_mount=leaked; fi
case "$tool_context" in
  *'Host workspace configuration: determined by agent policy'*) ;;
  *) workspace_context=leaked ;;
esac
printf '{"type":"delta","run":"%s","text":"%s-%s-%s"}\n' "$CTX_RUN_ID" "$workspace_env" "$workspace_mount" "$workspace_context"
"#,
    );
    set_file_mode(&agent_executable, 0o755);
    let (mut client, mut socket) = UnixStream::pair()?;
    client.write_all(
        br#"{"op":"send","id":"msg-1","session":"default","cwd":"/workspace","workspace":"/","input":"hi"}
"#,
    )?;
    client.shutdown(Shutdown::Write)?;

    let outcome = serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        AgentExecutableSocketRuntime {
            ctx_root: &root,
            source_root: &root,
            identity: view.identity(),
            env: view.env(),
            session_root: &session_root,
            default_cwd: "/workspace",
            model: Some("debug/echo"),
            network_allowed: false,
            agent_name: "coder",
            agent_executable: &agent_executable,
            execution: AgentExecutableSocketExecution::Bwrap {
                program: Path::new("/usr/bin/bwrap"),
                mount_table: view.mount_table(),
                control_dir: None,
            },
        },
    )
    .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    assert!(outcome.jsonl().contains("absent-absent-neutral"));
    assert!(!outcome.jsonl().contains("leaked"));
    Ok(())
}

#[test]
fn agent_executable_socket_bwrap_args_preserve_network_when_policy_allows() {
    let root = reference_tree("bwrap-network");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent").join("coder");
    let runtime = AgentExecutableSocketRuntime {
        ctx_root: &root,
        source_root: &root,
        identity: view.identity(),
        env: view.env(),
        session_root: &session_root,
        default_cwd: "/workspace",
        model: Some("debug/echo"),
        network_allowed: true,
        agent_name: "coder",
        agent_executable: &agent_executable,
        execution: AgentExecutableSocketExecution::Bwrap {
            program: Path::new("/usr/bin/bwrap"),
            mount_table: view.mount_table(),
            control_dir: None,
        },
    };

    let args = agent_executable_socket_bwrap_args(&BwrapAgentExecutableArgs {
        runtime,
        mount_table: view.mount_table(),
        cwd: "/workspace",
        debug: None,
        input: "hi",
        agent_executable_fd: 9,
        agent_home_source_fd: 10,
        agent_home_sandbox_fd: 11,
        agent_home: session_root.parent().unwrap_or(&session_root),
        environment: &[],
        control_socket: None,
        control_environment: None,
        control_gate: None,
        provider_egress: None,
    });

    assert!(!args.contains(&"--unshare-net".to_owned()));
    assert!(args.contains(&"--unshare-pid".to_owned()));
    assert!(
        !args
            .iter()
            .any(|arg| arg == crate::runtime::egress::PROVIDER_EGRESS_DIR_ENV)
    );
}

#[test]
fn agent_executable_socket_bwrap_args_preserve_explicit_workspace_mount() {
    let root = reference_tree("agent-bwrap-explicit-workspace");
    let session_root = agent_session_root(&root, "coder");
    let control = root.join("agent").join("coder.d");
    write_text_file(
        &control.join("mount"),
        "/ctx\t/ctx\tro\trbind,nosuid,nodev\n/repo-explicit\t/workspace\trw\trbind,nosuid,nodev\n",
    );
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent").join("coder");
    let runtime = AgentExecutableSocketRuntime {
        ctx_root: &root,
        source_root: &root,
        identity: view.identity(),
        env: view.env(),
        session_root: &session_root,
        default_cwd: "/workspace",
        model: Some("debug/echo"),
        network_allowed: false,
        agent_name: "coder",
        agent_executable: &agent_executable,
        execution: AgentExecutableSocketExecution::Bwrap {
            program: Path::new("/usr/bin/bwrap"),
            mount_table: view.mount_table(),
            control_dir: None,
        },
    };

    let args = agent_executable_socket_bwrap_args(&BwrapAgentExecutableArgs {
        runtime,
        mount_table: view.mount_table(),
        cwd: "/workspace",
        debug: None,
        input: "hi",
        agent_executable_fd: 9,
        agent_home_source_fd: 10,
        agent_home_sandbox_fd: 11,
        agent_home: session_root.parent().unwrap_or(&session_root),
        environment: &[],
        control_socket: None,
        control_environment: None,
        control_gate: None,
        provider_egress: None,
    });

    assert!(contains_arg_triplet(
        &args,
        "--bind",
        "/repo-explicit",
        "/workspace"
    ));
    assert!(!contains_arg_triplet(
        &args,
        "--bind",
        "/repo-default",
        "/workspace"
    ));
}

use super::*;

const BWRAP_CAPABILITY_PROBE: &str = r#"#!/bin/sh
IFS= read -r envelope || exit 2
[ "$(env | grep -c '^CTX_CONTROL_')" = 1 ] || exit 3
/usr/bin/python3 - <<'PY' || exit 4
import json, os, socket, sys
run = os.environ["CTX_RUN_ID"]
stream = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
stream.connect(os.environ["CTX_CONTROL_SOCKET"])
stream.sendall((json.dumps({"op":"ping", "request_id":"startup-" + run, "agent":"coder", "session":"default", "run":run}) + "\n").encode())
reply_bytes = b""
while not reply_bytes.endswith(b"\n"):
    chunk = stream.recv(16384)
    if not chunk:
        sys.exit(4)
    reply_bytes += chunk
reply = json.loads(reply_bytes.decode())
if reply.get("type") != "pong" or reply.get("request_id") != "startup-" + run:
    sys.exit(4)
PY
printf '{"type":"delta","run":"%s","text":"capability-ok"}\n' "$CTX_RUN_ID"
"#;

fn assert_unregistered_bwrap_capability_client_is_denied(
    capability: &std::sync::Arc<crate::runtime::control::RunCapability>,
    identity: &crate::AgentUnixIdentity,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut sibling =
        crate::runtime::socket::command_for_agent_identity("/usr/bin/python3", identity);
    sibling
        .args([
            "-c",
            "import os, socket, sys; s=socket.socket(socket.AF_UNIX); s.connect(os.environ['CTX_CONTROL_SOCKET']); s.sendall(b'{\"op\":\"ping\",\"request_id\":\"sibling\",\"agent\":\"coder\",\"session\":\"default\",\"run\":\"run-1\"}\\n');\ntry: reply=s.recv(1)\nexcept OSError: reply=b''\nsys.exit(1 if reply else 0)",
        ])
        .env("CTX_CONTROL_SOCKET", capability.socket());
    let output = sibling.output()?;
    assert!(
        output.status.success(),
        "unregistered same-UID client unexpectedly received: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

fn run_registered_bwrap_capability_probe(
    root: &Path,
    view: &crate::AgentRuntimeView,
    session_root: &Path,
    executable: &Path,
    control_dir: &Path,
    capability: &std::sync::Arc<crate::runtime::control::RunCapability>,
) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    let environment = crate::runtime::control::RunCapability::environment(Path::new(
        crate::runtime::socket::SOCKET_RUN_CONTROL_PATH,
    ));
    let gate = capability.launch_gate()?;
    let opened = open_agent_executable_no_follow(executable)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let runtime = AgentExecutableSocketRuntime {
        ctx_root: root,
        source_root: root,
        identity: view.identity(),
        env: view.env(),
        session_root,
        default_cwd: "/workspace",
        model: Some("debug/echo"),
        network_allowed: false,
        agent_name: "coder",
        agent_executable: executable,
        execution: AgentExecutableSocketExecution::Bwrap {
            program: Path::new("/usr/bin/bwrap"),
            mount_table: view.mount_table(),
            control_dir: Some(control_dir),
        },
    };
    let request = AgentExecutableRunRequest {
        run_id: "run-1",
        cancellation_id: "run-1",
        session: "default",
        cwd: None,
        input: "ignored",
        history_messages: "[]",
        tool_context: "",
        debug: None,
    };
    let (mut command, agent_executable_fds) = agent_executable_socket_command(
        runtime,
        &opened,
        request,
        0,
        Some((capability.socket(), environment.as_slice(), gate.block_fd())),
        None,
    )
    .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    assert!(
        command
            .get_args()
            .filter_map(|arg| arg.to_str())
            .any(|arg| arg == "--block-fd")
    );
    let mut child = command.spawn()?;
    let mut gate = gate;
    gate.register_and_release(child.id())?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("agent stdin unavailable"))?;
    stdin.write_all(agent_envelope("run-1").as_bytes())?;
    drop(stdin);
    let output = child.wait_with_output()?;
    drop(agent_executable_fds);
    Ok(output)
}

#[test]
fn agent_bwrap_capability_allows_only_registered_launch_roots()
-> Result<(), Box<dyn std::error::Error>> {
    let root = reference_tree("agent-bwrap-capability");
    let session_root = agent_session_root(&root, "coder");
    let view = derive_agent_runtime_view(&root, "coder")
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let executable = root.join("agent").join("coder");
    write_text_file(&executable, BWRAP_CAPABILITY_PROBE);
    set_file_mode(&executable, 0o755);
    let control_dir = root.join("runtime");
    fs::create_dir_all(&control_dir)?;
    fs::set_permissions(&control_dir, fs::Permissions::from_mode(0o711))?;
    let (capability, listener) = crate::runtime::control::RunCapability::create_with_source(
        &control_dir,
        &root,
        "coder",
        "default",
        "run-1",
        view.identity().uid(),
        view.identity().gid(),
    )?;
    let capability = std::sync::Arc::new(capability);
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_shutdown = std::sync::Arc::clone(&shutdown);
    let (startup_sender, startup_receiver) = std::sync::mpsc::sync_channel(1);
    let server_capability = std::sync::Arc::clone(&capability);
    let server = std::thread::spawn(move || {
        server_capability.serve_run(&listener, &server_shutdown, &startup_sender, || {
            Some("run-1".to_owned())
        })
    });

    assert_unregistered_bwrap_capability_client_is_denied(&capability, view.identity())?;
    let output = run_registered_bwrap_capability_probe(
        &root,
        &view,
        &session_root,
        &executable,
        &control_dir,
        &capability,
    )?;
    assert!(
        output.status.success(),
        "bwrap capability process failed: {} {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("capability-ok"));
    assert!(matches!(
        startup_receiver.recv_timeout(Duration::from_secs(1)),
        Ok(Ok(()))
    ));
    shutdown.store(true, std::sync::atomic::Ordering::Release);
    assert!(matches!(server.join(), Ok(Ok(()))));
    capability.cleanup()?;
    Ok(())
}
