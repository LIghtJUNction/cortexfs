struct OverlayTestDir(tempfile::TempDir);

impl OverlayTestDir {
    fn new() -> std::io::Result<Self> {
        tempfile::Builder::new()
            .prefix("cfs-atl-overlay-write-")
            .tempdir()
            .map(Self)
    }

    fn path(&self) -> &Path {
        self.0.path()
    }
}

impl Drop for OverlayTestDir {
    fn drop(&mut self) {
        let work = self.0.path().join("work/work");
        let _ignored = fs::set_permissions(work, fs::Permissions::from_mode(0o700));
    }
}

#[test]
fn agent_tool_call_executes_visible_tsh_for_search_and_load()
-> Result<(), Box<dyn std::error::Error>> {
    let (root, tool_control) = agent_tool_fixture("atl", "tsh")?;
    fs::create_dir_all(root.join("home/1000/agent/executor"))?;
    fs::write(
        tool_control.join("policy"),
        "allow executor_t tool:tsh execute\n",
    )?;
    fs::write(
        root.join("tool").join("tsh"),
        r#"#!/bin/sh
case "$1" in
  tools)
    text='fs.read\nshell.exec\ntsh\n'
    ;;
  load)
    if [ "$2" = "fs.read" ]; then
      text="loaded fs.read\\t$CTX_ROOT/tool/fs.read\\tmetadata\\n"
    else
      printf '{"type":"start","run":"%s","tool":"tsh"}\n' "$CTX_RUN_ID"
      printf '{"type":"error","run":"%s","code":"EINVAL","message":"unknown tool"}\n' "$CTX_RUN_ID"
      printf '{"type":"done","run":"%s","status":"error"}\n' "$CTX_RUN_ID"
      exit 0
    fi
    ;;
  *)
    exit 2
    ;;
