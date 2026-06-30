#[test]
fn agent_model_process_gets_clean_runtime_environment() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("CORTEXFS_ENV_CHILD").is_none() {
        let output = std::process::Command::new(std::env::current_exe()?)
            .arg("--exact")
            .arg("tests::agent_model_process_gets_clean_runtime_environment")
            .arg("--nocapture")
            .env("CORTEXFS_ENV_CHILD", "1")
            .env("CORTEXFS_SHOULD_NOT_LEAK", "secret")
            .env("CTX_PROVIDER_SECRET_PROVIDER", "fixture")
            .env("CTX_PROVIDER_SECRET_SLOT", "default")
            .env("CTX_PROVIDER_SECRET_PATH", "/tmp/runtime-secret")
            .output()?;
        assert!(
            output.status.success(),
            "child test failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return Ok(());
    }

    let root = unique_temp_dir("runner-agent-model-env")?;
    let model = root.join("model-script");
    write_executable_script(
        &model,
        r#"#!/bin/sh
if [ -n "$CORTEXFS_SHOULD_NOT_LEAK" ]; then
  printf '{"type":"error","run":"%s","code":"EIO","message":"inherited parent env"}\n' "$CTX_RUN_ID"
  exit 2
fi
if [ "$PATH" != "/usr/bin:/bin" ]; then
  printf '{"type":"error","run":"%s","code":"EIO","message":"unexpected path"}\n' "$CTX_RUN_ID"
  exit 2
fi
if [ "$CTX_PROVIDER_SECRET_PROVIDER" != "fixture" ] || [ "$CTX_PROVIDER_SECRET_SLOT" != "default" ] || [ "$CTX_PROVIDER_SECRET_PATH" != "/tmp/runtime-secret" ]; then
  printf '{"type":"error","run":"%s","code":"EIO","message":"missing runtime provider secret env"}\n' "$CTX_RUN_ID"
  exit 2
fi
if [ "$CTX_AGENT_HISTORY_MESSAGES" != "previous message" ]; then
  printf '{"type":"error","run":"%s","code":"EIO","message":"missing history env"}\n' "$CTX_RUN_ID"
  exit 2
fi
printf '{"type":"delta","run":"%s","text":"clean"}\n' "$CTX_RUN_ID"
printf '{"type":"done","run":"%s","status":"ok"}\n' "$CTX_RUN_ID"
"#,
    )?;
    let config = AgentModelRunConfig {
        model_path: model,
        ..test_agent_run_config()
    };
    let mut output = Vec::new();

    let result = run_agent_model_once(&config, "hello", &mut output);

    let outcome = result?;
    assert!(outcome.success);
    assert!(outcome.frames.iter().any(|frame| frame.contains(r#""text":"clean""#)));
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}
#[test]
fn agent_model_process_rejects_symlink_executable_without_running_target()
-> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-agent-model-symlink-executable")?;
    let target = root.join("target-script");
    let model = root.join("model-script");
    let marker = root.join("target-ran");
    fs::write(
        &target,
        format!(
            "#!/bin/sh\nprintf ran > '{}'\nprintf '{{\"type\":\"done\",\"run\":\"%s\",\"status\":\"ok\"}}\\n' \"$CTX_RUN_ID\"\n",
            marker.display()
        ),
    )?;
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755))?;
    symlink(&target, &model)?;
    let config = AgentModelRunConfig {
        model_path: model,
        ..test_agent_run_config()
    };
    let mut output = Vec::new();

    let result = run_agent_model_once(&config, "hello", &mut output);

    assert!(result.is_err());
    assert!(!marker.exists(), "symlink target was executed");
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn agent_model_process_timeout_kills_process_group() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-agent-model-timeout")?;
    let model = root.join("model-script");
    let marker = root.join("child-survived");
    write_executable_script(
        &model,
        format!(
            "#!/bin/sh\n(trap '' TERM; sleep 1; printf alive > '{}') &\nsleep 5\n",
            marker.display()
        ),
    )?;
    let config = AgentModelRunConfig {
        model_path: model,
        ..test_agent_run_config()
    };
    let mut output = Vec::new();
    let started = Instant::now();

    let outcome =
        run_agent_model_once_with_timeout(&config, "hello", &mut output, Duration::from_millis(100))?;

    assert!(!outcome.success);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(outcome.frames.iter().any(|frame| frame.contains(r#""code":"ETIMEDOUT""#)));
    thread::sleep(Duration::from_millis(1500));
    assert!(
        !marker.exists(),
        "model timeout killed only the parent process; child process survived"
    );
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""code":"ETIMEDOUT""#));
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn agent_model_output_rejects_oversized_frame() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-agent-model-oversized-frame")?;
    let model = root.join("model-script");
    write_executable_script(
        &model,
        format!(
            "#!/bin/sh\nhead -c {MAX_AGENT_MODEL_FRAME_BYTES} /dev/zero | tr '\\0' x\nprintf '\\n'\n"
        ),
    )?;
    let config = AgentModelRunConfig {
        model_path: model,
        ..test_agent_run_config()
    };
    let mut output = Vec::new();

    let outcome = run_agent_model_once(&config, "hello", &mut output)?;

    assert!(!outcome.success);
    assert!(outcome.frames.iter().any(|frame| frame.contains(r#""code":"EOVERFLOW""#)));
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""code":"EOVERFLOW""#));
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn agent_model_output_rejects_too_many_frames() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-agent-model-too-many-frames")?;
    let model = root.join("model-script");
    write_executable_script(
        &model,
        format!(
            "#!/bin/sh\ni=0\nwhile [ \"$i\" -le {MAX_AGENT_MODEL_FRAMES} ]; do printf '{{\"type\":\"delta\",\"run\":\"%s\",\"text\":\"x\"}}\\n' \"$CTX_RUN_ID\"; i=$((i + 1)); done\n"
        ),
    )?;
    let config = AgentModelRunConfig {
        model_path: model,
        ..test_agent_run_config()
    };
    let mut output = Vec::new();

    let outcome = run_agent_model_once(&config, "hello", &mut output)?;

    assert!(!outcome.success);
    assert!(outcome.frames.iter().any(|frame| frame.contains(r#""code":"EOVERFLOW""#)));
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""code":"EOVERFLOW""#));
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}
