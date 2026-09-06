#![expect(clippy::redundant_pub_crate, reason = "test functions inside module")]

use super::*;
use crate::agent::createop::{child_executable, create_child};
use crate::*;

struct ConflictTool(crate::agent::create::AgentRollbackConflict);

impl cortexfs_tool_sdk::Tool for ConflictTool {
    fn spec(&self) -> cortexfs_tool_sdk::ToolSpec {
        cortexfs_tool_sdk::ToolSpec {
            name: "agent.create",
            description: "test",
            input_schema: "{\"type\":\"object\"}",
        }
    }

    fn call(
        &self,
        _invocation: &ToolInvocation,
        _output: &mut cortexfs_tool_sdk::ToolEmitter<&mut dyn std::io::Write>,
    ) -> cortexfs_tool_sdk::ToolResult<()> {
        Err(create_error(
            crate::agent::create::AgentCreateError::RollbackConflict(self.0.clone()),
        ))
    }
}

#[test]
pub(crate) fn withheld_agent_create_error_contains_complete_rollback_conflict() {
    let conflict = crate::agent::create::AgentRollbackConflict {
        original: PathBuf::from("/ctx/agent/worker"),
        quarantine: Some(PathBuf::from("/ctx/agent/.ctx-rollback-1")),
        dev: 7,
        ino: 11,
        stage: "original-recreated",
    };

    let error = create_error(crate::agent::create::AgentCreateError::RollbackConflict(
        conflict,
    ));

    assert_eq!(error.code(), "EIO");
    for expected in [
        "original=/ctx/agent/worker",
        "quarantine=/ctx/agent/.ctx-rollback-1",
        "dev=7",
        "ino=11",
        "stage=original-recreated",
    ] {
        assert!(error.message().contains(expected));
    }

    let conflict = ConflictTool(crate::agent::create::AgentRollbackConflict {
        original: PathBuf::from("/ctx/agent/worker"),
        quarantine: Some(PathBuf::from("/ctx/agent/.ctx-rollback-1")),
        dev: 7,
        ino: 11,
        stage: "original-recreated",
    });
    let mut jsonl = Vec::new();
    assert!(run_tool(&conflict, &ToolInvocation::new("r1", "{}"), &mut jsonl).is_ok());
    let frames = String::from_utf8_lossy(&jsonl)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect::<Vec<_>>();
    let error = frames.iter().find(|frame| frame["type"] == "error");
    assert!(error.is_some());
    let Some(error) = error else {
        return;
    };
    assert_eq!(error["code"], "EIO");
    let message = error["message"].as_str().unwrap_or_default();
    for expected in [
        "original=/ctx/agent/worker",
        "quarantine=/ctx/agent/.ctx-rollback-1",
        "dev=7",
        "ino=11",
        "stage=original-recreated",
    ] {
        assert!(message.contains(expected));
    }
}

#[test]
pub(crate) fn withheld_agent_create_uses_standard_agent_wrapper() {
    let wrapper = child_executable("worker-1");
    assert_eq!(
        wrapper,
        executable_wrapper_script(
            ObjectClass::Agent,
            "worker-1",
            "/ctx/bin/cortexfs-object-runner"
        )
    );
    assert!(!wrapper.contains("/ctx/model/"));
}

