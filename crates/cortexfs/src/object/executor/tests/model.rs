#[test]
fn object_path_parses_class_and_name() {
    assert_eq!(
        ObjectPath::parse(Path::new("/ctx/model/debug/echo")),
        Ok(ObjectPath {
            class: "model".to_owned(),
            name: "debug/echo".to_owned(),
        })
    );
}
#[test]
fn object_args_use_exec_metadata_for_proc_fd_wrappers() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-object-path-proc-fd")?;
    let wrapper = root.join("coder");
    fs::write(
        &wrapper,
        "#!/usr/bin/cortexfs-object-runner\n# cortexfs.object=agent\n# cortexfs.name=coder\n",
    )?;
    let file = fs::File::open(&wrapper)?;
    let fd_path = PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()));

    let (path, rest) = split_object_args(vec![fd_path.into_os_string(), OsString::from("hello")])?;

    assert_eq!(path, PathBuf::from("/ctx/agent/coder"));
    assert_eq!(rest, vec![OsString::from("hello")]);
    assert_eq!(
        ObjectPath::parse(&path),
        Ok(ObjectPath {
            class: "agent".to_owned(),
            name: "coder".to_owned(),
        })
    );

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn object_args_reject_exec_metadata_that_disagrees_with_authorized_object()
-> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("CORTEXFS_RUNNER_METADATA_MISMATCH_CHILD").is_none() {
        let output = std::process::Command::new(std::env::current_exe()?)
            .arg("--exact")
            .arg("object::executor::tests::model::object_args_reject_exec_metadata_that_disagrees_with_authorized_object")
            .arg("--nocapture")
            .env("CORTEXFS_RUNNER_METADATA_MISMATCH_CHILD", "1")
            .env("CTX_AUTHORIZED_OBJECT", "/ctx/tool/safe")
            .output()?;
        assert!(
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains(
                    "object::executor::tests::model::object_args_reject_exec_metadata_that_disagrees_with_authorized_object",
                ),
            "child test failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return Ok(());
    }

    let root = unique_temp_dir("runner-object-path-mismatch")?;
    let wrapper = root.join("safe");
    fs::write(
        &wrapper,
        "#!/usr/bin/cortexfs-object-runner\n# cortexfs.object=tool\n# cortexfs.name=bash\n",
    )?;
    let file = fs::File::open(&wrapper)?;
    let fd_path = PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()));

    let result = split_object_args(vec![fd_path.into_os_string(), OsString::from("hello")]);

    assert!(matches!(
        result,
        Err(error)
            if error.message().contains("/ctx/tool/bash")
                && error.message().contains("/ctx/tool/safe")
                && error.message().contains("does not match authorized object")
    ));

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn runner_rejects_missing_object_path() {
    assert_eq!(run(Vec::new()), Err(ExecError::new("missing object path")));
}

#[test]
fn runner_rejects_unknown_model() {
    assert_eq!(
        run(vec![
            OsString::from("/ctx/model/missing-provider/gpt-5.4"),
            OsString::from("{}"),
        ]),
        Err(ExecError::new("missing provider: missing-provider"))
    );
}

#[test]
fn model_alias_resolves_only_ctx_model_objects() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!(
        "cortexfs-runner-model-alias-ok-{}",
        std::process::id()
    ));
    let _ignored = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("model"))?;
    symlink("/ctx/model/debug/echo", root.join("model/main"))?;

    assert_eq!(
        resolve_model_alias(&root, "main"),
        Ok("debug/echo".to_owned())
    );
    assert_eq!(
        resolved_model_path(&root, "main"),
        Ok(root.join("model/debug/echo"))
    );

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn model_alias_rejects_cross_class_symlink_target() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!(
        "cortexfs-runner-model-alias-bad-{}",
        std::process::id()
    ));
    let _ignored = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("model"))?;
    symlink("../tool/shell.exec", root.join("model/main"))?;

    assert_eq!(
        resolve_model_alias(&root, "main"),
        Err(ExecError::new("invalid model alias target: main"))
    );

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn model_alias_rejects_symlink_model_directory() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-model-alias-symlink-model-dir")?;
    let outside = unique_temp_dir("runner-model-alias-symlink-model-dir-outside")?;
    fs::create_dir_all(&outside)?;
    symlink("/ctx/model/debug/echo", outside.join("main"))?;
    symlink(&outside, root.join("model"))?;

    assert_eq!(
        resolve_model_alias(&root, "main"),
        Err(ExecError::new("missing model alias: main"))
    );
    assert_eq!(
        missing_model_message(&root, "main", &root.join("model/debug/echo")),
        format!("missing model: {}", root.join("model/debug/echo").display())
    );

    let _ignored = fs::remove_dir_all(root);
    let _ignored = fs::remove_dir_all(outside);
    Ok(())
}

