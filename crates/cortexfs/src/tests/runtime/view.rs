#[test]
fn agent_runtime_view_derives_identity_environment_policy_and_view() {
    let root = clean_test_dir("agent-runtime-view");
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    let control = root.join("agent").join("coder.d");
    write_text_file(
        &control.join("env"),
        "CTX_ROOT=/ignored\nHOME=/ignored\nPATH=/tmp/pwn\nLD_PRELOAD=/tmp/libpwn.so\nRUST_LOG=info\nCTX_PROVIDER_SECRET_PATH=/tmp/secret\nTERM=vt100\n",
    );
    write_text_file(&control.join("model"), "main\n");
    write_text_file(&control.join("policy"), "allow coder_t model:main use\n");

    let view = derive_agent_runtime_view(&root, "coder");
    let view = ok!(view);
    let home = ctx_home(&root);
    let agent_home = agent_home(&root, "coder");

    assert_eq!(view.agent_name(), "coder");
    assert_eq!(view.control_dir(), control.as_path());
    assert_eq!(view.ctx_root(), root.as_path());
    assert_eq!(view.ctx_home(), home.as_path());
    assert_eq!(view.home(), agent_home.as_path());
    assert_eq!(view.owner(), 1000);
    assert_eq!(view.identity().uid(), 1000);
    assert_eq!(view.identity().gid(), 100);
    assert_eq!(view.identity().groups(), &[10, 20]);
    assert_eq!(view.label(), "user_u:agent_r:coder_t:s0");
    assert_eq!(view.policy_subject(), "coder_t");
    assert_eq!(view.iso(), "shared");
    assert_eq!(view.parent(), None);
    assert_eq!(view.lifecycle(), ChildLifecycle::Owned);
    assert_eq!(view.approval(), crate::AgentApprovalMode::Auto);
    assert_eq!(view.root(), Path::new("/ctx/home/1000/agent/coder/root"));
    assert_eq!(view.cwd(), Path::new("/work"));
    assert_eq!(view.model(), "main");
    assert_eq!(view.window_setting(), AgentWindowSetting::Auto);
    assert_eq!(view.effective_window(), ModelContextLimit::Unknown);
    assert_eq!(
        view.tool_path().dirs(),
        [
            PathBuf::from("/ctx/tool"),
            PathBuf::from("/ctx/home/1000/tool")
        ]
    );
    assert_eq!(view.mount_table().entries().len(), 1);
    assert!(view.policy().allows(
        "coder_t",
        PolicyObjectClass::Model,
        "main",
        PolicyPermission::Use,
    ));
    assert_eq!(
        env_value(view.env(), "CTX_ROOT").map(str::to_owned),
        Some(root.display().to_string())
    );
    assert_eq!(
        env_value(view.env(), "CTX_HOME").map(str::to_owned),
        Some(home.display().to_string())
    );
    assert_eq!(
        env_value(view.env(), "HOME").map(str::to_owned),
        Some(agent_home.display().to_string())
    );
    assert_eq!(env_value(view.env(), "PATH"), Some("/usr/bin:/bin"));
    assert_eq!(
        env_value(view.env(), "CTX_PATH"),
        Some("/ctx/tool:/ctx/home/1000/tool")
    );
    assert_eq!(env_value(view.env(), "TERM"), None);
    assert_eq!(env_value(view.env(), "LD_PRELOAD"), None);
    assert_eq!(env_value(view.env(), "RUST_LOG"), None);
    assert_eq!(env_value(view.env(), "CTX_PROVIDER_SECRET_PATH"), None);
    assert_eq!(env_value(view.env(), "CTX_CONTEXT_WINDOW_TOKENS"), None);
    assert_eq!(env_value(view.env(), "CTX_CONTEXT_WINDOW_CHARS"), None);
    assert_eq!(view.loop_kind(), &crate::AgentLoop::Chat);
    assert_eq!(env_value(view.env(), "CTX_AGENT_LOOP"), Some("chat"));
}

