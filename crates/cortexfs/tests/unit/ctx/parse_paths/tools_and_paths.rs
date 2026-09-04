#[test]
fn bootstrap_output_lists_all_reference_agents() {
    assert_eq!(
        BOOTSTRAP_REFERENCE_AGENT_SUMMARY_LINE,
        "agents=architect,executor,product-manager"
    );
}

#[test]
fn reference_bootstrap_gives_executor_source_editing_tools() {
    let root = clean_test_dir("ctx-reference-executor-source-tools");

    assert!(ensure_reference_tree(&root).is_ok());

    for tool in ["fs.read", "fs.write", "fs.replace", "shell.exec"] {
        let path = root.join("tool").join(tool);
        assert!(path.exists(), "{tool} executable missing");
        assert!(
            fs::metadata(&path).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0),
            "{tool} is not executable"
        );
    }

    let executor_policy =
        fs::read_to_string(root.join("agent/executor.d/policy")).unwrap_or_default();
    assert!(executor_policy.contains("allow executor_t tool:fs.read execute"));
    assert!(executor_policy.contains("allow executor_t tool:fs.write execute"));
    assert!(executor_policy.contains("allow executor_t tool:fs.replace execute"));
    assert!(executor_policy.contains("allow executor_t tool:shell.exec execute"));

    let product_policy =
        fs::read_to_string(root.join("agent/product-manager.d/policy")).unwrap_or_default();
    assert!(!product_policy.contains("tool:fs.write execute"));
    assert!(!product_policy.contains("tool:fs.replace execute"));
    assert!(!product_policy.contains("tool:shell.exec execute"));

    let executor_prompt =
        fs::read_to_string(root.join("agent/executor.d/system.md")).unwrap_or_default();
    assert!(executor_prompt.contains("writable project checkout mounted at `/workspace`"));
    assert!(executor_prompt.contains("fs.write"));
    assert!(executor_prompt.contains("fs.replace"));
    assert!(executor_prompt.contains("shell.exec"));
    assert!(executor_prompt.contains("smallest atomic change"));
    assert!(executor_prompt.contains("run focused formatter/check/lint/tests"));
    assert!(executor_prompt.contains("Read the applicable rules"));
    assert!(executor_prompt.contains("current workspace state"));
    assert!(executor_prompt.contains("Do not invent a result"));
}