#[test]
fn missing_model_message_names_dangling_alias_target() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-missing-model-message")?;
    fs::create_dir_all(root.join("model"))?;
    symlink("/ctx/model/fixture/alpha", root.join("model/main"))?;

    assert_eq!(
        missing_model_message(&root, "main", &root.join("model/fixture/alpha")),
        "missing model: main -> /ctx/model/fixture/alpha"
    );

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn model_path_rejects_traversal_reference() {
    let root = std::env::temp_dir().join("cortexfs-runner-model-path");

    assert_eq!(
        resolved_model_path(&root, "../tool/shell.exec"),
        Err(ExecError::new(
            "invalid model reference: ../tool/shell.exec"
        ))
    );
}

#[test]
fn model_path_rejects_symlink_intermediate_directory() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-model-path-symlink-intermediate")?;
    let outside = unique_temp_dir("runner-model-path-symlink-intermediate-outside")?;
    fs::create_dir_all(outside.join("debug"))?;
    fs::write(outside.join("debug/echo"), "#!/bin/sh\n")?;
    symlink(&outside, root.join("model"))?;

    assert!(!is_regular_file_no_follow(&root.join("model/debug/echo")));

    let _ignored = fs::remove_dir_all(root);
    let _ignored = fs::remove_dir_all(outside);
    Ok(())
}

