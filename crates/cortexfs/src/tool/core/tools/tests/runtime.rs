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
    let started = Instant::now();

    let output = run_shell_exec_command_with_timeout("sleep 5", Duration::from_millis(100));

    assert!(matches!(output, Err(ref error) if error.contains("timed out")));
    assert!(started.elapsed() < Duration::from_secs(2));
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
