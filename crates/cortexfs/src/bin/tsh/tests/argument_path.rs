#[test]
fn parses_tool_command_and_root() {
    let parsed = parse_args(vec![
        OsString::from("--root"),
        OsString::from("/tmp/ctx"),
        OsString::from("bash"),
        OsString::from("-lc"),
        OsString::from("pwd"),
    ]);
    assert_eq!(
        parsed,
        Ok((
            PathBuf::from("/tmp/ctx"),
            TshCommand::Tool {
                name: "bash".to_owned(),
                args: vec![OsString::from("-lc"), OsString::from("pwd")]
            }
        ))
    );
}

#[test]
fn builtin_words_preserve_tsh_builtin_argv() {
    assert_eq!(
        builtin_words("tools", vec![OsString::from("-l")]),
        Ok(vec!["tools".to_owned(), "-l".to_owned()])
    );
}

#[test]
fn help_describes_generic_visible_tool_invocation() {
    let help = help_text();

    assert!(help.contains("TOOL [ARG...]    run a visible tool with CLI-style argv and stdio"));
    assert!(!help.contains("fs.read PATH"));
}

#[test]
fn get_id_program_returns_absolute_path() {
    assert_eq!(get_id_program(), "/usr/bin/id");
}

#[test]
fn id_command_uses_clean_runtime_environment() {
    let command = id_command();
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let mut envs = command
        .get_envs()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            )
        })
        .collect::<Vec<_>>();
    envs.sort();

    assert_eq!(command.get_program(), "/usr/bin/id");
    assert_eq!(args, vec!["-u".to_owned()]);
    assert_eq!(
        envs,
        vec![("PATH".to_owned(), Some("/usr/bin:/bin".to_owned()))]
    );
}

#[test]
fn parse_current_uid_accepts_digits_only() {
    assert_eq!(parse_current_uid("1000\n"), Ok("1000".to_owned()));
    assert!(parse_current_uid("1000\n1001\n").is_err());
    assert!(parse_current_uid("user\n").is_err());
    assert!(parse_current_uid("\n").is_err());
}