#[test]
fn agent_runtime_view_loads_a_custom_loop_control() {
    let root = clean_test_dir("agent-runtime-loop-control");
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    let control = root.join("agent/coder.d");
    write_text_file(&control.join("loop"), "coding\n");

    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    assert_eq!(view.loop_kind(), &crate::AgentLoop::Coding);
    assert_eq!(env_value(view.env(), "CTX_AGENT_LOOP"), Some("coding"));

    write_text_file(&control.join("loop"), "custom-review\n");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    assert_eq!(
        view.loop_kind(),
        &crate::AgentLoop::Custom("custom-review".to_owned())
    );
}

#[test]
fn agent_runtime_view_resolves_auto_and_explicit_windows() {
    let root = clean_test_dir("agent-runtime-window-resolution");
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    let control = root.join("agent/coder.d");
    write_text_file(&control.join("model"), "local/chat\n");
    write_text_file(&root.join("model/local/chat.d/limit"), "64\n");

    let auto = ok!(derive_agent_runtime_view(&root, "coder"));
    assert_eq!(auto.window_setting(), AgentWindowSetting::Auto);
    assert_eq!(auto.effective_window().tokens(), Some(32));
    assert_eq!(
        env_value(auto.env(), "CTX_CONTEXT_WINDOW_TOKENS"),
        Some("32")
    );
    assert_eq!(
        env_value(auto.env(), "CTX_CONTEXT_WINDOW_CHARS"),
        Some("128")
    );
    for value in ["1\n", "64\n"] {
        write_text_file(&control.join("window"), value);
        assert!(
            derive_agent_runtime_view(&root, "coder").is_ok(),
            "{value:?}"
        );
    }
    write_text_file(&control.join("window"), "65\n");
    assert!(matches!(
        derive_agent_runtime_view(&root, "coder"),
        Err(AgentRuntimeViewError::InvalidControlFile(ref file)) if file == "window"
    ));
    write_text_file(&root.join("model/local/chat.d/limit"), "1000000\n");
    write_text_file(&root.join("model/local/chat.d/recommended"), "500000\n");
    write_text_file(&root.join("model/local/chat.d/compact"), "450000\n");
    write_text_file(&control.join("window"), "auto\n");
    let recommended = ok!(derive_agent_runtime_view(&root, "coder"));
    assert_eq!(recommended.model_limit().tokens(), Some(1_000_000));
    assert_eq!(recommended.model_recommended().tokens(), Some(500_000));
    assert_eq!(recommended.model_compact().tokens(), Some(450_000));
    assert_eq!(recommended.effective_window().tokens(), Some(500_000));
    assert_eq!(recommended.effective_compact().tokens(), Some(450_000));
    assert_eq!(
        env_value(recommended.env(), "CTX_CONTEXT_WINDOW_TOKENS"),
        Some("500000")
    );
    assert_eq!(
        env_value(recommended.env(), "CTX_CONTEXT_COMPACTION_TOKENS"),
        Some("450000")
    );
    write_text_file(&root.join("model/local/chat.d/limit"), "unknown\n");
    write_text_file(&control.join("window"), "32\n");
    assert!(matches!(
        derive_agent_runtime_view(&root, "coder"),
        Err(AgentRuntimeViewError::InvalidControlFile(ref file)) if file == "window"
    ));
    write_text_file(&control.join("window"), "auto\n");
    assert_eq!(
        ok!(derive_agent_runtime_view(&root, "coder")).effective_window(),
        ModelContextLimit::Unknown
    );
}