esac
printf '{"type":"start","run":"%s","tool":"tsh"}\n' "$CTX_RUN_ID"
printf '{"type":"message","run":"%s","role":"tool","content":[{"type":"text","text":"%s"}]}\n' "$CTX_RUN_ID" "$text"
printf '{"type":"done","run":"%s","status":"ok"}\n' "$CTX_RUN_ID"
"#,
    )?;
    fs::set_permissions(
        root.join("tool").join("tsh"),
        fs::Permissions::from_mode(0o755),
    )?;
    let config = AgentModelRunConfig {
        ctx_root: root.clone(),
        source: root.clone(),
        ..test_agent_run_config()
    };

    let search = AgentToolCall {
        id: "call-1".to_owned(),
        name: "tsh".to_owned(),
        args: vec![OsString::from("tools")],
    };
    let search_result = execute_prepared_agent_tool_call(&config, &search)?;

    assert!(search_result.contains("fs.read"));
    assert!(search_result.contains("tsh"));

    let load = AgentToolCall {
        id: "call-2".to_owned(),
        name: "tsh".to_owned(),
        args: vec![OsString::from("load"), OsString::from("fs.read")],
    };
    let load_result = execute_prepared_agent_tool_call(&config, &load)?;

    assert!(load_result.contains("loaded fs.read"));
    assert!(load_result.contains("/tool/fs.read"));
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn agent_tool_bwrap_args_use_overlay_workspace_upper() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("tool-overlay-args")?;
    let workspace = root.join("workspace");
    let git = root.join("git");
    let upper = root.join("overlay-upper");
    let work = root.join("overlay-work");
    fs::create_dir_all(&workspace)?;
    fs::create_dir_all(&upper)?;
    fs::create_dir_all(&work)?;
    let config = AgentModelRunConfig {
        ctx_root: root.join("ctx"),
        source: root.join("source"),
        ..test_agent_run_config()
    };
    let mount_table = cortexfs::MountTable::parse(&format!(
        "{}\t/ctx\tro\trbind,nosuid,nodev\n{}\t/workspace\trw\trbind,nosuid,nodev\n{}\t/workspace/.git\trw\trbind,nosuid,nodev\n",
        config.source.display(),
        workspace.display(),
        git.display()
    ))
    .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let sandbox = AgentToolSandbox {
        workspace: workspace.clone(),
        upper: upper.clone(),
        work: work.clone(),
    };
    let env = vec![
        ("SAFE_ENV".to_owned(), "ok".to_owned()),
        ("CTX_PROVIDER_SECRET_FD".to_owned(), "9".to_owned()),
        (
            "CTX_PROVIDER_SECRET_PATH".to_owned(),
            "/run/secret".to_owned(),
        ),
        (
            "CTX_PROVIDER_SECRET_PROVIDER".to_owned(),
            "api.test".to_owned(),
        ),
        ("CTX_PROVIDER_SECRET_SLOT".to_owned(), "default".to_owned()),
    ];

    let args = agent_tool_bwrap_args(&AgentToolBwrapArgs {
        authorized_object: Path::new("/ctx/tool/probe"),
        config: &test_agent_tool_config(&config),
        tool_executable: Path::new("/proc/self/fd/9"),
        tool_args: &[OsString::from("tools")],
        env: &env,
        mount_table: &mount_table,
        cwd: Path::new("/workspace"),
        sandbox: Some(&sandbox),
        network_allowed: false,
        home_fd: 10,
        home_alias_fd: 11,
        home_target: Path::new("/ctx/home/1000/agent/executor"),
        ctx_home_target: Path::new("/ctx/home/1000"),
        control: None,
        control_gate: None,
        invoke_strategy: crate::tool::InvokeStrategy::default(),
    });

    assert!(contains_os_arg_triplet(
        &args,
        "--overlay",
        &upper.display().to_string(),
        &work.display().to_string()
    ));
    assert!(contains_os_arg_triplet(
        &args,
        "--bind",
        &upper.display().to_string(),
        &upper.display().to_string()
    ));
    assert!(contains_os_arg_triplet(
        &args,
        "--bind",
        &work.display().to_string(),
        &work.display().to_string()
    ));
    assert!(contains_os_arg_pair(
        &args,
        "--overlay-src",
        &workspace.display().to_string()
    ));
    assert!(!contains_os_arg_triplet(
        &args,
        "--bind",
        &workspace.display().to_string(),
        "/workspace"
    ));
    assert!(contains_os_arg_triplet(&args, "--setenv", "SAFE_ENV", "ok"));
    assert!(!args.iter().any(|arg| arg == "CTX_PROVIDER_SECRET_FD"));
    assert!(!args.iter().any(|arg| arg == "CTX_PROVIDER_SECRET_PATH"));
    assert!(!args.iter().any(|arg| arg == "CTX_PROVIDER_SECRET_PROVIDER"));
    assert!(!args.iter().any(|arg| arg == "CTX_PROVIDER_SECRET_SLOT"));
    assert!(contains_os_arg_pair(&args, "--chdir", "/workspace"));
    assert!(!args.iter().any(|arg| arg == git.as_os_str()));
    assert!(args.iter().any(|arg| arg == "--unshare-net"));
    assert!(contains_os_arg_triplet(
        &args,
        "--bind-fd",
        "10",
        "/ctx/home/1000/agent/executor"
    ));
    assert!(contains_os_arg_triplet(
        &args,
        "--bind-fd",
        "11",
        "/home/agent"
    ));
    assert!(contains_os_arg_triplet(
        &args,
        "--setenv",
        "HOME",
        "/home/agent"
    ));

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn tool_bwrap_has_no_control_environment_without_host_control()
-> Result<(), Box<dyn std::error::Error>> {
    let config = test_agent_run_config();
    let mounts = cortexfs::MountTable::parse("")
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let args = agent_tool_bwrap_args(&AgentToolBwrapArgs {
        authorized_object: Path::new("/ctx/tool/probe"),
        config: &test_agent_tool_config(&config),
        tool_executable: Path::new("/tool"),
        tool_args: &[],
        env: &[],
        mount_table: &mounts,
        cwd: Path::new("/workspace"),
        sandbox: None,
        network_allowed: false,
        home_fd: 10,
        home_alias_fd: 11,
        home_target: Path::new("/ctx/home/1000/agent/executor"),
        ctx_home_target: Path::new("/ctx/home/1000"),
        control: None,
        control_gate: None,
        invoke_strategy: crate::tool::InvokeStrategy::default(),
    });
    assert!(
        !args
            .iter()
            .any(|arg| arg.to_string_lossy().starts_with("CTX_CONTROL_"))
    );
    Ok(())
}