#[test]
pub(crate) fn agent_create_is_consistent_across_public_dispatch() {
    let spec = core_tool_specs()
        .into_iter()
        .find(|spec| spec.name == "agent.create");
    assert!(spec.is_some());
    let Some(spec) = spec else { return };
    assert_eq!(
        spec.input_schema,
        crate::agent::createop::AGENT_CREATE_SCHEMA
    );

    let invocation = ToolInvocation::new("r1", r#"{"name":"worker","handoff":"task"}"#);
    let mut output = Vec::new();
    assert!(matches!(
        run_core_tool("agent.create", &invocation, &mut output),
        Ok(true)
    ));
    assert!(!output.is_empty());

    output.clear();
    assert!(matches!(
        run_core_tool_cli_with_root(Path::new("/ctx"), "agent.create", &[], &mut output),
        Ok(Some(_))
    ));
    assert!(!output.is_empty());
}

/// agent.update 必须在 spec 列表、tool dispatch 与 CLI dispatch 三条公共路径上一致，
/// 且缺失 run capability 环境时以错误帧 fail closed 而不是静默成功。
#[test]
pub(crate) fn agent_update_is_consistent_across_public_dispatch() {
    let spec = core_tool_specs()
        .into_iter()
        .find(|spec| spec.name == "agent.update");
    assert!(spec.is_some());
    let Some(spec) = spec else { return };
    assert_eq!(
        spec.input_schema,
        crate::agent::updateop::AGENT_UPDATE_SCHEMA
    );

    let invocation = ToolInvocation::new("r1", r#"{"control":"system.md","content":"updated"}"#);
    let mut output = Vec::new();
    assert!(matches!(
        run_core_tool("agent.update", &invocation, &mut output),
        Ok(true)
    ));
    assert!(!output.is_empty());

    let rejected = ToolInvocation::new("r1", r#"{"control":"policy","content":"allow"}"#);
    output.clear();
    assert!(matches!(
        run_core_tool("agent.update", &rejected, &mut output),
        Ok(true)
    ));
    let frames = String::from_utf8_lossy(&output).into_owned();
    assert!(frames.contains("\"type\":\"error\""), "{frames}");

    output.clear();
    assert!(matches!(
        run_core_tool_cli_with_root(Path::new("/ctx"), "agent.update", &[], &mut output),
        Ok(Some(_))
    ));
    assert!(!output.is_empty());
}

#[test]
#[ignore = "subprocess entrypoint for agent.create lifecycle test"]
pub(crate) fn agent_create_lifecycle_subprocess() {
    let mode = std::env::var("CORTEXFS_TEST_CHILD_LIFE").unwrap_or_default();
    let request_id = std::env::var("CORTEXFS_TEST_REQUEST_ID").unwrap_or_default();
    let (name, input) = match mode.as_str() {
        "default" => (
            "worker-default",
            r#"{"name":"worker-default","handoff":"default handoff"}"#,
        ),
        "temp" => (
            "worker-temp",
            r#"{"name":"worker-temp","handoff":"temp handoff","life":"temp"}"#,
        ),
        "path" => (
            "worker-path",
            r#"{"name":"worker-path","handoff":"path handoff","path":"/ctx/home/1000/tool"}"#,
        ),
        "window" => (
            "worker-window",
            r#"{"name":"worker-window","handoff":"window handoff","window":2048}"#,
        ),
        _ => return,
    };
    let invocation = ToolInvocation::new(request_id, input);
    let mut output = Vec::new();
    assert!(matches!(
        run_core_tool("agent.create", &invocation, &mut output),
        Ok(true)
    ));
    let output = String::from_utf8_lossy(&output);
    assert!(output.contains(&format!("child {name} active")), "{output}");
}

#[test]
pub(crate) fn agent_create_passes_lifecycle_and_tool_path_to_runtime()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let root = tempfile::tempdir()?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o711))?;
    let identity = std::fs::metadata(root.path())?;
    let (capability, listener) = crate::runtime::control::RunCapability::create(
        root.path(),
        "parent",
        "default",
        "run-1",
        identity.uid(),
        identity.gid(),
    )?;
    capability.register_launch_root(std::process::id())?;
    let environment = crate::runtime::control::RunCapability::environment(capability.socket());
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_shutdown = Arc::clone(&shutdown);
    let (startup_sender, _startup_receiver) = std::sync::mpsc::sync_channel(1);
    let (request_sender, request_receiver) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        capability.serve_run_with_handler(
            &listener,
            &server_shutdown,
            &startup_sender,
            || Some("run-1".to_owned()),
            |request| {
                request_sender
                    .send((
                        request.child.clone(),
                        request.life.clone(),
                        request.path.clone(),
                        request.window,
                    ))
                    .map_err(|_error| crate::runtime::control::RunCapabilityError::CannotCreate)?;
                Ok(crate::runtime::control::CreateChildResult {
                    child: request.child,
                    child_session: request.child_session,
                    pid: 42,
                })
            },
            |_request| Err(crate::runtime::control::RunCapabilityError::Unsupported),
        )
    });
    for (mode, request_id) in [
        ("default", "create-default"),
        ("temp", "create-temp"),
        ("path", "create-path"),
        ("window", "create-window"),
    ] {
        let output = std::process::Command::new(std::env::current_exe()?)
            .arg("--exact")
            .arg("tool::core::tools::tests::runtime::agent_create_lifecycle_subprocess")
            .arg("--ignored")
            .env("CORTEXFS_TEST_CHILD_LIFE", mode)
            .env("CORTEXFS_TEST_REQUEST_ID", request_id)
            .env("CTX_AGENT", "parent")
            .env("CTX_SESSION", "default")
            .env("CTX_RUN_ID", "run-1")
            .env(&environment[0].0, &environment[0].1)
            .output()?;
        assert!(output.status.success(), "{output:?}");
    }
    assert_eq!(
        [
            request_receiver.recv()?,
            request_receiver.recv()?,
            request_receiver.recv()?,
            request_receiver.recv()?,
        ],
        [
            ("worker-default".to_owned(), "owned".to_owned(), None, None),
            ("worker-temp".to_owned(), "temp".to_owned(), None, None),
            (
                "worker-path".to_owned(),
                "owned".to_owned(),
                Some("/ctx/home/1000/tool".to_owned()),
                None,
            ),
            (
                "worker-window".to_owned(),
                "owned".to_owned(),
                None,
                Some(2048)
            )
        ]
    );
    shutdown.store(true, Ordering::Release);
    assert!(matches!(server.join(), Ok(Ok(()))));
    Ok(())
}

