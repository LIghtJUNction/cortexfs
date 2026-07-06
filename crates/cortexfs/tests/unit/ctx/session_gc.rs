fn create_agent_session_gc_fixture(root: &Path) -> Result<PathBuf, CliError> {
    let home = ctx_home(root)?;
    let session_root = home.join("agent").join("coder").join("session");
    assert!(fs::create_dir_all(session_root.join("index")).is_ok());
    assert!(fs::write(session_root.join("index/current"), "current\n").is_ok());
    for session in [
        "default",
        "current",
        "e2e-old",
        "smoke-old",
        "e2e-keep",
        "manual",
    ] {
        assert!(fs::create_dir_all(session_root.join(session)).is_ok());
    }
    Ok(session_root)
}

#[test]
fn parses_agent_session_gc_command() {
    let command = cmd!(
        "agent",
        "session",
        "gc",
        "coder",
        "--match",
        "e2e-*",
        "--keep",
        "keep-me",
        "--yes",
        "--older-than-days",
        "7"
    );

    assert!(matches!(
        command,
        Ok(Command::Agent(AgentArgs::SessionGc(AgentSessionGcArgs {
            ref name,
            dry_run: false,
            yes: true,
            ref keep,
            ref patterns,
            older_than_days: Some(7),
        }))) if name == "coder"
            && keep == &vec!["keep-me".to_owned()]
            && patterns == &vec!["e2e-*".to_owned()]
    ));
}

#[test]
fn agent_session_gc_dry_run_keeps_matching_sessions() {
    let root = clean_test_dir("ctx-agent-session-gc-dry-run");
    let session_root = create_agent_session_gc_fixture(&root);
    assert!(session_root.is_ok(), "{session_root:?}");
    let Ok(session_root) = session_root else {
        return;
    };
    let args = AgentSessionGcArgs {
        name: "coder".to_owned(),
        dry_run: true,
        yes: false,
        keep: Vec::new(),
        patterns: vec!["e2e-*".to_owned(), "*smoke*".to_owned()],
        older_than_days: None,
    };

    assert!(agent_session_gc(&root, &args).is_ok());

    for session in ["e2e-old", "smoke-old", "default", "current", "manual"] {
        assert!(session_root.join(session).is_dir(), "{session} should remain");
    }
}

#[test]
fn agent_session_gc_yes_deletes_only_matching_unprotected_sessions() {
    let root = clean_test_dir("ctx-agent-session-gc-delete");
    let session_root = create_agent_session_gc_fixture(&root);
    assert!(session_root.is_ok(), "{session_root:?}");
    let Ok(session_root) = session_root else {
        return;
    };
    let args = AgentSessionGcArgs {
        name: "coder".to_owned(),
        dry_run: false,
        yes: true,
        keep: vec!["e2e-keep".to_owned()],
        patterns: vec!["e2e-*".to_owned(), "*smoke*".to_owned()],
        older_than_days: None,
    };

    assert!(agent_session_gc(&root, &args).is_ok());

    assert!(!session_root.join("e2e-old").exists());
    assert!(!session_root.join("smoke-old").exists());
    for session in ["default", "current", "e2e-keep", "manual"] {
        assert!(session_root.join(session).is_dir(), "{session} should remain");
    }
}