#[test]
fn agent_tool_process_cancellation_terminates_process_group() -> std::io::Result<()> {
    let root = tempfile::Builder::new()
        .prefix("cfs-tool-cancel-")
        .tempdir()?;
    let leaked = root.path().join("leaked");
    let mut command = std::process::Command::new("/bin/sh");
    command.args([
        "-c",
        &format!("sleep 5; printf leaked > {}", leaked.display()),
    ]);
    let start = Instant::now();
    let result =
        crate::object::executor::exec::run_agent_tool_process_cancellable(&mut command, || {
            start.elapsed() >= Duration::from_millis(100)
        });
    assert_eq!(result, Err(ExecError::new("tool cancelled")));
    thread::sleep(Duration::from_millis(100));
    assert!(!leaked.exists());
    Ok(())
}

#[test]
fn agent_tool_bwrap_exec_writes_workspace_overlay_upper() -> Result<(), Box<dyn std::error::Error>>
{
    let root = OverlayTestDir::new()?;
    let workspace = root.path().join("workspace-lower");
    let source = root.path().join("source");
    let upper = root.path().join("upper");
    let work = root.path().join("work");
    fs::create_dir_all(&source)?;
    fs::create_dir_all(&workspace)?;
    fs::create_dir_all(&upper)?;
    fs::create_dir_all(&work)?;
    let home = source.join("home/1000/agent/executor");
    fs::create_dir_all(&home)?;
    fs::write(workspace.join("README.md"), "lower\n")?;
    let tool = root.path().join("write-workspace");
    write_executable_script(
        &tool,
        "#!/bin/sh\nprintf upper-write > /workspace/generated.txt\nprintf ok\n",
    )?;
    let tool_executable = open_executable_no_follow(&tool)?;
    let config = AgentModelRunConfig {
        ctx_root: root.path().to_path_buf(),
        source: source.clone(),
        ..test_agent_run_config()
    };
    let mount_table = cortexfs::MountTable::parse(&format!(
        "{}\t/ctx\tro\trbind,nosuid,nodev\n",
        source.display()
    ))
    .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let sandbox = AgentToolSandbox {
        workspace: workspace.clone(),
        upper: upper.clone(),
        work,
    };
    let home_dir = open_plain_directory(&home)?;
    let home_alias_dir = home_dir.try_clone()?;
    crate::provider::name::files::clear_fd_cloexec(&home_dir)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    crate::provider::name::files::clear_fd_cloexec(&home_alias_dir)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let args = agent_tool_bwrap_args(&AgentToolBwrapArgs {
        authorized_object: Path::new("/ctx/tool/probe"),
        config: &test_agent_tool_config(&config),
        tool_executable: &proc_fd_path(&tool_executable),
        tool_args: &[],
        env: &[],
        mount_table: &mount_table,
        cwd: Path::new("/workspace"),
        sandbox: Some(&sandbox),
        network_allowed: false,
        home_fd: home_dir.as_raw_fd(),
        home_alias_fd: home_alias_dir.as_raw_fd(),
        home_target: Path::new("/ctx/home/1000/agent/executor"),
        ctx_home_target: Path::new("/ctx/home/1000"),
        control: None,
        control_gate: None,
        invoke_strategy: crate::tool::InvokeStrategy::default(),
    });
    let mut command = std::process::Command::new(BWRAP_PROGRAM);
    command.args(args);
    let output = run_agent_tool_process_with_timeout(&mut command, Duration::from_secs(5))?;

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "ok");
    assert!(!workspace.join("generated.txt").exists());
    let upper_file = find_overlay_generated_file(&upper)?;
    assert_eq!(fs::read_to_string(upper_file)?, "upper-write");
    assert_eq!(fs::read_to_string(workspace.join("README.md"))?, "lower\n");

    Ok(())
}