#[test]
fn agent_prompt_renders_runtime_system_prompt_from_control_files() {
    let root = clean_test_dir("ctx-agent-prompt-render");
    assert!(ensure_reference_tree(&root).is_ok());
    let control = root.join("agent").join("executor.d");
    assert!(fs::write(
        control.join("system.md"),
        "Be precise.
"
    )
    .is_ok());
    assert!(fs::write(
        control.join("prompt.template.md"),
        "agent={{agent}}
time={{current_time_unix}}
inst={{agent_instructions}}
{{runtime_contract}}
",
    )
    .is_ok());

    let prompt = build_agent_system_prompt(&root, "executor", "123");
    assert!(matches!(
        prompt,
        Ok(ref prompt) if prompt.contains("agent=executor")
            && prompt.contains("time=123")
            && prompt.contains("inst=Be precise.")
            && prompt.contains("`tsh` is always native")
            && prompt.contains("only tools statically declared by the agent `tools` control")
            && prompt.contains("For useful tool work")
            && prompt.contains("Results echo it with stdout/stderr")
            && prompt.contains("Ask for a concrete path only when")
            && prompt.contains("Before code changes, inspect")
            && prompt.contains("Run `git reset --hard`, `git checkout --`, or `git clean`")
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
    for name in [
        "NAME", "SESSION", "AGENT", "SOURCE", "TARGET", "PATH", "INPUT", "RUN",
    ] {
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
    args.windows(2).any(|window| {
        window.first().map(String::as_str) == Some(first)
            && window.get(1).map(String::as_str) == Some(second)
    })
}

fn contains_arg_triplet(args: &[String], first: &str, second: &str, third: &str) -> bool {
    args.windows(3).any(|window| {
        window.first().map(String::as_str) == Some(first)
            && window.get(1).map(String::as_str) == Some(second)
            && window.get(2).map(String::as_str) == Some(third)
    })
}

fn contains_empty_startup_bind(args: &[String], target: &str) -> bool {
    contains_arg_triplet(args, "--ro-bind", "/dev/null", target)
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

    let bootstrap_source = cmd!("bootstrap", "/tmp/cortexfs-source");
    assert!(matches!(
        bootstrap_source,
        Ok(Command::Bootstrap {
            source: Some(ref source),
            dry_run: false,
            check: false
        }) if source == Path::new("/tmp/cortexfs-source")
    ));

    let dry_run = cmd!("bootstrap", "--dry-run", "/tmp/cortexfs-source");
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
        cmd!("bootstrap", "--check", "--dry-run"),
        Err(ref error) if error.code == 2
    ));

    assert!(matches!(
        cmd!("storage", "update", "/var/lib/cortexfs/storage"),
        Ok(Command::StorageUpdate { storage: Some(ref path), prune: false })
            if path == Path::new("/var/lib/cortexfs/storage")
    ));
    assert!(matches!(
        cmd!("storage", "update"),
        Ok(Command::StorageUpdate {
            storage: None,
            prune: false
        })
    ));
    assert!(matches!(
        cmd!("storage", "update", "--prune", "/var/lib/cortexfs/storage"),
        Ok(Command::StorageUpdate { storage: Some(ref path), prune: true })
            if path == Path::new("/var/lib/cortexfs/storage")
    ));
    assert!(cmd!("storage", "delete").is_err());

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
    let command = cmd!("exec", "agent/executor", "fix tests");
    assert!(matches!(
        command,
        Ok(Command::Exec {
            ref path,
            ref args
        }) if path == "agent/executor" && args == &["fix tests".to_owned()]
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
    let login = cmd!(
        "provider",
        "oauth",
        "login",
        "api.openai.com",
        "--timeout",
        "30"
    );
    assert!(matches!(
        login,
        Ok(Command::Provider(ProviderArgs::Login {
            ref provider, ref profile, timeout: 30, device: false
        })) if provider == "api.openai.com" && profile == "default"
    ));
    assert!(
        matches!(cmd!("provider", "oauth", "login", "codex", "--device"),
        Ok(Command::Provider(ProviderArgs::Login {
            ref provider, ref profile, timeout: 120, device: true
        })) if provider == "codex" && profile == "default")
    );

    let status = cmd!("provider", "oauth", "status", "api.openai.com");
    assert!(matches!(
        status,
        Ok(Command::Provider(ProviderArgs::Status { ref provider, ref profile }))
            if provider == "api.openai.com" && profile == "default"
    ));

    let refresh = cmd!("provider", "oauth", "refresh", "api.openai.com");
    assert!(matches!(
        refresh,
        Ok(Command::Provider(ProviderArgs::Refresh { ref provider, ref profile }))
            if provider == "api.openai.com" && profile == "default"
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
fn parses_provider_auth_methods_command() {
    assert!(matches!(
        cmd!("provider", "auth", "methods", "openai"),
        Ok(Command::Provider(ProviderArgs::AuthMethods { ref provider }))
            if provider == "openai"
    ));
    assert!(matches!(
        cmd!("provider", "auth", "methods", "--help"),
        Ok(Command::HelpTopic(ref topic)) if topic == "provider auth methods"
    ));
}

#[test]
fn parses_unified_auth_commands() {
    assert!(matches!(
        cmd!("auth", "methods", "openai"),
        Ok(Command::Auth(ProviderArgs::AuthMethods { ref provider })) if provider == "openai"
    ));
    assert!(matches!(
        cmd!("auth", "login", "codex", "--device"),
        Ok(Command::Auth(ProviderArgs::Login {
            ref provider, ref profile, timeout: 120, device: true
        })) if provider == "codex" && profile == "default"
    ));
    assert!(matches!(
        cmd!("auth", "status", "openai"),
        Ok(Command::Auth(ProviderArgs::Status { ref provider, ref profile }))
            if provider == "openai" && profile == "default"
    ));
    assert!(matches!(
        cmd!("auth", "refresh", "openai"),
        Ok(Command::Auth(ProviderArgs::Refresh { ref provider, ref profile }))
            if provider == "openai" && profile == "default"
    ));
    assert!(matches!(
        cmd!("auth", "login", "openai", "--method", "api-key", "--stdin", "--profile", "work"),
        Ok(Command::Auth(ProviderArgs::ApiKeyLogin { ref provider, ref profile }))
            if provider == "openai" && profile == "work"
    ));
    assert!(matches!(
        cmd!("auth", "login", "openai", "--method", "api-key"),
        Err(ref error) if error.message.contains("requires --stdin")
    ));
}

#[test]
fn auth_login_without_provider_opens_the_selector() {
    assert!(matches!(
        cmd!("auth", "login"),
        Ok(Command::Auth(ProviderArgs::LoginSelect))
    ));
}

#[test]
fn login_selector_includes_configured_custom_provider() {
    let root = clean_test_dir("ctx-auth-login-custom-provider");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(
        fs::write(
            root.join("local.json"),
            r#"{"name":"local","base_url":"http://127.0.0.1:8317/v1","auth":[{"type":"api_key","slot":"default"}]}"#,
        )
        .is_ok()
    );

    let options = login_options_from_dir(&root).unwrap_or_default();

    assert!(options.iter().any(|option| {
        option.provider == "local" && option.method.method == cortexfs::AuthMethod::ApiKey
    }));
}

#[test]
fn login_selector_uses_the_explicit_numbered_choice() {
    let root = clean_test_dir("ctx-auth-login-choice");
    let options = login_options_from_dir(&root).unwrap_or_default();
    let mut output = Vec::new();

    let selected = prompt_login_choice(std::io::Cursor::new("2\n"), &mut output, &options);

    assert_eq!(selected, Ok(Some(1)));
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
        Ok(Command::Provider(ProviderArgs::PresetInstall {
            ref preset,
            name: None,
            base_url: None,
            model: None
        })) if preset == "anthropic"
    ));
    assert!(matches!(
        cmd!(
            "provider",
            "preset",
            "install",
            "compatible",
            "--name",
            "local",
            "--base-url",
            "http://127.0.0.1:11434/v1",
            "--model",
            "llama3"
        ),
        Ok(Command::Provider(ProviderArgs::PresetInstall {
            ref preset,
            ref name,
            ref base_url,
            ref model
        })) if preset == "compatible"
            && name.as_deref() == Some("local")
            && base_url.as_deref() == Some("http://127.0.0.1:11434/v1")
            && model.as_deref() == Some("llama3")
    ));
}

#[test]
fn codex_preset_is_separate_and_responses_only() {
    let codex = provider_preset("codex").map(ProviderPreset::config);
    let openai = provider_preset("openai").map(ProviderPreset::config);
    let groq = provider_preset("groq").map(ProviderPreset::config);
    assert!(
        matches!(codex, Ok(config) if config.contains("chatgpt.com/backend-api/codex") && config.contains(r#""formats": ["openai.responses"]"#))
    );
    assert!(
        matches!(openai, Ok(config) if config.contains("api.openai.com/v1") && !config.contains("oauth"))
    );
    assert!(
        matches!(groq, Ok(config) if config.contains("api.groq.com/openai/v1") && config.contains(r#""name": "groq""#))
    );
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
    assert!(ensure_reference_tree(&root).is_ok());

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
    assert!(ensure_reference_tree(&root).is_ok());
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
