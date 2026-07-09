use super::*;
use crate::*;

#[test]
pub(crate) fn parses_tool_command_and_root() {
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
pub(crate) fn builtin_words_preserve_tsh_builtin_argv() {
    assert_eq!(
        builtin_words("tools", vec![OsString::from("-l")]),
        Ok(vec!["tools".to_owned(), "-l".to_owned()])
    );
}

#[test]
pub(crate) fn help_describes_generic_visible_tool_invocation() {
    let help = help_text();

    assert!(help.contains("TOOL [ARG...]    run a visible tool with CLI-style argv and stdio"));
    assert!(!help.contains("fs.read PATH"));
}

#[test]
pub(crate) fn tools_default_collapses_dotted_names_into_groups() {
    let entries = vec![
        test_tool_list_entry("bash"),
        test_tool_list_entry("fs.read"),
        test_tool_list_entry("fs.write"),
        test_tool_list_entry("tmux"),
    ];

    assert_eq!(top_level_tool_names(&entries), vec!["bash", "fs.", "tmux"]);
}

#[test]
pub(crate) fn tools_group_matches_only_children() {
    assert!(tool_is_in_group("fs.read", "fs"));
    assert!(tool_is_in_group("fs.write", "fs."));
    assert!(!tool_is_in_group("fs", "fs"));
    assert!(!tool_is_in_group("fstat", "fs"));
    assert!(!tool_is_in_group("bash", "fs"));
}

#[test]
pub(crate) fn help_tool_describes_diagnostic_flow() {
    let help = tool_diagnostic_help_text();

    assert!(help.contains("tool diagnostics"));
    assert!(help.contains("tools GROUP"));
    assert!(help.contains("which TOOL"));
    assert!(help.contains("CTX_PATH controls visibility"));
}

pub(crate) fn test_tool_list_entry(name: &str) -> ToolListEntry {
    ToolListEntry {
        name: name.to_owned(),
        path: PathBuf::from("/ctx/tool").join(name),
        description: String::new(),
    }
}

#[test]
pub(crate) fn parses_repl_words_without_shell_operators() {
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
pub(crate) fn canonical_repl_reader_accepts_line_at_limit() {
    let input = format!("{}\n", "x".repeat(MAX_TSH_REPL_LINE_BYTES));
    let mut reader = std::io::Cursor::new(input);

    let line = read_repl_line_canonical_from(&mut reader);

    assert_eq!(
        line.map(|line| line.map(|line| line.len())),
        Ok(Some(MAX_TSH_REPL_LINE_BYTES))
    );
}

#[test]
pub(crate) fn canonical_repl_reader_rejects_line_over_limit() {
    let input = format!("{}\n", "x".repeat(MAX_TSH_REPL_LINE_BYTES + 1));
    let mut reader = std::io::Cursor::new(input);

    let line = read_repl_line_canonical_from(&mut reader);

    assert!(matches!(line, Err(ref error) if error.message.contains("exceeds limit")));
}

#[test]
pub(crate) fn parses_tshrc_ctx_path_as_data() {
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
pub(crate) fn rejects_tshrc_ctx_path_outside_ctx_namespace() {
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
pub(crate) fn tshrc_precedence_depends_on_standalone_mode() {
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
    let env_path = format!(
        "{}:{}",
        root.join("tool").display(),
        home.join("tool").display()
    );

    let Ok(tool_path) = ctx_tool_path_with_home(&root, &home, Ok(env_path.clone()), true) else {
        return;
    };

    assert_eq!(tool_path.dirs(), &[home.join("tool"), root.join("tool")]);

    let Ok(tool_path) = ctx_tool_path_with_home(&root, &home, Ok(env_path), false) else {
        return;
    };

    assert_eq!(tool_path.dirs(), &[root.join("tool"), home.join("tool")]);

    let _ignored = fs::remove_dir_all(dir);
}

#[test]
pub(crate) fn standalone_tshrc_abi_paths_are_resolved_under_selected_root() {
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
        .is_ok(),
        "failed to write test .tshrc",
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
pub(crate) fn tshrc_ctx_path_refuses_symlink() {
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
pub(crate) fn tshrc_ctx_path_refuses_symlink_intermediate_directory() {
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
pub(crate) fn parses_tsh_config_as_data() {
    assert_eq!(
        parse_tsh_runtime_config(
            "\
# tsh runtime policy
max_loaded_tools=16
cache_capacity=8
window_percent=25
"
        ),
        Ok(TshRuntimeConfig {
            max_loaded_tools: 16,
            cache_capacity: 8,
            window_percent: 25,
        })
    );
    assert!(parse_tsh_runtime_config("max_loaded_tools=0\n").is_err());
    assert!(parse_tsh_runtime_config("cache_capacity=1025\n").is_err());
    assert!(parse_tsh_runtime_config("window_percent=101\n").is_err());
    assert!(parse_tsh_runtime_config("export cache_capacity=8\n").is_err());
}

#[test]
pub(crate) fn read_tsh_config_text_refuses_symlink_config() {
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
pub(crate) fn open_executable_no_follow_refuses_symlink_tool() {
    let dir = std::env::temp_dir().join(format!(
        "cortexfs-tsh-executable-symlink-{}",
        std::process::id()
    ));
    assert!(fs::create_dir_all(&dir).is_ok());
    let target = dir.join("target");
    let link = dir.join("tool");
    assert!(fs::write(&target, "#!/bin/sh\nexit 0\n").is_ok());
    assert!(fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).is_ok());
    assert!(symlink(&target, &link).is_ok());

    assert!(open_executable_no_follow(&link).is_err());
    let _ignored = fs::remove_dir_all(dir);
}