#[test]
fn agent_runtime_view_rejects_malformed_alias_and_limit() {
    let root = clean_test_dir("agent-runtime-window-invalid-model-state");
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    let control = root.join("agent/coder.d");
    write_text_file(&control.join("model"), "main\n");
    assert!(fs::remove_file(root.join("model/main")).is_ok());
    assert!(symlink("../escape", root.join("model/main")).is_ok());
    assert!(matches!(
        derive_agent_runtime_view(&root, "coder"),
        Err(AgentRuntimeViewError::InvalidControlFile(ref file)) if file == "model"
    ));
    assert!(fs::remove_file(root.join("model/main")).is_ok());
    assert!(symlink("/ctx/model/local/chat", root.join("model/main")).is_ok());
    write_text_file(&root.join("model/local/chat.d/limit"), "0\n");
    assert!(matches!(
        derive_agent_runtime_view(&root, "coder"),
        Err(AgentRuntimeViewError::InvalidControlFile(ref file)) if file == "limit"
    ));
    assert!(fs::remove_file(root.join("model/local/chat.d/limit")).is_ok());
    assert!(matches!(
        derive_agent_runtime_view(&root, "coder"),
        Err(AgentRuntimeViewError::MissingControlFile(ref file)) if file == "limit"
    ));
}

#[test]
fn agent_runtime_view_requires_sdk_envelope_abi() {
    let root = clean_test_dir("agent-runtime-abi");
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    let control = root.join("agent/coder.d");
    write_text_file(&control.join("abi"), "sdk-envelope-v1\n");
    assert!(derive_agent_runtime_view(&root, "coder").is_ok());

    assert!(fs::remove_file(control.join("abi")).is_ok());
    assert!(matches!(
        derive_agent_runtime_view(&root, "coder"),
        Err(AgentRuntimeViewError::MissingControlFile(ref file)) if file == "abi"
    ));
    write_text_file(&control.join("abi"), "argv-v1\n");
    assert!(matches!(
        derive_agent_runtime_view(&root, "coder"),
        Err(AgentRuntimeViewError::InvalidControlFile(ref file)) if file == "abi"
    ));
    write_text_file(&control.join("abi"), "sdk-envelope-v1\n");
    write_text_file(&control.join("approval"), "ask\n");
    assert_eq!(
        ok!(derive_agent_runtime_view(&root, "coder")).approval(),
        crate::AgentApprovalMode::Ask
    );
}

#[test]
fn agent_runtime_view_prefers_current_user_agent_control() {
    let root = clean_test_dir("agent-runtime-view-user-override");
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    let uid = nix::unistd::Uid::effective().as_raw().to_string();
    let user_control = root.join("home").join(&uid).join("agent").join("coder.d");
    assert!(fs::create_dir_all(&user_control).is_ok());
    let root_control = format!("/ctx/home/{uid}/agent/coder/root");
    let tool_path = format!("/ctx/tool:/ctx/home/{uid}/tool");
    for (file, value) in [
        ("owner", uid.as_str()),
        ("uid", uid.as_str()),
        ("gid", uid.as_str()),
        ("groups", uid.as_str()),
        ("perm", "rwx"),
        ("label", "user_u:agent_r:usercoder_t:s0"),
        ("iso", "shared"),
        ("parent", "agent:base"),
        ("life", "owned"),
        ("root", root_control.as_str()),
        ("cwd", "/workspace"),
        ("env", ""),
        ("path", tool_path.as_str()),
        ("mount", "/ctx\t/ctx\tro\trbind,nosuid,nodev"),
        ("model", "debug/echo"),
        ("abi", "sdk-envelope-v1"),
        ("window", "auto"),
        ("policy", "allow usercoder_t model:debug/echo use"),
    ] {
        write_text_file(&user_control.join(file), &format!("{value}\n"));
    }

    let view = derive_agent_runtime_view(&root, "coder");
    let view = ok!(view);

    assert_eq!(view.control_dir(), user_control.as_path());
    assert_eq!(view.owner().to_string(), uid);
    assert_eq!(view.policy_subject(), "usercoder_t");
    assert_eq!(view.model(), "debug/echo");
}

#[test]
fn secret_tool_lookup_uses_absolute_program_path() {
    assert_eq!(super::SECRET_TOOL_PROGRAM, "/usr/bin/secret-tool");
}