#[test]
pub(crate) fn agent_create_rejects_invalid_lifecycle() {
    for life in ["detached", " temp "] {
        let input = format!(r#"{{"name":"worker","handoff":"task","life":"{life}"}}"#);
        let invocation = ToolInvocation::new("invalid-life", input);
        let mut output = Vec::new();
        assert!(matches!(
            run_core_tool("agent.create", &invocation, &mut output),
            Ok(true)
        ));
        assert!(String::from_utf8_lossy(&output).contains("life must be owned or temp"));
    }
}

#[test]
pub(crate) fn agent_create_rejects_invalid_window_before_runtime() {
    for window in ["0", "-1", "1.5", "\"1\"", "4294967296"] {
        let input = format!(r#"{{"name":"worker","handoff":"task","window":{window}}}"#);
        let invocation = ToolInvocation::new("invalid-window", input);
        let mut output = Vec::new();
        assert!(matches!(
            run_core_tool("agent.create", &invocation, &mut output),
            Ok(true)
        ));
        assert!(String::from_utf8_lossy(&output).contains("window must be a positive u32 integer"));
    }
}

#[test]
#[ignore = "requires an explicitly authorized live parent runtime"]
pub(crate) fn live_withheld_agent_create_reaches_active_with_real_pid() {
    let Ok(name) = std::env::var("CORTEXFS_LIVE_CHILD") else {
        return;
    };
    let Ok((session, pid)) = create_child(&name, "live P3 handoff", "owned") else {
        return;
    };
    assert!(!session.is_empty());
    assert!(pid > 0);
    let Ok(source) = std::env::var("CTX_SOURCE") else {
        return;
    };
    let source = PathBuf::from(source);
    let control = source.join("agent").join(format!("{name}.d"));
    assert!(fs::read_to_string(control.join("status")).is_ok_and(|value| value == "ready\n"));
    assert_eq!(
        fs::read_to_string(control.join("pid"))
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok()),
        Some(pid)
    );
}

#[test]
pub(crate) fn shell_exec_tool_returns_stdout() {
    let tool = ShellExecTool;
    let invocation = ToolInvocation::new("r1", r#"{"cmd":"printf shell-ok"}"#);
    let mut output = Vec::new();
    assert!(run_tool(&tool, &invocation, &mut output).is_ok());
    let text = String::from_utf8(output).unwrap_or_default();
    assert!(text.contains(r#""tool":"shell.exec""#));
    assert!(text.contains("shell-ok"));
}

#[test]
pub(crate) fn shell_exec_rejects_oversized_output() {
    let output = run_shell_exec_command_with_timeout(
        "head -c 131072 /dev/zero | tr '\\0' x",
        Duration::from_secs(2),
    );

    assert!(matches!(output, Err(ref error) if error.contains("output exceeds")));
}

#[test]
pub(crate) fn shell_exec_cli_rejects_oversized_output() {
    let mut output = Vec::new();
    let result = run_core_tool_cli(
        "shell.exec",
        &[
            OsString::from("head -c 131072 /dev/zero"),
            OsString::from("| tr '\\0' x"),
        ],
        &mut output,
    );

    assert!(result.is_err());
}

#[test]
pub(crate) fn shell_exec_kills_process_after_oversized_output() {
    let started = Instant::now();

    let output = run_shell_exec_command_with_timeout("yes x", Duration::from_secs(10));

    assert!(matches!(output, Err(ref error) if error.contains("output exceeds")));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
pub(crate) fn shell_exec_accepts_output_at_limit() {
    let output = run_shell_exec_command_with_timeout(
        &format!("head -c {MAX_SHELL_EXEC_OUTPUT_BYTES} /dev/zero | tr '\\0' x"),
        Duration::from_secs(2),
    );

    assert!(output.is_ok());
    let Ok(output) = output else {
        return;
    };
    assert!(output.status.success());
    assert_eq!(output.stdout.len(), MAX_SHELL_EXEC_OUTPUT_BYTES);
}

#[test]
pub(crate) fn shell_exec_times_out_instead_of_hanging() {
    for command in [
        "sleep 5",
        "sleep 5 &",
        "sleep 5 >/dev/null &",
        "sleep 5 2>/dev/null &",
    ] {
        let started = Instant::now();
        let output = run_shell_exec_command_with_timeout(command, Duration::from_millis(100));
        assert!(
            matches!(output, Err(ref error) if error.contains("timed out")),
            "{command}"
        );
        assert!(started.elapsed() < Duration::from_secs(2), "{command}");
    }
}

#[test]
pub(crate) fn shell_exec_uses_absolute_shell_path() {
    assert_eq!(shell_exec_command().get_program(), SHELL_EXEC_SHELL);
}

#[test]
pub(crate) fn shell_exec_command_uses_clean_runtime_environment() {
    let command = shell_exec_command();
    let envs = command
        .get_envs()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(envs.len(), 2);
    assert!(envs.contains(&("PATH".to_owned(), Some("/usr/bin:/bin".to_owned()))));
    assert!(envs.contains(&("GIT_OPTIONAL_LOCKS".to_owned(), Some("0".to_owned()))));
}

#[test]
pub(crate) fn tsh_config_writer_updates_runtime_config_file() {
    let dir = std::env::temp_dir().join(format!("cortexfs-tsh-config-{}", std::process::id()));
    let path = dir.join("tool/tsh.d/config");
    let config = TshRuntimeConfig {
        max_loaded_tools: 12,
        cache_capacity: 6,
        window_percent: 10,
    };
    assert!(write_tsh_runtime_config(&path, config).is_ok());
    let config = fs::read_to_string(&path).unwrap_or_default();
    assert!(config.contains("max_loaded_tools=12\n"));
    assert!(config.contains("cache_capacity=6\n"));
    assert!(config.contains("window_percent=10\n"));
    let _ignored = fs::remove_dir_all(dir);
}

#[test]
pub(crate) fn tsh_runtime_config_rejects_oversized_tool_counts() {
    assert!(parse_tsh_runtime_config("max_loaded_tools=1025\n").is_err());
    assert!(parse_tsh_runtime_config("cache_capacity=1025\n").is_err());
    assert!(tsh_tool_count(&serde_json::json!(1025), "cache_capacity").is_err());
}

#[test]
pub(crate) fn tsh_config_writer_rejects_symlink_parent_directory() {
    let dir = std::env::temp_dir().join(format!(
        "cortexfs-tsh-config-parent-symlink-{}",
        std::process::id()
    ));
    let path = dir.join("tool/tsh.d/config");
    let outside = dir.join("outside");
    let config = TshRuntimeConfig {
        max_loaded_tools: 12,
        cache_capacity: 6,
        window_percent: 10,
    };
    let _ignored = fs::remove_dir_all(&dir);
    assert!(fs::create_dir_all(dir.join("tool")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(symlink(&outside, dir.join("tool/tsh.d")).is_ok());

    assert!(write_tsh_runtime_config(&path, config).is_err());
    assert!(!outside.join("config").exists());
    assert!(!fs::read_dir(&outside).map_or(true, |entries| {
        entries.filter_map(Result::ok).any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".config.tmp-")
        })
    }));
    assert!(
        dir.join("tool/tsh.d")
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
    );
    let _ignored = fs::remove_dir_all(dir);
}

#[test]
pub(crate) fn tsh_config_writer_rejects_symlink_intermediate_directory() {
    let dir = std::env::temp_dir().join(format!(
        "cortexfs-tsh-config-intermediate-symlink-{}",
        std::process::id()
    ));
    let outside = std::env::temp_dir().join(format!(
        "cortexfs-tsh-config-intermediate-outside-{}",
        std::process::id()
    ));
    let path = dir.join("tool/tsh.d/config");
    let config = TshRuntimeConfig {
        max_loaded_tools: 12,
        cache_capacity: 6,
        window_percent: 10,
    };
    let _ignored = fs::remove_dir_all(&dir);
    let _ignored = fs::remove_dir_all(&outside);
    assert!(fs::create_dir_all(&dir).is_ok());
    assert!(fs::create_dir_all(outside.join("tsh.d")).is_ok());
    assert!(symlink(&outside, dir.join("tool")).is_ok());

    assert!(write_tsh_runtime_config(&path, config).is_err());
    assert!(!outside.join("tsh.d/config").exists());
    assert!(
        !fs::read_dir(outside.join("tsh.d")).map_or(true, |entries| {
            entries.filter_map(Result::ok).any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".config.tmp-")
            })
        })
    );
    let _ignored = fs::remove_dir_all(dir);
    let _ignored = fs::remove_dir_all(outside);
}

#[test]
pub(crate) fn tsh_config_reader_refuses_symlink_targets() {
    let dir = std::env::temp_dir().join(format!(
        "cortexfs-tsh-config-symlink-{}",
        std::process::id()
    ));
    let path = dir.join("tool/tsh.d/config");
    let outside = dir.join("outside-config");
    assert!(fs::create_dir_all(path.parent().unwrap_or(&dir)).is_ok());
    assert!(fs::write(&outside, "max_loaded_tools=12\n").is_ok());
    assert!(symlink(&outside, &path).is_ok());

    let result = read_tsh_runtime_config(&path);

    assert!(result.is_err());
    let _ignored = fs::remove_dir_all(dir);
}

#[test]
pub(crate) fn tsh_config_reader_refuses_symlink_intermediate_directory() {
    let dir = std::env::temp_dir().join(format!(
        "cortexfs-tsh-config-read-intermediate-{}",
        std::process::id()
    ));
    let outside = std::env::temp_dir().join(format!(
        "cortexfs-tsh-config-read-intermediate-outside-{}",
        std::process::id()
    ));
    let path = dir.join("tool/tsh.d/config");
    let _ignored = fs::remove_dir_all(&dir);
    let _ignored = fs::remove_dir_all(&outside);
    assert!(fs::create_dir_all(&dir).is_ok());
    assert!(fs::create_dir_all(outside.join("tsh.d")).is_ok());
    assert!(fs::write(outside.join("tsh.d/config"), "max_loaded_tools=12\n").is_ok());
    assert!(symlink(&outside, dir.join("tool")).is_ok());

    let result = read_tsh_runtime_config(&path);

    assert!(result.is_err());
    let _ignored = fs::remove_dir_all(dir);
    let _ignored = fs::remove_dir_all(outside);
}

#[test]
pub(crate) fn tsh_config_tool_rejects_non_default_path() {
    let tool = TshConfigTool;
    let invocation = ToolInvocation::new(
        "r1",
        r#"{"path":"/tmp/cortexfs-outside-config","max_loaded_tools":12}"#,
    );
    let mut output = Vec::new();
    assert!(run_tool(&tool, &invocation, &mut output).is_ok());
    let text = String::from_utf8(output).unwrap_or_default();
    assert!(text.contains(r#""code":"EACCES""#));
    assert!(text.contains(r#""status":"error""#));
}