#[test]
fn visible_workspace_source_falls_back_to_current_namespace_workspace() {
    let missing = PathBuf::from("/tmp/cortexfs-missing-workspace-for-test");
    if Path::new("/workspace").exists() {
        assert_eq!(
            visible_workspace_source(missing),
            PathBuf::from("/workspace")
        );
    }
}

#[test]
fn agent_tool_call_refuses_symlinked_tsh_policy() -> Result<(), Box<dyn std::error::Error>> {
    let (root, tool_control) = agent_tool_fixture("atl-policy-symlink", "tsh")?;
    let outside_policy = root.join("outside-policy");
    fs::write(&outside_policy, "allow executor_t tool:tsh execute\n")?;
    symlink(&outside_policy, tool_control.join("policy"))?;
    fs::write(root.join("tool").join("tsh"), "#!/bin/sh\nexit 0\n")?;
    fs::set_permissions(
        root.join("tool").join("tsh"),
        fs::Permissions::from_mode(0o755),
    )?;
    let config = AgentModelRunConfig {
        ctx_root: root.clone(),
        source: root.clone(),
        ..test_agent_run_config()
    };

    let call = AgentToolCall {
        id: "call-1".to_owned(),
        name: "tsh".to_owned(),
        args: vec![OsString::from("tools")],
    };
    let result = execute_prepared_agent_tool_call(&config, &call);

    assert!(matches!(result, Err(ref error) if error.message().contains("cannot read")));
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn object_runner_executable_open_refuses_symlink_tool() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-executable-symlink")?;
    let target = root.join("target-tool");
    let link = root.join("tsh");
    fs::write(&target, "#!/bin/sh\nexit 0\n")?;
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755))?;
    symlink(&target, &link)?;

    assert!(open_executable_no_follow(&link).is_err());

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn async_stderr_reader_drains_large_child_stderr() -> Result<(), Box<dyn std::error::Error>> {
    let mut child = std::process::Command::new("sh")
        .arg("-c")
        .arg("i=0; while [ $i -lt 10000 ]; do printf 'stderr line %04d\\n' \"$i\" >&2; i=$((i + 1)); done")
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let stderr_reader = child.stderr.take().map(spawn_child_stderr_reader);
    let _status = child.wait()?;
    let stderr = collect_child_stderr(stderr_reader);

    assert!(stderr.len() <= MAX_CHILD_STDERR_BYTES);
    assert!(stderr.contains("stderr line"));
    Ok(())
}

#[test]
fn stream_tool_call_buffer_rejects_oversized_json_prefix() {
    let mut emitter = OpenAiStreamTextEmitter::new("run-1");
    let mut output = Vec::new();
    let oversized = format!(
        "{{\"type\":\"tool_call\",\"padding\":\"{}\"",
        "x".repeat(MAX_STREAM_TOOL_CALL_BUFFER_BYTES)
    );

    let result = emitter.push(&mut output, &oversized);

    assert!(result.is_err());
}

#[test]
fn runner_stdin_reader_accepts_input_at_limit() {
    let input = "x".repeat(MAX_RUNNER_STDIN_INPUT_BYTES);

    let read = read_limited_input_text(
        Cursor::new(input.as_bytes()),
        MAX_RUNNER_STDIN_INPUT_BYTES,
        "stdin exceeds runner input limit",
    );

    assert_eq!(read.unwrap_or_default().len(), MAX_RUNNER_STDIN_INPUT_BYTES);
}

#[test]
fn runner_stdin_reader_rejects_input_over_limit() {
    let input = "x".repeat(MAX_RUNNER_STDIN_INPUT_BYTES + 1);

    let read = read_limited_input_text(
        Cursor::new(input.as_bytes()),
        MAX_RUNNER_STDIN_INPUT_BYTES,
        "stdin exceeds runner input limit",
    );

    assert!(matches!(read, Err(ref error) if error.kind() == std::io::ErrorKind::InvalidData));
}