#[test]
fn parses_repl_words_without_shell_operators() {
    assert_eq!(
        parse_repl_line(r#"fs.read '{"path":"/tmp/a b"}'"#),
        Ok(vec![
            "fs.read".to_owned(),
            r#"{"path":"/tmp/a b"}"#.to_owned()
        ])
    );
    assert!(parse_repl_line("bash 'unterminated").is_err());
}

#[test]
fn canonical_repl_reader_accepts_line_at_limit() {
    let input = format!("{}\n", "x".repeat(MAX_TSH_REPL_LINE_BYTES));
    let mut reader = std::io::Cursor::new(input);

    let line = read_repl_line_canonical_from(&mut reader);

    assert_eq!(
        line.map(|line| line.map(|line| line.len())),
        Ok(Some(MAX_TSH_REPL_LINE_BYTES))
    );
}

#[test]
fn canonical_repl_reader_rejects_line_over_limit() {
    let input = format!("{}\n", "x".repeat(MAX_TSH_REPL_LINE_BYTES + 1));
    let mut reader = std::io::Cursor::new(input);

    let line = read_repl_line_canonical_from(&mut reader);

    assert!(matches!(line, Err(ref error) if error.message.contains("exceeds limit")));
}

#[test]
fn parses_tshrc_ctx_path_as_data() {
    assert_eq!(
        parse_tshrc_ctx_path(
            "\
# user tools first for this account
CTX_PATH=/ctx/home/1000/tool:/ctx/tool
"
        ),
        Ok(Some("/ctx/home/1000/tool:/ctx/tool".to_owned()))
    );
    assert_eq!(parse_tshrc_ctx_path("# empty\n\n"), Ok(None));
    assert!(parse_tshrc_ctx_path("export CTX_PATH=/ctx/tool\n").is_err());
    assert!(parse_tshrc_ctx_path("CTX_PATH=\n").is_err());
}

#[test]
fn rejects_tshrc_ctx_path_outside_ctx_namespace() {
    let root = Path::new("/tmp/cortexfs-root");
    let home = root.join("home").join("1000");

    assert!(validate_tshrc_ctx_path("/ctx/tool:/ctx/home/1000/tool", root, &home).is_ok());
    assert!(
        validate_tshrc_ctx_path(
            "/tmp/cortexfs-root/tool:/tmp/cortexfs-root/home/1000/tool",
            root,
            &home,
        )
        .is_ok()
    );
    assert!(validate_tshrc_ctx_path(".", root, &home).is_err());
    assert!(validate_tshrc_ctx_path("/usr/bin", root, &home).is_err());
    assert!(validate_tshrc_ctx_path("/tmp/attacker", root, &home).is_err());
    assert!(validate_tshrc_ctx_path("/ctx/tool::/ctx/home/1000/tool", root, &home).is_err());
}

#[test]
fn standalone_tshrc_ctx_path_takes_precedence_over_process_env() {
    let dir = std::env::temp_dir().join(format!("cortexfs-tsh-ctx-path-{}", std::process::id()));
    let root = dir.join("ctx");
    let home = root.join("home").join("1000");
    assert!(
        fs::create_dir_all(&home).is_ok(),
        "failed to create test home"
    );
    assert!(
        fs::write(
            home.join(".tshrc"),
            "CTX_PATH=/ctx/home/1000/tool:/ctx/tool\n",
        )
        .is_ok(),
        "failed to write test .tshrc"
    );

    let Ok(tool_path) = ctx_tool_path_with_home(
        &root,
        &home,
        Ok(format!(
            "{}:{}",
            root.join("tool").display(),
            home.join("tool").display()
        )),
        true,
    ) else {
        return;
    };

    assert_eq!(tool_path.dirs(), &[home.join("tool"), root.join("tool")]);

    let _ignored = fs::remove_dir_all(dir);
}

#[test]
fn standalone_tshrc_abi_paths_are_resolved_under_selected_root() {
    let dir = std::env::temp_dir().join(format!(
        "cortexfs-tsh-ctx-path-rooted-{}",
        std::process::id()
    ));
    let root = dir.join("ctx");
    let home = root.join("home").join("1000");
    assert!(fs::create_dir_all(home.join("tool")).is_ok());
    assert!(fs::create_dir_all(root.join("tool")).is_ok());
    assert!(
        fs::write(
            home.join(".tshrc"),
            "CTX_PATH=/ctx/tool:/ctx/home/1000/tool\n"
        )
        .is_ok()
    );

    let Ok(tool_path) =
        ctx_tool_path_with_home(&root, &home, Err(std::env::VarError::NotPresent), true)
    else {
        return;
    };

    assert_eq!(tool_path.dirs(), &[root.join("tool"), home.join("tool")]);
    assert!(!tool_path.dirs().contains(&PathBuf::from("/ctx/tool")));

    let _ignored = fs::remove_dir_all(dir);
}

#[test]
fn agent_tsh_process_env_takes_precedence_over_tshrc() {
    let dir = std::env::temp_dir().join(format!(
        "cortexfs-tsh-agent-ctx-path-{}",
        std::process::id()
    ));
    let root = dir.join("ctx");
    let home = root.join("home").join("1000");
    assert!(
        fs::create_dir_all(&home).is_ok(),
        "failed to create test home"
    );
    assert!(
        fs::write(
            home.join(".tshrc"),
            "CTX_PATH=/ctx/home/1000/tool:/ctx/tool\n",
        )
        .is_ok(),
        "failed to write test .tshrc"
    );

    let env_path = format!(
        "{}:{}",
        root.join("tool").display(),
        home.join("tool").display()
    );
    let Ok(tool_path) = ctx_tool_path_with_home(&root, &home, Ok(env_path), false) else {
        return;
    };

    assert_eq!(tool_path.dirs(), &[root.join("tool"), home.join("tool")]);

    let _ignored = fs::remove_dir_all(dir);
}

#[test]
fn tshrc_ctx_path_refuses_symlink() {
    let dir = std::env::temp_dir().join(format!("cortexfs-tshrc-symlink-{}", std::process::id()));
    let root = dir.join("ctx");
    let home = root.join("home").join("1000");
    assert!(fs::create_dir_all(&home).is_ok());
    let outside = dir.join("outside-tshrc");
    assert!(
        fs::write(&outside, "CTX_PATH=/ctx/tool\n").is_ok(),
        "failed to write outside .tshrc"
    );
    assert!(
        symlink(&outside, home.join(".tshrc")).is_ok(),
        "failed to create .tshrc symlink"
    );

    let result = tshrc_ctx_path(&root, &home);

    assert!(matches!(result, Err(error) if error.message.contains("cannot read")));
    let _ignored = fs::remove_dir_all(dir);
}

#[test]
fn tshrc_ctx_path_refuses_symlink_intermediate_directory() {
    let dir = std::env::temp_dir().join(format!(
        "cortexfs-tshrc-symlink-intermediate-{}",
        std::process::id()
    ));
    let root = dir.join("ctx");
    let outside = dir.join("outside-home");
    let home = root.join("home").join("1000");
    assert!(fs::create_dir_all(root.join("home")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(fs::write(outside.join(".tshrc"), "CTX_PATH=/ctx/tool\n").is_ok());
    assert!(symlink(&outside, &home).is_ok());

    let result = tshrc_ctx_path(&root, &home);

    assert!(matches!(result, Err(error) if error.message.contains("cannot read")));
    let _ignored = fs::remove_dir_all(dir);
}

#[test]
fn parses_tsh_config_as_data() {
    assert_eq!(
        parse_tsh_config(
            "\
# tsh runtime policy
max_loaded_tools=16
cache_capacity=8
window_percent=25
"
        ),
        Ok(TshConfig {
            max_loaded_tools: 16,
            cache_capacity: 8,
            window_percent: 25,
        })
    );
    assert!(parse_tsh_config("max_loaded_tools=0\n").is_err());
    assert!(parse_tsh_config("cache_capacity=1025\n").is_err());
    assert!(parse_tsh_config("window_percent=101\n").is_err());
    assert!(parse_tsh_config("export cache_capacity=8\n").is_err());
}

#[test]
fn read_tsh_config_text_refuses_symlink_config() {
    let dir = std::env::temp_dir().join(format!(
        "cortexfs-tsh-config-symlink-{}",
        std::process::id()
    ));
    let root = dir.join("ctx");
    let control_dir = root.join("tool").join("tsh.d");
    assert!(fs::create_dir_all(&control_dir).is_ok());
    let tool = root.join("tool").join("tsh");
    assert!(fs::write(&tool, "#!/bin/sh\nexit 0\n").is_ok());
    assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());
    let outside = dir.join("outside-config");
    assert!(fs::write(&outside, "max_loaded_tools=1\n").is_ok());
    assert!(symlink(&outside, control_dir.join("config")).is_ok());

    let result = read_tsh_config_text(&root);

    assert!(matches!(result, Err(error) if error.message.contains("cannot read")));
    let _ignored = fs::remove_dir_all(dir);
}

#[test]
fn open_executable_no_follow_refuses_symlink_tool() {
    let dir =
        std::env::temp_dir().join(format!("cortexfs-tsh-executable-symlink-{}", std::process::id()));
    assert!(fs::create_dir_all(&dir).is_ok());
    let target = dir.join("target");
    let link = dir.join("tool");
    assert!(fs::write(&target, "#!/bin/sh\nexit 0\n").is_ok());
    assert!(fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).is_ok());
    assert!(symlink(&target, &link).is_ok());

    assert!(open_executable_no_follow(&link).is_err());
    let _ignored = fs::remove_dir_all(dir);
}
