#[test]
fn agent_tool_call_executes_visible_tsh_for_search_and_load() -> Result<(), Box<dyn std::error::Error>>
{
    let root = short_unique_temp_path("atl");
    let _ignored = fs::remove_dir_all(&root);
    let control = root.join("agent").join("coder.d");
    let tool_control = root.join("tool").join("tsh.d");
    fs::create_dir_all(&control)?;
    fs::create_dir_all(&tool_control)?;
    fs::create_dir_all(root.join("tool"))?;
    fs::write(control.join("owner"), "1000\n")?;
    fs::write(control.join("uid"), "1000\n")?;
    fs::write(control.join("gid"), "1000\n")?;
    fs::write(control.join("groups"), "1000\n")?;
    fs::write(control.join("label"), "user_u:agent_r:coder_t:s0\n")?;
    fs::write(control.join("iso"), "shared\n")?;
    fs::write(control.join("parent"), "\n")?;
    fs::write(control.join("life"), "owned\n")?;
    fs::write(control.join("root"), "/ctx/home/1000/agent/coder/root\n")?;
    fs::write(control.join("cwd"), "/workspace\n")?;
    fs::write(control.join("env"), "\n")?;
    fs::write(control.join("model"), "main\n")?;
    fs::write(control.join("status"), "idle\n")?;
    fs::write(control.join("pid"), "\n")?;
    fs::write(control.join("log"), "\n")?;
    fs::write(control.join("meta.json"), "{}\n")?;
    fs::write(control.join("path"), format!("{}\n", root.join("tool").display()))?;
    fs::write(
        control.join("mount"),
        format!(
            "{}\t{}\tro\trbind,nosuid,nodev\n",
            root.display(),
            root.display()
        ),
    )?;
    fs::write(
        control.join("policy"),
        "allow coder_t model:main use\nallow coder_t tool:tsh execute\n",
    )?;
    fs::write(
        tool_control.join("policy"),
        "allow coder_t tool:tsh execute\n",
    )?;
    fs::write(
        root.join("tool").join("tsh"),
        r#"#!/bin/sh
case "$1" in
  tools)
    printf 'fs.read\nshell.exec\ntsh\n'
    ;;
  load)
    if [ "$2" = "fs.read" ]; then
      printf 'loaded fs.read\t%s/tool/fs.read\tmetadata\n' "$CTX_ROOT"
    else
      printf 'unknown tool: %s\n' "$2" >&2
      exit 2
    fi
    ;;
  *)
    printf 'unexpected tsh args: %s %s\n' "$1" "$2" >&2
    exit 2
    ;;
