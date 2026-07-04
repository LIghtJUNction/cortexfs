use super::*;

#[test]
fn shell_exec_tool_returns_stdout() {
    let tool = ShellExecTool;
    let invocation = ToolInvocation::new("r1", r#"{"cmd":"printf shell-ok"}"#);
    let mut output = Vec::new();
    assert!(run_tool(&tool, &invocation, &mut output).is_ok());
    let text = String::from_utf8(output).unwrap_or_default();
    assert!(text.contains(r#""tool":"shell.exec""#));
    assert!(text.contains("shell-ok"));
}

#[test]
fn shell_exec_rejects_oversized_output() {
    let output = run_shell_exec_command_with_timeout(
        "head -c 131072 /dev/zero | tr '\\0' x",
        Duration::from_secs(2),
    );

    assert!(matches!(output, Err(ref error) if error.contains("output exceeds")));
}

#[test]
fn shell_exec_cli_rejects_oversized_output() {
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
fn shell_exec_kills_process_after_oversized_output() {
    let started = Instant::now();

    let output = run_shell_exec_command_with_timeout("yes x", Duration::from_secs(10));

    assert!(matches!(output, Err(ref error) if error.contains("output exceeds")));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn shell_exec_accepts_output_at_limit() {
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
fn shell_exec_times_out_instead_of_hanging() {
    let started = Instant::now();

    let output = run_shell_exec_command_with_timeout("sleep 5", Duration::from_millis(100));

    assert!(matches!(output, Err(ref error) if error.contains("timed out")));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn shell_exec_uses_absolute_shell_path() {
    assert_eq!(shell_exec_command().get_program(), SHELL_EXEC_SHELL);
}

#[test]
fn shell_exec_command_uses_clean_runtime_environment() {
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
fn tsh_config_writer_updates_runtime_config_file() {
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
fn tsh_runtime_config_rejects_oversized_tool_counts() {
    assert!(parse_tsh_runtime_config("max_loaded_tools=1025\n").is_err());
    assert!(parse_tsh_runtime_config("cache_capacity=1025\n").is_err());
    assert!(tsh_tool_count(&serde_json::json!(1025), "cache_capacity").is_err());
}

#[test]
fn tsh_config_writer_rejects_symlink_parent_directory() {
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
fn tsh_config_writer_rejects_symlink_intermediate_directory() {
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
fn tsh_config_reader_refuses_symlink_targets() {
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
fn tsh_config_reader_refuses_symlink_intermediate_directory() {
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
fn tsh_config_tool_rejects_non_default_path() {
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
