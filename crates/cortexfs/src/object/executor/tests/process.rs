#[test]
fn agent_model_process_gets_clean_runtime_environment() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("CORTEXFS_ENV_CHILD").is_none() {
        let output = std::process::Command::new(std::env::current_exe()?)
            .arg("--exact")
            .arg("object::executor::tests::process::agent_model_process_gets_clean_runtime_environment")
            .arg("--nocapture")
            .env("CORTEXFS_ENV_CHILD", "1")
            .env("CORTEXFS_SHOULD_NOT_LEAK", "secret")
            .env("CTX_PROVIDER_SECRET_PROVIDER", "fixture")
            .env("CTX_PROVIDER_SECRET_SLOT", "default")
            .env("CTX_PROVIDER_SECRET_PATH", "/tmp/runtime-secret")
            .env("CTX_PROVIDER_CONFIG_DIR", "/tmp/providers.d")
            .output()?;
        assert!(
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains(
                    "object::executor::tests::process::agent_model_process_gets_clean_runtime_environment",
                ),
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
if [ "$CTX_PROVIDER_CONFIG_DIR" != "/tmp/providers.d" ]; then
    printf '{"type":"error","run":"%s","code":"EIO","message":"missing provider config env"}\n' "$CTX_RUN_ID"
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
    assert!(
        outcome
            .frames
            .iter()
            .any(|frame| frame.contains(r#""text":"clean""#))
    );
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

    let outcome = run_agent_model_once_with_timeout(
        &config,
        "hello",
        &mut output,
        Duration::from_millis(100),
    )?;

    assert!(!outcome.success);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(
        outcome
            .frames
            .iter()
            .any(|frame| frame.contains(r#""code":"ETIMEDOUT""#))
    );
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
    assert_model_overflow(
        "agent_model_output_rejects_oversized_frame",
        &format!(
            "#!/bin/sh\nhead -c {MAX_AGENT_MODEL_FRAME_BYTES} /dev/zero | tr '\\0' x\nprintf '\\n'\n"
        ),
    )
}

#[test]
fn agent_model_output_rejects_too_many_frames() -> Result<(), Box<dyn std::error::Error>> {
    assert_model_overflow(
        "agent_model_output_rejects_too_many_frames",
        &format!(
            "#!/bin/sh\ni=0\nwhile [ \"$i\" -le {MAX_AGENT_MODEL_FRAMES} ]; do printf '{{\"type\":\"delta\",\"run\":\"%s\",\"text\":\"x\"}}\\n' \"$CTX_RUN_ID\"; i=$((i + 1)); done\n"
        ),
    )
}

#[test]
fn agent_model_output_rejects_aggregate_overflow() -> Result<(), Box<dyn std::error::Error>> {
    assert_model_overflow(
        "agent_model_output_rejects_aggregate_overflow",
        "#!/bin/sh\npayload=$(head -c 131072 /dev/zero | tr '\\0' x)\ni=0\nwhile [ \"$i\" -le 64 ]; do printf '{\"type\":\"delta\",\"run\":\"%s\",\"text\":\"%s\"}\\n' \"$CTX_RUN_ID\" \"$payload\"; i=$((i + 1)); done\n",
    )
}

fn assert_model_overflow(case: &str, script: &str) -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir(case)?;
    let model = root.join("model-script");
    write_executable_script(&model, script)?;
    let config = AgentModelRunConfig {
        model_path: model,
        ..test_agent_run_config()
    };
    let mut output = Vec::new();
    let outcome = run_agent_model_once(&config, "hello", &mut output)?;
    assert!(
        !outcome.success,
        "{case}: overflow run unexpectedly succeeded"
    );
    assert!(
        outcome
            .frames
            .iter()
            .any(|frame| frame.contains(r#""code":"EOVERFLOW""#)),
        "{case}: frames missing EOVERFLOW"
    );
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(
        output.contains(r#""code":"EOVERFLOW""#),
        "{case}: output missing EOVERFLOW"
    );
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn oversized_prompt_returns_e2big_before_opening_model() -> Result<(), ExecError> {
    let mut config = test_agent_run_config();
    config.model_path = PathBuf::from("/definitely/not/a/model");
    config.context_budget =
        budget_from_effective(ModelContextLimit::known(1).unwrap_or(ModelContextLimit::Unknown));
    config.suppress_model_error_events = true;
    let mut output = Vec::new();

    let outcome = run_agent_model_once(&config, "too large", &mut output)?;

    assert!(!outcome.success);
    assert!(
        outcome
            .frames
            .iter()
            .any(|frame| frame.contains(r#""code":"E2BIG""#))
    );
    assert!(output.is_empty());
    Ok(())
}

#[test]
fn model_child_receives_context_window_pair_only_when_known()
-> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("model-context-env")?;
    let model = root.join("model");
    write_executable_script(
        &model,
        r#"#!/bin/sh
if [ "$1" = known ]; then
  [ "$CTX_CONTEXT_WINDOW_TOKENS" = 4096 ] && [ "$CTX_CONTEXT_WINDOW_CHARS" = 16384 ] && [ "$CTX_AGENT_WINDOW_SETTING" = auto ] || exit 2
else
  [ -z "${CTX_CONTEXT_WINDOW_TOKENS+x}" ] && [ -z "${CTX_CONTEXT_WINDOW_CHARS+x}" ] && [ "$CTX_AGENT_WINDOW_SETTING" = auto ] || exit 3
fi
printf '{"type":"done","run":"%s","status":"ok"}\n' "$CTX_RUN_ID"
"#,
    )?;
    let mut config = AgentModelRunConfig {
        model_path: model,
        ..test_agent_run_config()
    };
    config.context_budget = budget_from_effective(
        ModelContextLimit::known(4_096).unwrap_or(ModelContextLimit::Unknown),
    );
    let mut output = Vec::new();
    assert!(run_agent_model_once(&config, "known", &mut output)?.success);
    config.context_budget = None;
    assert!(run_agent_model_once(&config, "unknown", &mut output)?.success);
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}
use super::runtime::test_agent_run_config;
use super::*;
#[cfg(unix)]
#[test]
fn agent_model_command_forwards_only_exact_provider_egress_dir()
-> Result<(), Box<dyn std::error::Error>> {
    const CHILD_ENV: &str = "CORTEXFS_TEST_AGENT_MODEL_EGRESS_ENV";
    if let Some(expected) = std::env::var_os(CHILD_ENV) {
        let executable = fs::File::open(std::env::current_exe()?)?;
        let command = agent_model_command(&test_agent_run_config(), "input", &executable);
        let actual = command
            .get_envs()
            .find_map(|(name, value)| {
                (name == cortexfs::runtime::egress::PROVIDER_EGRESS_DIR_ENV)
                    .then(|| value.map(OsStr::to_owned))
            })
            .flatten();
        let expected = (expected == "exact")
            .then(|| OsString::from(cortexfs::runtime::egress::PROVIDER_EGRESS_SANDBOX_PATH));
        assert_eq!(actual, expected);
        return Ok(());
    }

    for (case, value) in [
        (
            "exact",
            Some(cortexfs::runtime::egress::PROVIDER_EGRESS_SANDBOX_PATH),
        ),
        ("missing", None),
        ("drift", Some("/run/cortexfs/attacker-controlled")),
    ] {
        let mut child = std::process::Command::new(std::env::current_exe()?);
        child
            .arg("agent_model_command_forwards_only_exact_provider_egress_dir")
            .arg("--nocapture")
            .env(CHILD_ENV, case);
        if let Some(value) = value {
            child.env(cortexfs::runtime::egress::PROVIDER_EGRESS_DIR_ENV, value);
        } else {
            child.env_remove(cortexfs::runtime::egress::PROVIDER_EGRESS_DIR_ENV);
        }
        assert!(child.status()?.success(), "failed environment case: {case}");
    }
    Ok(())
}