esac
"#,
    )?;
    fs::set_permissions(root.join("tool").join("tsh"), fs::Permissions::from_mode(0o755))?;
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
    let search_result = execute_agent_tool_call(&config, &search)?;

    assert!(search_result.contains("fs.read"));
    assert!(search_result.contains("tsh"));

    let load = AgentToolCall {
        id: "call-2".to_owned(),
        name: "tsh".to_owned(),
        args: vec![OsString::from("load"), OsString::from("fs.read")],
    };
    let load_result = execute_agent_tool_call(&config, &load)?;

    assert!(load_result.contains("loaded fs.read"));
    assert!(load_result.contains("/tool/fs.read"));
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn agent_tool_bwrap_args_use_overlay_workspace_upper() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("tool-overlay-args")?;
    let workspace = root.join("workspace");
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
        "{}\t/ctx\tro\trbind,nosuid,nodev\n{}\t/workspace\trw\trbind,nosuid,nodev\n",
        config.source.display(),
        workspace.display()
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
        config: &config,
        tool_executable: Path::new("/proc/self/fd/9"),
        tool_args: &[OsString::from("tools")],
        env: &env,
        mount_table: &mount_table,
        cwd: Path::new("/workspace"),
        sandbox: Some(&sandbox),
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
    assert!(!args
        .iter()
        .any(|arg| arg == "CTX_PROVIDER_SECRET_PROVIDER"));
    assert!(!args.iter().any(|arg| arg == "CTX_PROVIDER_SECRET_SLOT"));
    assert!(contains_os_arg_pair(&args, "--chdir", "/workspace"));

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn agent_tool_bwrap_exec_writes_workspace_overlay_upper() -> Result<(), Box<dyn std::error::Error>>
{
    let root = short_unique_temp_path("atl-overlay-write");
    let _ignored = fs::remove_dir_all(&root);
    let workspace = root.join("workspace-lower");
    let source = root.join("source");
    let upper = root.join("upper");
    let work = root.join("work");
    fs::create_dir_all(&source)?;
    fs::create_dir_all(&workspace)?;
    fs::create_dir_all(&upper)?;
    fs::create_dir_all(&work)?;
    fs::write(workspace.join("README.md"), "lower\n")?;
    let tool = root.join("write-workspace");
    write_executable_script(
        &tool,
        "#!/bin/sh\nprintf upper-write > /workspace/generated.txt\nprintf ok\n",
    )?;
    let tool_executable = open_executable_no_follow(&tool)?;
    let config = AgentModelRunConfig {
        ctx_root: root.clone(),
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
    let args = agent_tool_bwrap_args(&AgentToolBwrapArgs {
        config: &config,
        tool_executable: &proc_fd_path(&tool_executable),
        tool_args: &[],
        env: &[],
        mount_table: &mount_table,
        cwd: Path::new("/workspace"),
        sandbox: Some(&sandbox),
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

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn visible_workspace_source_falls_back_to_current_namespace_workspace() {
    let missing = PathBuf::from("/tmp/cortexfs-missing-workspace-for-test");
    if Path::new("/workspace").exists() {
        assert_eq!(visible_workspace_source(missing), PathBuf::from("/workspace"));
    }
}

#[test]
fn agent_tool_call_refuses_symlinked_tsh_policy() -> Result<(), Box<dyn std::error::Error>> {
    let root = short_unique_temp_path("atl-policy-symlink");
    let _ignored = fs::remove_dir_all(&root);
    let control = root.join("agent").join("coder.d");
    let tool_control = root.join("tool").join("tsh.d");
    fs::create_dir_all(&control)?;
    fs::create_dir_all(&tool_control)?;
    fs::create_dir_all(root.join("tool"))?;
    fs::write(control.join("owner"), "1000\n")?;
    fs::write(control.join("uid"), "1000\n")?;
    fs::write(control.join("gid"), "1000\n")?;
    fs::write(control.join("groups"), "1000\n")?;
    fs::write(control.join("label"), "user_u:agent_r:coder_t:s0\n")?;
    fs::write(control.join("iso"), "shared\n")?;
    fs::write(control.join("parent"), "\n")?;
    fs::write(control.join("life"), "owned\n")?;
    fs::write(control.join("root"), "/ctx/home/1000/agent/coder/root\n")?;
    fs::write(control.join("cwd"), "/workspace\n")?;
    fs::write(control.join("env"), "\n")?;
    fs::write(control.join("model"), "main\n")?;
    fs::write(control.join("status"), "idle\n")?;
    fs::write(control.join("pid"), "\n")?;
    fs::write(control.join("log"), "\n")?;
    fs::write(control.join("meta.json"), "{}\n")?;
    fs::write(control.join("path"), format!("{}\n", root.join("tool").display()))?;
    fs::write(
        control.join("mount"),
        format!(
            "{}\t{}\tro\trbind,nosuid,nodev\n",
            root.display(),
            root.display()
        ),
    )?;
    fs::write(
        control.join("policy"),
        "allow coder_t model:main use\nallow coder_t tool:tsh execute\n",
    )?;
    let outside_policy = root.join("outside-policy");
    fs::write(&outside_policy, "allow coder_t tool:tsh execute\n")?;
    symlink(&outside_policy, tool_control.join("policy"))?;
    fs::write(root.join("tool").join("tsh"), "#!/bin/sh\nexit 0\n")?;
    fs::set_permissions(root.join("tool").join("tsh"), fs::Permissions::from_mode(0o755))?;
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
    let result = execute_agent_tool_call(&config, &call);

    assert!(matches!(result, Err(ref error) if error.contains("cannot read")));
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

    let read = read_runner_stdin_limited(Cursor::new(input.as_bytes()), MAX_RUNNER_STDIN_INPUT_BYTES);

    assert_eq!(read.unwrap_or_default().len(), MAX_RUNNER_STDIN_INPUT_BYTES);
}

#[test]
fn runner_stdin_reader_rejects_input_over_limit() {
    let input = "x".repeat(MAX_RUNNER_STDIN_INPUT_BYTES + 1);

    let read = read_runner_stdin_limited(Cursor::new(input.as_bytes()), MAX_RUNNER_STDIN_INPUT_BYTES);

    assert!(matches!(read, Err(ref error) if error.kind() == std::io::ErrorKind::InvalidData));
}

#[test]
fn agent_tool_process_times_out_instead_of_hanging() {
    let mut command = std::process::Command::new("sh");
    command.arg("-c").arg("sleep 5");
    let started = Instant::now();

    let result = run_agent_tool_process_with_timeout(&mut command, Duration::from_millis(100));

    assert!(matches!(result, Err(ref error) if error.contains("timed out")));
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
    assert!(started.elapsed() < Duration::from_secs(2));
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

    assert!(matches!(result, Err(ref error) if error.contains("tool output exceeds")));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn agent_tool_process_rejects_fast_oversized_output() {
    let mut command = std::process::Command::new("sh");
    let oversized = MAX_AGENT_TOOL_OUTPUT_BYTES + 1;
    command.arg("-c").arg(format!("yes x | head -c {oversized}"));

    let result = run_agent_tool_process_with_timeout(&mut command, Duration::from_secs(2));

    assert!(matches!(result, Err(ref error) if error.contains("tool output exceeds")));
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
        Err("tool_call args cannot override tsh root".to_owned())
    );
    assert_eq!(
        validate_agent_tsh_args(&[
            OsString::from("-r"),
            OsString::from("/tmp/fakectx"),
            OsString::from("evil"),
        ]),
        Err("tool_call args cannot override tsh root".to_owned())
    );
}

#[test]
fn agent_tsh_args_reject_empty_args() {
    assert_eq!(
        validate_agent_tsh_args(&[]),
        Err("tool_call args for tsh cannot be empty".to_owned())
    );
}

#[test]
fn agent_tsh_args_reject_recursive_tsh_program_name() {
    assert_eq!(
        validate_agent_tsh_args(&[OsString::from("tsh")]),
        Err("tool_call args for tsh must not include the tsh program name".to_owned())
    );
    assert_eq!(
        validate_agent_tsh_args(&[OsString::from("tsh"), OsString::from("tools")]),
        Err("tool_call args for tsh must not include the tsh program name".to_owned())
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
    let root = short_unique_temp_path("atl-empty-args");
    let _ignored = fs::remove_dir_all(&root);
    let control = root.join("agent").join("coder.d");
    let tool_control = root.join("tool").join("tsh.d");
    fs::create_dir_all(&control)?;
    fs::create_dir_all(&tool_control)?;
    fs::create_dir_all(root.join("tool"))?;
    fs::write(control.join("owner"), "1000\n")?;
    fs::write(control.join("uid"), "1000\n")?;
    fs::write(control.join("gid"), "1000\n")?;
    fs::write(control.join("groups"), "1000\n")?;
    fs::write(control.join("label"), "user_u:agent_r:coder_t:s0\n")?;
    fs::write(control.join("iso"), "shared\n")?;
    fs::write(control.join("parent"), "\n")?;
    fs::write(control.join("life"), "owned\n")?;
    fs::write(control.join("root"), "/ctx/home/1000/agent/coder/root\n")?;
    fs::write(control.join("cwd"), "/workspace\n")?;
    fs::write(control.join("env"), "\n")?;
    fs::write(control.join("model"), "main\n")?;
    fs::write(control.join("status"), "idle\n")?;
    fs::write(control.join("pid"), "\n")?;
    fs::write(control.join("log"), "\n")?;
    fs::write(control.join("meta.json"), "{}\n")?;
    fs::write(control.join("path"), format!("{}\n", root.join("tool").display()))?;
    fs::write(
        control.join("mount"),
        format!(
            "{}\t{}\tro\trbind,nosuid,nodev\n",
            root.display(),
            root.display()
        ),
    )?;
    fs::write(
        control.join("policy"),
        "allow coder_t model:main use\nallow coder_t tool:tsh execute\n",
    )?;
    fs::write(
        tool_control.join("policy"),
        "allow coder_t tool:tsh execute\n",
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
    let result = execute_agent_tool_call(&config, &call);

    assert_eq!(
        result,
        Err("tool_call args for tsh cannot be empty".to_owned())
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
