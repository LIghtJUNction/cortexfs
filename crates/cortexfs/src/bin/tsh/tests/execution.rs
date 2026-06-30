#[test]
fn tsh_refuses_tool_execution_without_agent_authority() {
    let root = std::env::temp_dir().join(format!("cortexfs-tsh-empty-argv-{}", std::process::id()));
    let tool_dir = root.join("tool");
    assert!(fs::create_dir_all(&tool_dir).is_ok());
    let tool = tool_dir.join("noop");
    assert!(fs::write(&tool, "#!/bin/sh\n[ \"$CTX_TOOL_MODE\" = cli ]\n").is_ok());
    assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());

    let result = run_tool(&root, "noop", Vec::new());
    assert!(matches!(
        result,
        Err(error)
            if error.message.contains("CTX_AGENT")
                && error.message.contains("ctx agent attach AGENT")
    ));
    let _ignored = fs::remove_dir_all(root);
}

#[test]
fn tsh_tool_execution_gets_clean_agent_environment() {
    if std::env::var_os("CORTEXFS_TSH_ENV_CHILD").is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap_or_default())
            .arg("--exact")
            .arg("tests::tsh_tool_execution_gets_clean_agent_environment")
            .arg("--nocapture")
            .env("CORTEXFS_TSH_ENV_CHILD", "1")
            .env("CORTEXFS_SHOULD_NOT_LEAK", "secret")
            .env("CTX_AGENT", "coder")
            .output();
        assert!(matches!(output, Ok(ref output) if output.status.success()));
        return;
    }

    let root = std::env::temp_dir().join(format!(
        "cortexfs-tsh-clean-tool-env-{}",
        std::process::id()
    ));
    let control = root.join("agent").join("coder.d");
    let tool_control = root.join("tool").join("probe.d");
    assert!(fs::create_dir_all(&control).is_ok());
    assert!(fs::create_dir_all(&tool_control).is_ok());
    assert!(fs::write(control.join("owner"), "1000\n").is_ok());
    assert!(fs::write(control.join("uid"), "1000\n").is_ok());
    assert!(fs::write(control.join("gid"), "1000\n").is_ok());
    assert!(fs::write(control.join("groups"), "1000\n").is_ok());
    assert!(fs::write(control.join("label"), "user_u:agent_r:coder_t:s0\n").is_ok());
    assert!(fs::write(control.join("iso"), "shared\n").is_ok());
    assert!(fs::write(control.join("parent"), "\n").is_ok());
    assert!(fs::write(control.join("life"), "owned\n").is_ok());
    assert!(fs::write(control.join("root"), "/ctx/home/1000/agent/coder/root\n").is_ok());
    assert!(fs::write(control.join("cwd"), "/workspace\n").is_ok());
    assert!(fs::write(control.join("env"), "\n").is_ok());
    assert!(fs::write(control.join("model"), "main\n").is_ok());
    assert!(fs::write(control.join("status"), "idle\n").is_ok());
    assert!(fs::write(control.join("pid"), "\n").is_ok());
    assert!(fs::write(control.join("log"), "\n").is_ok());
    assert!(fs::write(control.join("meta.json"), "{}\n").is_ok());
    assert!(
        fs::write(
            control.join("path"),
            format!("{}\n", root.join("tool").display())
        )
        .is_ok()
    );
    assert!(
        fs::write(
            control.join("mount"),
            format!(
                "{}\t{}\tro\trbind,nosuid,nodev\n",
                root.display(),
                root.display()
            ),
        )
        .is_ok()
    );
    assert!(
        fs::write(
            control.join("policy"),
            "allow coder_t model:main use\nallow coder_t tool:probe execute\n",
        )
        .is_ok()
    );
    assert!(
        fs::write(
            tool_control.join("policy"),
            "allow coder_t tool:probe execute\n"
        )
        .is_ok()
    );
    let tool = root.join("tool").join("probe");
    assert!(
        fs::write(
            &tool,
            r#"#!/bin/sh
[ -z "$CORTEXFS_SHOULD_NOT_LEAK" ] || exit 10
[ "$CTX_TOOL_MODE" = cli ] || exit 11
[ "$CTX_AGENT" = coder ] || exit 12
[ "$CTX_AUTHORIZED_OBJECT" = /ctx/tool/probe ] || exit 15
[ "$PATH" = /usr/bin:/bin ] || exit 13
[ -n "$CTX_ROOT" ] || exit 14
exit 0
"#,
        )
        .is_ok()
    );
    assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());

    let result = run_tool(&root, "probe", Vec::new());

    assert!(matches!(result, Ok(code) if code == ExitCode::SUCCESS));
    let _ignored = fs::remove_dir_all(root);
}

#[test]
fn repl_allows_empty_argv_for_normal_cli_tools() {
    let root = std::env::temp_dir().join(format!(
        "cortexfs-tsh-repl-empty-normal-{}",
        std::process::id()
    ));
    let tool_dir = root.join("tool");
    assert!(fs::create_dir_all(&tool_dir).is_ok());
    let tool = tool_dir.join("noop");
    assert!(fs::write(&tool, "#!/bin/sh\nexit 0\n").is_ok());
    assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());

    let mut context = ToolContext::new(4);
    let result = run_repl_tool(&root, &mut context, "noop", Vec::new());

    assert!(matches!(
        result,
        Err(error)
            if error.message.contains("CTX_AGENT")
                && error.message.contains("ctx agent attach AGENT")
    ));
    let _ignored = fs::remove_dir_all(root);
}

#[test]
fn repl_keeps_explicit_input_guard_for_structured_core_tools() {
    assert!(requires_explicit_repl_input("fs.read"));
    assert!(requires_explicit_repl_input("fs.write"));
    assert!(requires_explicit_repl_input("shell.exec"));
    assert!(!requires_explicit_repl_input("ls"));
    assert!(!requires_explicit_repl_input("project.test"));
}