#[test]
fn secret_tool_lookup_uses_minimal_dbus_environment() {
    let inherited = super::secret_tool_dbus_address(
        |name| {
            if name == "DBUS_SESSION_BUS_ADDRESS" {
                Some("unix:path=/tmp/session-bus".into())
            } else {
                Some("ignored".into())
            }
        },
        1000,
    );
    assert_eq!(
        inherited,
        std::ffi::OsString::from("unix:path=/tmp/session-bus")
    );

    let defaulted = super::secret_tool_dbus_address(|_name| None, 1000);
    assert_eq!(
        defaulted,
        std::ffi::OsString::from("unix:path=/run/user/1000/bus")
    );
}

#[test]
fn secret_tool_runner_rejects_oversized_stdout() {
    let mut command = std::process::Command::new("sh");
    command
        .arg("-c")
        .arg("head -c 16384 /dev/zero | tr '\\0' x")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());

    let output = super::run_secret_tool_command_with_timeout(command, Duration::from_secs(2));

    assert!(matches!(output, Err(ref error) if error.contains("output exceeds limit")));
}

#[test]
fn secret_tool_runner_kills_child_after_oversized_stdout() {
    let mut command = std::process::Command::new("sh");
    command
        .arg("-c")
        .arg("head -c 16384 /dev/zero | tr '\\0' x; sleep 5")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let started = std::time::Instant::now();

    let output = super::run_secret_tool_command_with_timeout(command, Duration::from_secs(10));

    assert!(matches!(output, Err(ref error) if error.contains("output exceeds limit")));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn secret_tool_runner_times_out_instead_of_hanging() {
    let mut command = std::process::Command::new("sh");
    command
        .arg("-c")
        .arg("sleep 5")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let started = std::time::Instant::now();

    let output = super::run_secret_tool_command_with_timeout(command, Duration::from_millis(100));

    assert!(matches!(output, Err(ref error) if error.contains("timed out")));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn agent_runtime_view_rejects_invalid_control_files() {
    let cases = [
        ("uid", "not-a-uid\n"),
        ("groups", "10\nbad\n"),
        ("label", "user_u:agent_r:bad/name:s0\n"),
        ("root", "../root\n"),
        ("cwd", "/work/../secret\n"),
        ("env", "1BAD=value\n"),
        ("env", "OK=\u{1b}]52;c;payload\u{7}\n"),
        ("path", "/ctx/tool:../tool\n"),
        ("mount", "bad\n"),
        ("model", "bad/name/extra\n"),
        ("policy", "allow bad\n"),
    ];

    for (file, value) in cases {
        let root = clean_test_dir(&format!("agent-runtime-invalid-{file}"));
        create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
        write_text_file(&root.join("agent").join("coder.d").join(file), value);

        assert_eq!(
            derive_agent_runtime_view(&root, "coder"),
            Err(AgentRuntimeViewError::InvalidControlFile(file.to_owned()))
        );
        assert_eq!(
            AgentRuntimeViewError::InvalidControlFile(file.to_owned()).errno(),
            "EINVAL"
        );
    }
}

#[test]
fn agent_runtime_view_rejects_symlink_control_files() {
    let root = clean_test_dir("agent-runtime-symlink-control");
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    let outside = root.join("outside-label");
    write_text_file(&outside, "user_u:agent_r:coder_t:s0\n");
    let label = root.join("agent").join("coder.d").join("label");
    assert!(fs::remove_file(&label).is_ok());
    assert!(symlink(&outside, &label).is_ok());

    assert_eq!(
        derive_agent_runtime_view(&root, "coder"),
        Err(AgentRuntimeViewError::CannotReadControl("label".to_owned()))
    );
}

#[test]
fn agent_runtime_view_rejects_symlink_agent_directory() {
    let root = clean_test_dir("agent-runtime-symlink-agent-dir");
    let outside = clean_test_dir("agent-runtime-symlink-agent-dir-outside");
    create_complete_object_layout(&outside, ObjectClass::Agent, "coder", "none");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(symlink(outside.join("agent"), root.join("agent")).is_ok());

    assert_eq!(
        derive_agent_runtime_view(&root, "coder"),
        Err(AgentRuntimeViewError::MissingControlDirectory)
    );
}

#[test]
fn agent_runtime_view_reports_missing_controls_and_bad_agent_names() {
    let root = clean_test_dir("agent-runtime-missing");
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    assert_eq!(
        derive_agent_runtime_view(&root, "bad/name"),
        Err(AgentRuntimeViewError::InvalidAgentName)
    );

    let model = root.join("agent").join("coder.d").join("model");
    assert!(fs::remove_file(model).is_ok());
    assert_eq!(
        derive_agent_runtime_view(&root, "coder"),
        Err(AgentRuntimeViewError::MissingControlFile(
            "model".to_owned()
        ))
    );
    assert_eq!(
        AgentRuntimeViewError::MissingControlFile("model".to_owned()).errno(),
        "ENOENT"
    );
}

#[test]
fn agent_runtime_view_defaults_missing_worker_model_to_current_default() {
    let root = clean_test_dir("agent-runtime-worker-missing-model");
    create_complete_object_layout(&root, ObjectClass::Agent, "worker", "none");
    let control = root.join("agent").join("worker.d");
    write_text_file(&control.join("label"), "user_u:agent_r:worker_t:s0\n");
    write_text_file(
        &control.join("policy"),
        "allow worker_t model:openai/gpt-5.6 use\n",
    );
    let default_model = root.join("model/openai/gpt-5.6.d");
    assert!(fs::create_dir_all(&default_model).is_ok());
    write_text_file(&default_model.join("limit"), "unknown\n");
    assert!(fs::remove_file(control.join("model")).is_ok());

    let view = derive_agent_runtime_view(&root, "worker");
    let view = ok!(view);
    assert_eq!(view.model(), "openai/gpt-5.6");
    assert!(view.policy().allows(
        "worker_t",
        PolicyObjectClass::Model,
        "openai/gpt-5.6",
        PolicyPermission::Use,
    ));
}

#[test]
fn agent_runtime_view_defaults_missing_life_to_owned() {
    let root = clean_test_dir("agent-runtime-missing-life");
    create_complete_object_layout(&root, ObjectClass::Agent, "worker", "none");
    let control = root.join("agent").join("worker.d");
    assert!(fs::remove_file(control.join("life")).is_ok());

    let view = derive_agent_runtime_view(&root, "worker");
    let view = ok!(view);
    assert_eq!(view.lifecycle(), ChildLifecycle::Owned);
}

#[test]
fn agent_runtime_view_accepts_parent_run_field() {
    let root = clean_test_dir("agent-runtime-parent-run");
    create_complete_object_layout(&root, ObjectClass::Agent, "worker", "none");
    let control = root.join("agent").join("worker.d");
    write_text_file(
        &control.join("parent"),
        "agent:coder session:default run:r123\n",
    );

    let view = derive_agent_runtime_view(&root, "worker");
    let view = ok!(view);
    assert_eq!(view.parent(), Some("agent:coder session:default run:r123"));
}

#[test]
fn agent_runtime_view_rejects_unknown_parent_field() {
    let root = clean_test_dir("agent-runtime-parent-unknown-field");
    create_complete_object_layout(&root, ObjectClass::Agent, "worker", "none");
    let control = root.join("agent").join("worker.d");
    write_text_file(&control.join("parent"), "agent:coder task:work\n");

    assert_eq!(
        derive_agent_runtime_view(&root, "worker"),
        Err(AgentRuntimeViewError::InvalidControlFile(
            "parent".to_owned()
        ))
    );
}

#[test]
fn agent_runtime_view_defaults_missing_worker_prefix_model_to_current_default() {
    let root = clean_test_dir("agent-runtime-worker-prefix-missing-model");
    create_complete_object_layout(&root, ObjectClass::Agent, "worker-fast", "none");
    let control = root.join("agent").join("worker-fast.d");
    write_text_file(&control.join("label"), "user_u:agent_r:worker-fast_t:s0\n");
    write_text_file(
        &control.join("policy"),
        "allow worker-fast_t model:openai/gpt-5.6 use\n",
    );
    let default_model = root.join("model/openai/gpt-5.6.d");
    assert!(fs::create_dir_all(&default_model).is_ok());
    write_text_file(&default_model.join("limit"), "unknown\n");
    assert!(fs::remove_file(control.join("model")).is_ok());

    let view = derive_agent_runtime_view(&root, "worker-fast");
    let view = ok!(view);
    assert_eq!(view.model(), "openai/gpt-5.6");
}

#[test]
fn agent_runtime_view_defaults_missing_executor_prefix_model_to_current_default() {
    let root = clean_test_dir("agent-runtime-executor-prefix-missing-model");
    create_complete_object_layout(&root, ObjectClass::Agent, "executor-fast", "none");
    let control = root.join("agent").join("executor-fast.d");
    write_text_file(
        &control.join("label"),
        "user_u:agent_r:executor-fast_t:s0\n",
    );
    write_text_file(
        &control.join("policy"),
        "allow executor-fast_t model:openai/gpt-5.6 use\n",
    );
    let default_model = root.join("model/openai/gpt-5.6.d");
    assert!(fs::create_dir_all(&default_model).is_ok());
    write_text_file(&default_model.join("limit"), "unknown\n");
    assert!(fs::remove_file(control.join("model")).is_ok());

    let view = derive_agent_runtime_view(&root, "executor-fast");
    let view = ok!(view);
    assert_eq!(view.model(), "openai/gpt-5.6");
}

#[test]
fn agent_runtime_view_env_prompt_and_skill_text_do_not_expand_tool_path() {
    let root = clean_test_dir("agent-runtime-no-text-grant");
    let allowed = root.join("tool");
    let env_only = root.join("env-tool");

    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    write_fixture_file(&env_only.join("fs.read"), 0o755);
    write_text_file(
        &root.join("work").join("AGENTS.md"),
        "The agent may execute fs.read.\n",
    );
    write_text_file(
        &root.join("work").join(".mcp.json"),
        "{\"servers\":{\"fs\":{\"tools\":[\"fs.read\"]}}}\n",
    );

    let control = root.join("agent").join("coder.d");
    write_text_file(&control.join("path"), &format!("{}\n", allowed.display()));
    write_text_file(
        &control.join("env"),
        &format!("CTX_PATH={}\nAGENT_RULES=allow\n", env_only.display()),
    );
    write_text_file(
        &control.join("policy"),
        "allow coder_t tool:fs.read execute\n",
    );

    let view = derive_agent_runtime_view(&root, "coder");
    let view = ok!(view);
    assert_eq!(
        env_value(view.env(), "CTX_PATH").map(str::to_owned),
        Some(allowed.display().to_string())
    );
    assert_eq!(env_value(view.env(), "AGENT_RULES"), None);

    let identity = ok!(unix_identity_for(&env_only.join("fs.read")));
    let mounts = mount_table_for_target(&env_only, "rw", "bind,nosuid,nodev");
    let tool_policy = allow_tool_policy("coder_t", "fs.read");
    let denied = authorize_tool_execution(
        view.tool_path(),
        "fs.read",
        ToolExecutionAuthority::new(
            &identity,
            &mounts,
            view.policy_subject(),
            view.policy(),
            &tool_policy,
            view.permissions(),
        ),
    );
    assert_eq!(denied, Err(ToolExecutionDenial::ToolNotFound));
}

#[test]
fn agent_runtime_view_accepts_missing_and_empty_optional_tools_control() {
    let root = clean_test_dir("agent-runtime-optional-tools");
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    let control = root.join("agent/coder.d");
    assert!(!control.join("tools").exists());
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    assert!(view.declared_tools().is_empty());
    write_text_file(&control.join("tools"), "\n");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    assert!(view.declared_tools().is_empty());
}
use super::*;
