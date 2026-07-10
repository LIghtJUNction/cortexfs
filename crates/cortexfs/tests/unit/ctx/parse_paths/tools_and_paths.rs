#[test]
fn bootstrap_output_lists_all_reference_agents() {
    assert_eq!(
        BOOTSTRAP_REFERENCE_AGENT_SUMMARY_LINE,
        "agents=architect,coder,reviewer,worker"
    );
}

#[test]
fn reference_bootstrap_gives_coder_source_editing_tools() {
    let root = clean_test_dir("ctx-reference-coder-source-tools");

    assert!(ensure_v1_reference_tree(&root).is_ok());

    for tool in ["fs.read", "fs.write", "fs.replace", "shell.exec"] {
        let path = root.join("tool").join(tool);
        assert!(path.exists(), "{tool} executable missing");
        assert!(
            fs::metadata(&path)
                .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0),
            "{tool} is not executable"
        );
    }

    let coder_policy = fs::read_to_string(root.join("agent/coder.d/policy")).unwrap_or_default();
    assert!(coder_policy.contains("allow coder_t tool:fs.read execute"));
    assert!(coder_policy.contains("allow coder_t tool:fs.write execute"));
    assert!(coder_policy.contains("allow coder_t tool:fs.replace execute"));
    assert!(coder_policy.contains("allow coder_t tool:shell.exec execute"));

    let reviewer_policy =
        fs::read_to_string(root.join("agent/reviewer.d/policy")).unwrap_or_default();
    assert!(!reviewer_policy.contains("tool:fs.write execute"));
    assert!(!reviewer_policy.contains("tool:fs.replace execute"));
    assert!(!reviewer_policy.contains("tool:shell.exec execute"));

    let coder_prompt = fs::read_to_string(root.join("agent/coder.d/system.md")).unwrap_or_default();
    assert!(coder_prompt.contains("writable project checkout mounted at `/workspace`"));
    assert!(coder_prompt.contains("fs.write"));
    assert!(coder_prompt.contains("fs.replace"));
    assert!(coder_prompt.contains("shell.exec"));
    assert!(coder_prompt.contains("do not stop at a plan"));
    assert!(coder_prompt.contains("implement the requested change directly through `tsh`"));
    assert!(coder_prompt.contains("formatter, static check, lint, and focused tests"));
    assert!(coder_prompt.contains("inspect the applicable project rules"));
    assert!(coder_prompt.contains("current workspace state"));
    assert!(!coder_prompt.contains("git status --short"));
    assert!(!coder_prompt.contains("find /workspace -name AGENTS.md -print"));
    assert!(coder_prompt.contains("never overwrite, revert, delete, or reformat unrelated user changes"));
    assert!(coder_prompt.contains("Never run destructive git commands"));
}