#[test]
fn agent_tool_process_times_out_instead_of_hanging() {
    let mut command = std::process::Command::new("sh");
    command.arg("-c").arg("sleep 5");
    let started = Instant::now();

    let result = run_agent_tool_process_with_timeout(&mut command, Duration::from_millis(100));

    assert!(matches!(result, Err(ref error) if error.message().contains("timed out")));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn agent_tool_process_returns_when_grandchild_keeps_stdout_open() {
    let mut command = std::process::Command::new("sh");
    command.arg("-c").arg("printf done; (sleep 5) & exit 0");
    let started = Instant::now();

    let output = run_agent_tool_process_with_timeout(&mut command, Duration::from_secs(2));

    assert!(output.is_ok());
    let Ok(output) = output else { return };
    assert!(output.status.success());
    assert_eq!(output.stdout, b"done");
    assert!(started.elapsed() < Duration::from_secs(4));
}

#[test]
fn agent_tool_process_kills_child_after_oversized_output() {
    let mut command = std::process::Command::new("sh");
    let oversized = MAX_AGENT_TOOL_OUTPUT_BYTES + 1;
    command
        .arg("-c")
        .arg(format!("yes x | head -c {oversized}; sleep 5"));
    let started = Instant::now();

    let result = run_agent_tool_process_with_timeout(&mut command, Duration::from_secs(10));

    assert!(matches!(result, Err(ref error) if error.message().contains("tool output exceeds")));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn agent_tool_process_rejects_fast_oversized_output() {
    let mut command = std::process::Command::new("sh");
    let oversized = MAX_AGENT_TOOL_OUTPUT_BYTES + 1;
    command
        .arg("-c")
        .arg(format!("yes x | head -c {oversized}"));

    let result = run_agent_tool_process_with_timeout(&mut command, Duration::from_secs(2));

    assert!(matches!(result, Err(ref error) if error.message().contains("tool output exceeds")));
}

#[test]
fn agent_tool_process_accepts_output_at_limit() {
    let mut command = std::process::Command::new("sh");
    command
        .arg("-c")
        .arg(format!("yes x | head -c {MAX_AGENT_TOOL_OUTPUT_BYTES}"));

    let output = run_agent_tool_process_with_timeout(&mut command, Duration::from_secs(2));

    assert!(output.is_ok());
    let Ok(output) = output else { return };
    assert!(output.status.success());
    assert_eq!(output.stdout.len(), MAX_AGENT_TOOL_OUTPUT_BYTES);
}

#[test]
fn tool_result_truncation_preserves_utf8_boundaries() {
    let oversized = "€".repeat((16 * 1024 / "€".len()) + 1);

    let result = std::panic::catch_unwind(|| trim_tool_result(&oversized));

    assert!(result.is_ok());
    let trimmed = result.unwrap_or_default();
    assert!(trimmed.ends_with("\n[truncated]\n"));
    assert!(trimmed.len() <= MAX_TOOL_RESULT_CHARS);
    assert!(trimmed.is_char_boundary(trimmed.len()));
}

#[test]
fn tool_stdout_accepts_canonical_sdk_success() {
    let output = concat!(
        "{\"type\":\"start\",\"run\":\"r1\",\"tool\":\"example.echo\"}\n",
        "{\"type\":\"message\",\"run\":\"r1\",\"role\":\"tool\",\"content\":[{\"type\":\"text\",\"text\":\"native:one\"}]}\n",
        "{\"type\":\"done\",\"run\":\"r1\",\"status\":\"ok\"}\n",
    );
    assert_eq!(
        parse_tool_stdout(output),
        Ok(ToolStdout::SdkSuccess("native:one".to_owned()))
    );
}

#[test]
fn tool_stdout_maps_canonical_sdk_error_even_with_successful_process_status() {
    let output = concat!(
        "{\"type\":\"start\",\"run\":\"r1\",\"tool\":\"example.echo\"}\n",
        "{\"type\":\"error\",\"run\":\"r1\",\"code\":\"EINVAL\",\"message\":\"bad input\"}\n",
        "{\"type\":\"done\",\"run\":\"r1\",\"status\":\"error\"}\n",
    );
    assert_eq!(
        parse_tool_stdout(output),
        Ok(ToolStdout::SdkError {
            content: String::new(),
            error: "EINVAL: bad input".to_owned(),
        })
    );
}

#[test]
fn tool_stdout_preserves_resource_link_content() {
    let output = concat!(
        "{\"type\":\"start\",\"run\":\"r1\",\"tool\":\"example.read\"}\n",
        "{\"type\":\"message\",\"run\":\"r1\",\"role\":\"tool\",\"content\":[{\"type\":\"resource_link\",\"uri\":\"file:///report.txt\",\"name\":\"report\",\"mimeType\":\"text/plain\"}]}\n",
        "{\"type\":\"done\",\"run\":\"r1\",\"status\":\"ok\"}\n",
    );
    let expected = serde_json::json!([{
        "type": "resource_link",
        "uri": "file:///report.txt",
        "name": "report",
        "mimeType": "text/plain"
    }])
    .to_string();

    assert_eq!(
        parse_tool_stdout(output),
        Ok(ToolStdout::SdkSuccess(expected))
    );
}

#[test]
fn tool_stdout_preserves_mixed_and_annotated_structured_content() {
    let content = serde_json::json!([
        {"type":"text","text":"caption"},
        {"type":"text","text":"annotated","annotations":{"audience":["user"]}},
        {"type":"image","data":"aW1hZ2U=","mimeType":"image/png"},
        {"type":"audio","data":"YXVkaW8=","mimeType":"audio/wav"},
        {"type":"resource","resource":{"uri":"file:///note.txt","text":"note"}}
    ]);
    let output = format!(
        "{{\"type\":\"start\",\"run\":\"r1\",\"tool\":\"example.read\"}}\n{{\"type\":\"message\",\"run\":\"r1\",\"role\":\"tool\",\"content\":{content}}}\n{{\"type\":\"done\",\"run\":\"r1\",\"status\":\"ok\"}}\n"
    );

    assert_eq!(
        parse_tool_stdout(&output),
        Ok(ToolStdout::SdkSuccess(content.to_string()))
    );
}

#[test]
fn tool_stdout_preserves_message_content_before_error() {
    let output = concat!(
        "{\"type\":\"start\",\"run\":\"r1\",\"tool\":\"example.read\"}\n",
        "{\"type\":\"message\",\"run\":\"r1\",\"role\":\"tool\",\"content\":[{\"type\":\"text\",\"text\":\"remote detail\"}]}\n",
        "{\"type\":\"error\",\"run\":\"r1\",\"code\":\"EIO\",\"message\":\"remote MCP tool returned an error\"}\n",
        "{\"type\":\"done\",\"run\":\"r1\",\"status\":\"error\"}\n",
    );

    assert_eq!(
        parse_tool_stdout(output),
        Ok(ToolStdout::SdkError {
            content: "remote detail".to_owned(),
            error: "EIO: remote MCP tool returned an error".to_owned(),
        })
    );
}

#[test]
fn tool_stdout_rejects_malformed_content_item() {
    let output = concat!(
        "{\"type\":\"start\",\"run\":\"r1\",\"tool\":\"example.read\"}\n",
        "{\"type\":\"message\",\"run\":\"r1\",\"role\":\"tool\",\"content\":[{\"text\":\"missing type\"}]}\n",
        "{\"type\":\"done\",\"run\":\"r1\",\"status\":\"ok\"}\n",
    );

    assert_eq!(
        parse_tool_stdout(output),
        Err(ExecError::new("invalid CortexFS Tool SDK content item"))
    );
}

#[test]
fn tool_stdout_rejects_malformed_sdk_after_start() {
    let output = concat!(
        "{\"type\":\"start\",\"run\":\"r1\",\"tool\":\"example.echo\"}\n",
        "not-json\n",
    );
    assert!(parse_tool_stdout(output).is_err());
}

#[test]
fn tool_stdout_rejects_sdk_run_mismatch_and_missing_done() {
    let mismatch = concat!(
        "{\"type\":\"start\",\"run\":\"r1\",\"tool\":\"example.echo\"}\n",
        "{\"type\":\"done\",\"run\":\"r2\",\"status\":\"ok\"}\n",
    );
    let missing_done = concat!(
        "{\"type\":\"start\",\"run\":\"r1\",\"tool\":\"example.echo\"}\n",
        "{\"type\":\"message\",\"run\":\"r1\",\"role\":\"tool\",\"content\":[]}\n",
    );
    assert!(parse_tool_stdout(mismatch).is_err());
    assert!(parse_tool_stdout(missing_done).is_err());
}

#[test]
fn tool_stdout_rejects_non_sdk_output() {
    assert!(parse_tool_stdout("").is_err());
    assert!(parse_tool_stdout("plain output\n").is_err());
    assert!(parse_tool_stdout("{\"type\":\"message\"}\n").is_err());
}

#[test]
fn agent_tool_timeout_env_is_bounded() {
    assert_eq!(
        agent_tool_timeout_seconds_from_env(|_| Some("45".to_owned())),
        45
    );
    assert_eq!(
        agent_tool_timeout_seconds_from_env(|_| Some("0".to_owned())),
        20
    );
    assert_eq!(
        agent_tool_timeout_seconds_from_env(|_| Some("999999".to_owned())),
        20
    );
    assert_eq!(
        agent_tool_timeout_seconds_from_env(|_| Some("bad".to_owned())),
        20
    );
    assert_eq!(agent_tool_timeout_seconds_from_env(|_| None), 20);
}

#[test]
fn agent_model_timeout_env_is_bounded() {
    assert_eq!(
        agent_model_timeout_seconds_from_env(|_| Some("45".to_owned())),
        45
    );
    assert_eq!(
        agent_model_timeout_seconds_from_env(|_| Some("0".to_owned())),
        120
    );
    assert_eq!(
        agent_model_timeout_seconds_from_env(|_| Some("999999".to_owned())),
        120
    );
    assert_eq!(
        agent_model_timeout_seconds_from_env(|_| Some("bad".to_owned())),
        120
    );
    assert_eq!(agent_model_timeout_seconds_from_env(|_| None), 120);
}

#[test]
fn agent_tsh_args_reject_root_override() {
    assert_eq!(
        validate_agent_tsh_args(&[
            OsString::from("--root"),
            OsString::from("/tmp/fakectx"),
            OsString::from("evil"),
        ]),
        Err(ExecError::new("tool_call args cannot override tsh root"))
    );
    assert_eq!(
        validate_agent_tsh_args(&[
            OsString::from("-r"),
            OsString::from("/tmp/fakectx"),
            OsString::from("evil"),
        ]),
        Err(ExecError::new("tool_call args cannot override tsh root"))
    );
}

#[test]
fn agent_tsh_args_reject_empty_args() {
    assert_eq!(
        validate_agent_tsh_args(&[]),
        Err(ExecError::new("tool_call args for tsh cannot be empty"))
    );
}

#[test]
fn agent_tsh_args_reject_recursive_tsh_program_name() {
    assert_eq!(
        validate_agent_tsh_args(&[OsString::from("tsh")]),
        Err(ExecError::new(
            "tool_call args for tsh must not include the tsh program name"
        ))
    );
    assert_eq!(
        validate_agent_tsh_args(&[OsString::from("tsh"), OsString::from("tools")]),
        Err(ExecError::new(
            "tool_call args for tsh must not include the tsh program name"
        ))
    );
}

#[test]
fn agent_tsh_args_allow_tool_arguments_after_tool_name() {
    assert_eq!(
        validate_agent_tsh_args(&[
            OsString::from("fs.read"),
            OsString::from("--root"),
            OsString::from("README.md"),
        ]),
        Ok(())
    );
}

#[test]
fn execute_agent_tsh_call_rejects_empty_args() -> Result<(), Box<dyn std::error::Error>> {
    let (root, tool_control) = agent_tool_fixture("atl-empty-args", "tsh")?;
    fs::write(
        tool_control.join("policy"),
        "allow executor_t tool:tsh execute\n",
    )?;

    let executed = root.join("tsh-called");
    write_executable_script(
        &root.join("tool").join("tsh"),
        format!("#!/bin/sh\ntouch {}\n", executed.display()),
    )?;

    let config = AgentModelRunConfig {
        ctx_root: root.clone(),
        source: root.clone(),
        ..test_agent_run_config()
    };
    let call = AgentToolCall {
        id: "call-1".to_owned(),
        name: "tsh".to_owned(),
        args: Vec::new(),
    };
    let result = execute_prepared_agent_tool_call(&config, &call);

    assert_eq!(
        result,
        Err(ExecError::new("tool_call args for tsh cannot be empty"))
    );
    assert!(!executed.exists());

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

fn contains_os_arg_pair(args: &[OsString], first: &str, second: &str) -> bool {
    args.windows(2).any(|window| {
        window.first().is_some_and(|arg| arg == first)
            && window.get(1).is_some_and(|arg| arg == second)
    })
}

fn contains_os_arg_triplet(args: &[OsString], first: &str, second: &str, third: &str) -> bool {
    args.windows(3).any(|window| {
        window.first().is_some_and(|arg| arg == first)
            && window.get(1).is_some_and(|arg| arg == second)
            && window.get(2).is_some_and(|arg| arg == third)
    })
}

fn find_overlay_generated_file(root: &Path) -> std::io::Result<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.file_name().and_then(|name| name.to_str()) == Some("generated.txt") {
                return Ok(path);
            }
            if entry.file_type()?.is_dir() {
                stack.push(path);
            }
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "generated.txt not found in overlay upper",
    ))
}
use super::runtime::test_agent_run_config;
use super::*;
use crate::object::executor::exec::{
    ToolStdout, authorized_tool_target, finish_agent_tool_output, parse_tool_stdout,
    tool_spawn_error,
};
use std::process::Command;