#[test]
fn agent_model_config_prefers_current_user_agent_control() -> Result<(), Box<dyn std::error::Error>>
{
    let root = unique_temp_dir("runner-agent-user-control-override")?;
    let global_agent = root.join("agent/coder.d");
    fs::create_dir_all(&global_agent)?;
    fs::write(global_agent.join("model"), "main\n")?;
    fs::write(global_agent.join("label"), "user_u:agent_r:coder_t:s0\n")?;
    fs::write(
        global_agent.join("policy"),
        "allow coder_t model:main use\n",
    )?;
    let uid = nix::unistd::Uid::effective().as_raw().to_string();
    let user_agent = root.join("home").join(uid).join("agent").join("coder.d");
    fs::create_dir_all(&user_agent)?;
    fs::write(user_agent.join("model"), "debug/echo\n")?;
    fs::write(user_agent.join("window"), "auto\n")?;
    fs::write(user_agent.join("label"), "user_u:agent_r:usercoder_t:s0\n")?;
    fs::write(
        user_agent.join("policy"),
        "allow usercoder_t model:debug/echo use\n",
    )?;
    fs::create_dir_all(root.join("model/debug"))?;
    fs::create_dir_all(root.join("model/debug/echo.d"))?;
    fs::write(root.join("model/debug/echo.d/limit"), "unknown\n")?;
    write_executable_script(&root.join("model/debug/echo"), "#!/bin/sh\nexit 0\n")?;

    let config = AgentModelRunConfig::new_with_paths("coder", root.clone(), root.clone());

    assert!(matches!(config, Ok(ref config) if config.model == "debug/echo"));
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn model_candidates_follow_fallback_control_file() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-model-fallback")?;
    fs::create_dir_all(root.join("model/primary/alpha.d"))?;
    fs::write(
        root.join("model/primary/alpha.d/fallback"),
        "primary/beta\nprimary/gamma\n",
    )?;

    let candidates = model_candidates(&root, "primary/alpha")?;

    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.name.as_str())
            .collect::<Vec<_>>(),
        ["primary/alpha", "primary/beta", "primary/gamma"]
    );
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn model_candidates_do_not_append_existing_local_counterpart()
-> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-model-no-implicit-fallback")?;
    fs::create_dir_all(root.join("model/remote/alpha.d"))?;
    fs::write(root.join("model/remote/alpha"), "#!/bin/sh\n")?;
    fs::create_dir_all(root.join("model/mirror"))?;
    fs::write(root.join("model/mirror/alpha"), "#!/bin/sh\n")?;

    let candidates = model_candidates(&root, "remote/alpha")?;

    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.name.as_str())
            .collect::<Vec<_>>(),
        ["remote/alpha"]
    );
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn model_candidates_ignore_symlinked_fallback_control_file()
-> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-model-fallback-symlink")?;
    fs::create_dir_all(root.join("model/primary/alpha.d"))?;
    let outside = root.join("outside-fallback");
    fs::write(&outside, "primary/beta\n")?;
    symlink(&outside, root.join("model/primary/alpha.d/fallback"))?;

    let candidates = model_candidates(&root, "primary/alpha")?;

    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.name.as_str())
            .collect::<Vec<_>>(),
        ["primary/alpha"]
    );
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn agent_model_config_allows_primary_selected_through_alias_policy()
-> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-agent-model-policy-primary")?;
    let agent_dir = root.join("agent/coder.d");
    fs::create_dir_all(&agent_dir)?;
    fs::create_dir_all(root.join("model/debug"))?;
    symlink("/ctx/model/debug/echo", root.join("model/main"))?;
    write_executable_script(&root.join("model/debug/echo"), "#!/bin/sh\nexit 0\n")?;
    fs::write(agent_dir.join("model"), "main\n")?;
    fs::write(agent_dir.join("window"), "auto\n")?;
    fs::create_dir_all(root.join("model/debug/echo.d"))?;
    fs::write(root.join("model/debug/echo.d/limit"), "unknown\n")?;
    fs::write(agent_dir.join("label"), " user_u:agent_r:coder_t:s0\n")?;
    fs::write(agent_dir.join("policy"), "allow coder_t model:main use\n")?;

    let config = AgentModelRunConfig::new_with_paths("coder", root.clone(), root.clone());

    assert!(matches!(config, Ok(ref config) if config.model == "debug/echo"));
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn agent_model_config_denies_fallback_without_selected_model_policy()
-> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-agent-model-policy-fallback")?;
    let agent_dir = root.join("agent/coder.d");
    fs::create_dir_all(&agent_dir)?;
    fs::create_dir_all(root.join("model/primary/approved.d"))?;
    fs::create_dir_all(root.join("model/evil"))?;
    fs::write(
        root.join("model/primary/approved.d/fallback"),
        "evil/leak\n",
    )?;
    write_executable_script(&root.join("model/evil/leak"), "#!/bin/sh\nexit 0\n")?;
    fs::write(agent_dir.join("model"), "primary/approved\n")?;
    fs::write(agent_dir.join("window"), "auto\n")?;
    fs::create_dir_all(root.join("model/evil/leak.d"))?;
    fs::write(root.join("model/evil/leak.d/limit"), "unknown\n")?;
    fs::write(agent_dir.join("label"), "user_u:agent_r:coder_t:s0\n")?;
    fs::write(
        agent_dir.join("policy"),
        "allow coder_t model:primary/approved use\n",
    )?;

    let config = AgentModelRunConfig::new_with_paths("coder", root.clone(), root.clone());

    assert!(matches!(
        config,
        Err(ref error) if error.message().contains("agent policy denies model:evil/leak use")
    ));
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn runner_control_reader_refuses_symlink() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-control-reader-symlink")?;
    let outside = root.join("outside-control");
    let link = root.join("control-link");
    fs::write(&outside, "secret\n")?;
    symlink(&outside, &link)?;

    let result =
        read_small_plain_text_file(&link, super::super::MAX_RUNNER_CONTROL_BYTES, "runner");

    assert!(result.is_err());
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

fn empty_prompt_context() -> AgentPromptContext {
    AgentPromptContext {
        template: String::new(),
        rules: String::new(),
        skills: String::new(),
        tool_injection: String::new(),
        history_messages: String::new(),
        current_time_unix: "0".to_owned(),
    }
}

/// Constructs a provider-candidate admission payload for one test request.
fn candidate_admission<'a>(
    root: &'a Path,
    setting: AgentWindowSetting,
    context: &'a AgentPromptContext,
    skills: &'a str,
) -> ProviderCandidateAdmission<'a> {
    ProviderCandidateAdmission {
        ctx_root: root,
        setting,
        agent: "coder",
        system: "",
        context,
        skills,
        input: "x",
    }
}