#[test]
fn agent_prompt_renders_runtime_system_prompt_from_control_files() {
    let root = clean_test_dir("ctx-agent-prompt-render");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let control = root.join("agent").join("coder.d");
    assert!(fs::write(control.join("system.md"), "Be precise.
").is_ok());
    assert!(
        fs::write(
            control.join("prompt.template.md"),
            "agent={{agent}}
time={{current_time_unix}}
inst={{agent_instructions}}
{{runtime_contract}}
",
        )
        .is_ok()
    );

    let prompt = build_agent_system_prompt(&root, "coder", "123");
    assert!(matches!(
        prompt,
        Ok(ref prompt) if prompt.contains("agent=coder")
            && prompt.contains("time=123")
            && prompt.contains("inst=Be precise.")
            && prompt.contains("native callable tool exposed by this runtime is `tsh`")
            && prompt.contains("When tool execution is useful")
            && prompt.contains("Tool results include the original `arguments.args`")
            && prompt.contains("If no concrete file path is provided")
            && prompt.contains("For coding work")
            && prompt.contains("inspect current files before editing")
            && prompt.contains("Never run destructive git commands")
            && !prompt.contains("output this exact tool call")
            && !prompt.contains(r#"["fs.read","/workspace/PATH"]"#)
            && !prompt.contains("{{agent}}")
    ));
}

#[test]
fn shell_quote_arg_escapes_single_quotes() {
    assert_eq!(shell_quote_arg("default"), "default");
    assert_eq!(shell_quote_arg("has space"), "'has space'");
    assert_eq!(shell_quote_arg("can't"), "'can'\\''t'");
}

#[test]
fn cli_names_accept_abi_valid_uppercase_names() {
    for name in ["NAME", "SESSION", "AGENT", "SOURCE", "TARGET", "PATH", "INPUT", "RUN"] {
        assert!(require_cli_name("agent name", name).is_ok(), "{name}");
        assert!(require_session_name(name).is_ok(), "{name}");
    }
}

#[test]
fn session_names_reject_control_characters() {
    for name in ["bad\rname", "bad\u{1b}name"] {
        assert!(require_session_name(name).is_err(), "{name:?}");
    }
}

fn contains_arg_pair(args: &[String], first: &str, second: &str) -> bool {
    args.windows(2)
        .any(|window| window.first().map(String::as_str) == Some(first)
            && window.get(1).map(String::as_str) == Some(second))
}

fn contains_arg_triplet(args: &[String], first: &str, second: &str, third: &str) -> bool {
    args.windows(3)
        .any(|window| window.first().map(String::as_str) == Some(first)
            && window.get(1).map(String::as_str) == Some(second)
            && window.get(2).map(String::as_str) == Some(third))
}

fn contains_ro_bind_stub(args: &[String], target: &str) -> bool {
    args.windows(3).any(|window| {
        window.first().map(String::as_str) == Some("--ro-bind")
            && window
                .get(1)
                .is_some_and(|source| source.ends_with("/.empty-shell-startup"))
            && window.get(2).map(String::as_str) == Some(target)
    })
}

fn create_agent_fixture(root: &Path, name: &str, parent: &str, status: &str, pid: &str) {
    let agent = fixture_path(root, &["agent", name]);
    write_text_file(&agent, "#!/bin/sh\nexit 0\n");
    let metadata = fs::metadata(&agent);
    assert!(metadata.is_ok());
    if let Ok(metadata) = metadata {
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o755);
        assert!(fs::set_permissions(&agent, permissions).is_ok());
    }
    let control = fixture_path(root, &["agent", &format!("{name}.d")]);
    write_text_file(&control.join("parent"), &format!("{parent}\n"));
    write_text_file(&control.join("life"), "owned\n");
    write_text_file(&control.join("status"), &format!("{status}\n"));
    write_text_file(&control.join("pid"), &format!("{pid}\n"));
}

#[test]
fn parses_bootstrap_and_mount_commands() {
    let bootstrap = cmd!("bootstrap");
    assert!(matches!(
        bootstrap,
        Ok(Command::Bootstrap {
            source: None,
            dry_run: false,
            check: false
        })
    ));

    let update = cmd!("update");
    assert!(matches!(
        update,
        Ok(Command::Bootstrap {
            source: None,
            dry_run: false,
            check: false
        })
    ));

    let bootstrap_source = cmd!("bootstrap", "/tmp/cortexfs-source");
    assert!(matches!(
        bootstrap_source,
        Ok(Command::Bootstrap {
            source: Some(ref source),
            dry_run: false,
            check: false
        }) if source == Path::new("/tmp/cortexfs-source")
    ));

    let update_source = cmd!("update", "/tmp/cortexfs-source");
    assert!(matches!(
        update_source,
        Ok(Command::Bootstrap {
            source: Some(ref source),
            dry_run: false,
            check: false
        }) if source == Path::new("/tmp/cortexfs-source")
    ));

    let dry_run = cmd!("update", "--dry-run", "/tmp/cortexfs-source");
    assert!(matches!(
        dry_run,
        Ok(Command::Bootstrap {
            source: Some(ref source),
            dry_run: true,
            check: false
        }) if source == Path::new("/tmp/cortexfs-source")
    ));

    let check = cmd!("bootstrap", "--check");
    assert!(matches!(
        check,
        Ok(Command::Bootstrap {
            source: None,
            dry_run: false,
            check: true
        })
    ));

    assert!(matches!(
        cmd!("update", "--check", "--dry-run"),
        Err(ref error) if error.code == 2
    ));

    let mount = cmd!(
        "mount",
        "--source",
        "/tmp/cortexfs-source",
        "/tmp/cortexfs-mount"
    );
    assert!(matches!(
        mount,
        Ok(Command::Mount {
            source: Some(ref source),
            mountpoint: Some(ref mountpoint)
        }) if source == Path::new("/tmp/cortexfs-source")
            && mountpoint == Path::new("/tmp/cortexfs-mount")
    ));
}

#[test]
fn parses_exec_command_with_arguments() {
    let command = cmd!("exec", "agent/coder", "fix tests");
    assert!(matches!(
        command,
        Ok(Command::Exec {
            ref path,
            ref args
        }) if path == "agent/coder" && args == &["fix tests".to_owned()]
    ));
}

#[test]
fn parses_tool_command_with_arguments() {
    let command = cmd!("tool", "fs.read", "README.md");
    assert!(matches!(
        command,
        Ok(Command::Tool {
            ref name,
            ref args
        }) if name == "fs.read" && args == &["README.md".to_owned()]
    ));
}

#[test]
fn parses_provider_oauth_commands() {
    let login = cmd!("provider", "oauth", "login", "api.openai.com", "--timeout", "30");
    assert!(matches!(
        login,
        Ok(Command::Provider(ProviderArgs::Login {
            ref provider,
            timeout
        })) if provider == "api.openai.com" && timeout == 30
    ));

    let status = cmd!("provider", "oauth", "status", "api.openai.com");
    assert!(matches!(
        status,
        Ok(Command::Provider(ProviderArgs::Status { ref provider }))
            if provider == "api.openai.com"
    ));

    let refresh = cmd!("provider", "oauth", "refresh", "api.openai.com");
    assert!(matches!(
        refresh,
        Ok(Command::Provider(ProviderArgs::Refresh { ref provider }))
            if provider == "api.openai.com"
    ));
}

#[test]
fn parses_provider_oauth_help_commands() {
    assert!(matches!(
        cmd!("provider", "--help"),
        Ok(Command::HelpTopic(ref topic)) if topic == "provider"
    ));
    assert!(matches!(
        cmd!("provider", "oauth", "--help"),
        Ok(Command::HelpTopic(ref topic)) if topic == "provider oauth"
    ));
    assert!(matches!(
        cmd!("provider", "oauth", "login", "--help"),
        Ok(Command::HelpTopic(ref topic)) if topic == "provider oauth login"
    ));
}

#[test]
fn parses_provider_preset_commands() {
    assert!(matches!(
        cmd!("provider", "preset", "list"),
        Ok(Command::Provider(ProviderArgs::PresetList))
    ));
    assert!(matches!(
        cmd!("provider", "preset", "show", "google"),
        Ok(Command::Provider(ProviderArgs::PresetShow { ref preset }))
            if preset == "google"
    ));
    assert!(matches!(
        cmd!("provider", "preset", "install", "anthropic"),
        Ok(Command::Provider(ProviderArgs::PresetInstall { ref preset }))
            if preset == "anthropic"
    ));
}

#[test]
fn parses_provider_secret_commands() {
    assert!(matches!(
        cmd!("provider", "secret", "set", "local"),
        Ok(Command::Provider(ProviderArgs::SecretSet {
            ref provider,
            ref slot
        })) if provider == "local" && slot == "default"
    ));
    assert!(matches!(
        cmd!("provider", "secret", "status", "openai", "--slot", "office"),
        Ok(Command::Provider(ProviderArgs::SecretStatus {
            ref provider,
            ref slot
        })) if provider == "openai" && slot == "office"
    ));
}

#[test]
fn tool_command_runs_core_tool_cli_at_selected_root() {
    let root = clean_test_dir("ctx-tool-command-core");
    assert!(ensure_v1_reference_tree(&root).is_ok());

    let mut output = Vec::new();
    let result = run_visible_tool_with_writer(
        &root,
        "tsh.config",
        &[r#"{"max_loaded_tools":9,"cache_capacity":4,"window_percent":2}"#.to_owned()],
        &mut output,
    );

    assert_eq!(result, Ok(ExitCode::SUCCESS));
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains("tool/tsh.d/config"));
    let config = fs::read_to_string(root.join("tool/tsh.d/config")).unwrap_or_default();
    assert!(config.contains("max_loaded_tools=9\n"));
    assert!(config.contains("cache_capacity=4\n"));
    assert!(config.contains("window_percent=2\n"));
}

#[test]
fn tool_command_requires_core_tool_to_be_visible() {
    let root = clean_test_dir("ctx-tool-command-core-hidden");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    assert!(fs::remove_file(root.join("tool").join("tsh.config")).is_ok());

    let result = run_visible_tool_with_writer(
        &root,
        "tsh.config",
        &[r#"{"max_loaded_tools":9}"#.to_owned()],
        &mut Vec::new(),
    );

    assert!(matches!(
        result,
        Err(ref error)
            if error.code == 69 && error.message.contains("tool not found in CTX_PATH: tsh.config")
    ));
}

#[test]
fn tool_command_refuses_authority_bearing_core_tool_cli() {
    let root = clean_test_dir("ctx-tool-command-core-authority");
    let tool = root.join("tool").join("fs.write");
    write_text_file(&tool, "#!/bin/sh\nexit 7\n");
    assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());
    let blocked_path = root.join("blocked-output");

    let result = run_visible_tool_with_writer(
        &root,
        "fs.write",
        &[blocked_path.display().to_string(), "blocked".to_owned()],
        &mut Vec::new(),
    );

    assert!(matches!(
        result,
        Err(ref error)
            if error.code == 69
                && error.message.contains("direct CTX_PATH execution bypasses CortexFS tool authorization")
    ));
    assert!(!blocked_path.exists());
}