#[test]
fn passthrough_tool_output_accepts_raw_stdout_and_reports_stderr()
-> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("sh")
        .arg("-c")
        .arg("printf 'workspace\\n'")
        .output()?;
    assert_eq!(finish_agent_tool_output(&output, "tsh")?, "workspace\n");

    let output = Command::new("sh")
        .arg("-c")
        .arg("printf 'missing tool\\n' >&2; exit 7")
        .output()?;
    let error = finish_agent_tool_output(&output, "tsh")
        .err()
        .ok_or_else(|| std::io::Error::other("failed tsh must be reported"))?;
    assert!(error.message().contains("missing tool"));
    assert!(error.message().contains("exit status: 7"));
    Ok(())
}

#[test]
fn authorized_tool_target_maps_backing_source_tiers_under_ctx() {
    let source = Path::new("/var/lib/cortexfs/generation");
    let system = cortexfs::ToolHit::new(source.join("tool/system"));
    let user = cortexfs::ToolHit::new(source.join("home/42/tool/user"));

    assert_eq!(
        authorized_tool_target(source, &system),
        PathBuf::from("/ctx/tool/system")
    );
    assert_eq!(
        authorized_tool_target(source, &user),
        PathBuf::from("/ctx/home/42/tool/user")
    );
}

#[test]
fn tool_spawn_error_names_missing_sandbox_helper() {
    let missing = std::io::Error::new(std::io::ErrorKind::NotFound, "No such file");
    let other = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
    assert_eq!(
        tool_spawn_error(OsStr::new(BWRAP_PROGRAM), &missing).message(),
        format!("sandbox helper missing: {BWRAP_PROGRAM}")
    );
    assert_eq!(
        tool_spawn_error(OsStr::new("/bin/echo"), &missing).message(),
        missing.to_string()
    );
    assert_eq!(
        tool_spawn_error(OsStr::new(BWRAP_PROGRAM), &other).message(),
        other.to_string()
    );
}

#[test]
fn authorized_tool_target_preserves_absolute_projected_tiers() {
    let source = Path::new("/var/lib/cortexfs/generation");
    let user = cortexfs::ToolHit::new(PathBuf::from("/ctx/home/42/tool/user-only"));
    let shared = cortexfs::ToolHit::new(PathBuf::from("/ctx/shared/team/tool/shared"));

    assert_eq!(
        authorized_tool_target(source, &user),
        PathBuf::from("/ctx/home/42/tool/user-only")
    );
    assert_eq!(
        authorized_tool_target(source, &shared),
        PathBuf::from("/ctx/shared/team/tool/shared")
    );
    assert_ne!(
        authorized_tool_target(source, &shared),
        PathBuf::from("/ctx/home/42/tool/shared")
    );
}