#[test]
fn provider_candidate_uses_its_own_limit_for_auto_and_explicit_windows()
-> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("candidate-own-window")?;
    for (model, limit) in [
        ("small/model", "16\n"),
        ("fit/model", "64\n"),
        ("unknown/model", "unknown\n"),
    ] {
        let (provider, name) = model.split_once('/').unwrap_or_default();
        let control = root.join("model").join(provider).join(format!("{name}.d"));
        fs::create_dir_all(&control)?;
        fs::write(control.join("limit"), limit)?;
    }
    let context = empty_prompt_context();
    assert!(matches!(
        admit_provider_candidate(
            "small/model",
            &candidate_admission(&root, AgentWindowSetting::Auto, &context, "")
        ),
        Err(("E2BIG", _))
    ));
    assert!(
        admit_provider_candidate(
            "fit/model",
            &candidate_admission(&root, AgentWindowSetting::Auto, &context, "")
        )
        .is_ok()
    );
    let explicit = AgentWindowSetting::parse_value("32").unwrap_or(AgentWindowSetting::Auto);
    assert!(matches!(
        admit_provider_candidate(
            "small/model",
            &candidate_admission(&root, explicit, &context, "")
        ),
        Err(("EINVAL", _))
    ));
    assert!(matches!(
        admit_provider_candidate(
            "unknown/model",
            &candidate_admission(&root, explicit, &context, "")
        ),
        Err(("EINVAL", _))
    ));
    assert!(
        admit_provider_candidate(
            "fit/model",
            &candidate_admission(&root, explicit, &context, "")
        )
        .is_ok()
    );
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn unknown_candidate_keeps_legacy_skill_cap() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("candidate-unknown-skills")?;
    fs::create_dir_all(root.join("model/local/chat.d"))?;
    fs::write(root.join("model/local/chat.d/limit"), "unknown\n")?;
    let context = empty_prompt_context();
    assert!(
        admit_provider_candidate(
            "local/chat",
            &candidate_admission(
                &root,
                AgentWindowSetting::Auto,
                &context,
                &"x".repeat(8_000)
            )
        )
        .is_ok()
    );
    assert!(matches!(
        admit_provider_candidate(
            "local/chat",
            &candidate_admission(
                &root,
                AgentWindowSetting::Auto,
                &context,
                &"x".repeat(8_001)
            )
        ),
        Err(("E2BIG", _))
    ));
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn agent_window_environment_fails_closed_on_missing_or_noncanonical_setting() {
    let agent = OsStr::new("coder");
    for setting in [
        None,
        Some(""),
        Some("auto\n"),
        Some("01"),
        Some("0"),
        Some("+1"),
        Some("4294967296"),
        Some("junk"),
    ] {
        assert!(
            parse_agent_window_environment(agent.into(), setting.map(OsStr::new)).is_err(),
            "{setting:?}"
        );
    }
    assert!(matches!(
        parse_agent_window_environment(Some(agent), Some(OsStr::new("auto"))),
        Ok(Some((ref name, AgentWindowSetting::Auto))) if name == "coder"
    ));
}

#[test]
fn direct_model_call_does_not_require_agent_window_setting() {
    assert_eq!(parse_agent_window_environment(None, None), Ok(None));
    assert_eq!(
        parse_agent_window_environment(None, Some(OsStr::new("junk"))),
        Ok(None)
    );
}

#[test]
fn runner_control_reader_refuses_symlink_intermediate_dir() -> Result<(), Box<dyn std::error::Error>>
{
    let root = unique_temp_dir("runner-control-reader-symlink-intermediate")?;
    let outside = root.join("outside");
    fs::create_dir_all(outside.join("control.d"))?;
    fs::write(outside.join("control.d/policy"), "secret\n")?;
    symlink(&outside, root.join("model"))?;

    let result = read_small_plain_text_file(
        &root.join("model/control.d/policy"),
        super::super::MAX_RUNNER_CONTROL_BYTES,
        "runner",
    );

    assert!(result.is_err());
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}
use super::*;
