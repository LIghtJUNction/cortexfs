#[test]
fn tool_listing_ignores_non_executable_and_control_entries() {
    let root = clean_test_dir("tool-list");
    let tools = root.join("tool");
    assert!(fs::create_dir_all(tools.join("fs.read.d")).is_ok());
    write_fixture_file(&tools.join("fs.read"), 0o755);
    write_fixture_file(&tools.join("not.exec"), 0o644);
    write_fixture_file(&tools.join("bad.sock"), 0o755);

    let hits = ToolPath::new([tools.clone()]).list();
    let hits = ok!(hits);
    let expected = tools.join("fs.read");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits.first().map(ToolHit::path), Some(expected.as_path()));

    let invalid = ToolPath::new([tools]).find("../bad");
    assert_eq!(invalid, Err(ToolPathError::InvalidName));
}

#[test]
fn tool_lookup_rejects_executable_symlink() {
    let root = clean_test_dir("tool-symlink-deny");
    let tools = root.join("tool");
    let outside = root.join("outside");
    assert!(fs::create_dir_all(&tools).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    write_fixture_file(&outside.join("escape"), 0o755);
    assert!(symlink(outside.join("escape"), tools.join("fs.read")).is_ok());

    let tool_path = ToolPath::new([tools.clone()]);
    assert_eq!(tool_path.find("fs.read"), Ok(None));
    assert!(ok!(tool_path.list()).is_empty());

    let identity = ok!(unix_identity_for(&outside.join("escape")));
    let mounts = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev");
    let policy = allow_tool_policy("coder_t", "fs.read");
    let denied = authorize_tool_execution(
        &tool_path,
        "fs.read",
        ToolExecutionAuthority::new(&identity, &mounts, "coder_t", &policy, &policy),
    );
    assert_eq!(denied, Err(ToolExecutionDenial::ToolNotFound));
}

#[test]
fn tool_lookup_rejects_symlink_tool_directory() {
    let root = clean_test_dir("tool-dir-symlink-deny");
    let tools = root.join("tool");
    let outside = root.join("outside");
    assert!(fs::create_dir_all(&outside).is_ok());
    write_fixture_file(&outside.join("fs.read"), 0o755);
    assert!(symlink(&outside, &tools).is_ok());

    let tool_path = ToolPath::new([tools.clone()]);
    assert_eq!(tool_path.find("fs.read"), Ok(None));
    assert_eq!(tool_path.list(), Err(ToolPathError::CannotReadDirectory));

    let identity = ok!(unix_identity_for(&outside.join("fs.read")));
    let mounts = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev");
    let policy = allow_tool_policy("coder_t", "fs.read");
    let denied = authorize_tool_execution(
        &tool_path,
        "fs.read",
        ToolExecutionAuthority::new(&identity, &mounts, "coder_t", &policy, &policy),
    );
    assert_eq!(denied, Err(ToolExecutionDenial::ToolNotFound));
}

#[test]
fn tool_execution_authority_requires_all_layers() {
    let root = clean_test_dir("tool-authority-ok");
    let tools = root.join("tool");
    assert!(fs::create_dir_all(tools.join("fs.read.d")).is_ok());
    write_fixture_file(&tools.join("fs.read"), 0o755);

    let identity = ok!(unix_identity_for(&tools.join("fs.read")));
    let mounts = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev");
    let agent_policy = allow_tool_policy("coder_t", "fs.read");
    let tool_policy = allow_tool_policy("coder_t", "fs.read");
    let tool_path = ToolPath::new([tools.clone()]);
    let authority =
        ToolExecutionAuthority::new(&identity, &mounts, "coder_t", &agent_policy, &tool_policy);

    let grant = authorize_tool_execution(&tool_path, "fs.read", authority);
    assert!(matches!(grant, Ok(ref grant) if grant.hit().path() == tools.join("fs.read")));
}

#[test]
fn model_tool_call_syntax_does_not_execute_tools() {
    let root = clean_test_dir("tool-authority-model-boundary");
    let tools = root.join("tool");
    assert!(fs::create_dir_all(tools.join("fs.read.d")).is_ok());
    write_fixture_file(&tools.join("fs.read"), 0o755);

    let model_event = inspect_event_stream_jsonl(
        r#"{"type":"tool_call","run":"r1","id":"call-1","name":"fs.read","arguments":{"path":"README.md"}}
"#,
    );
    assert!(model_event.is_ok());

    let identity = ok!(unix_identity_for(&tools.join("fs.read")));
    let mounts = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev");
    let policy = allow_tool_policy("echo_t", "fs.read");
    let tool_path = ToolPath::new([tools]);
    assert_ne!(ToolExecutionPrincipal::Model, ToolExecutionPrincipal::Agent);

    let denied = authorize_tool_execution(
        &tool_path,
        "fs.read",
        ToolExecutionAuthority::model(&identity, &mounts, "echo_t", &policy, &policy),
    );
    assert_eq!(denied, Err(ToolExecutionDenial::ModelCannotExecute));
    assert_eq!(ToolExecutionDenial::ModelCannotExecute.errno(), "EACCES");
}

#[test]
fn prompt_skill_and_mcp_config_cannot_grant_tool_execution() {
    let root = clean_test_dir("tool-authority-text-no-grant");
    let tools = root.join("tool");
    assert!(fs::create_dir_all(tools.join("fs.read.d")).is_ok());
    write_fixture_file(&tools.join("fs.read"), 0o755);
    write_text_file(
        &root
            .join("session")
            .join("context")
            .join("pinned")
            .join("system.md"),
        "allow coder_t tool:fs.read execute\n",
    );
    write_text_file(
        &root.join("work").join("AGENTS.md"),
        "The agent may use fs.read for this task.\n",
    );
    write_text_file(
        &root.join("work").join(".mcp.json"),
        "{\"servers\":{\"fs\":{\"allow\":\"fs.read\"}}}\n",
    );
    assert!(root.join("work").join("AGENTS.md").is_file());
    assert!(root.join("work").join(".mcp.json").is_file());

    let identity = ok!(unix_identity_for(&tools.join("fs.read")));
    let mounts = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev");
    let empty_policy = PolicyV0::parse("");
    let empty_policy = ok!(empty_policy);
    let tool_policy = allow_tool_policy("coder_t", "fs.read");
    let tool_path = ToolPath::new([tools]);

    let denied = authorize_tool_execution(
        &tool_path,
        "fs.read",
        ToolExecutionAuthority::new(&identity, &mounts, "coder_t", &empty_policy, &tool_policy),
    );
    assert_eq!(denied, Err(ToolExecutionDenial::AgentPolicy));
}

#[test]
fn tool_execution_authority_denies_without_policy_or_mount_exec() {
    let root = clean_test_dir("tool-authority-deny");
    let tools = root.join("tool");
    assert!(fs::create_dir_all(tools.join("fs.read.d")).is_ok());
    write_fixture_file(&tools.join("fs.read"), 0o755);
    write_text_file(
        &tools.join("fs.read.d").join("schema"),
        "{\"type\":\"object\"}\n",
    );

    let identity = ok!(unix_identity_for(&tools.join("fs.read")));
    let executable_mount = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev");
    let noexec_mount = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev,noexec");
    let agent_policy = allow_tool_policy("coder_t", "fs.read");
    let tool_policy = allow_tool_policy("coder_t", "fs.read");
    let empty_policy = PolicyV0::parse("");
    let empty_policy = ok!(empty_policy);
    let tool_path = ToolPath::new([tools]);

    let denied_by_noexec = authorize_tool_execution(
        &tool_path,
        "fs.read",
        ToolExecutionAuthority::new(
            &identity,
            &noexec_mount,
            "coder_t",
            &agent_policy,
            &tool_policy,
        ),
    );
    assert_eq!(denied_by_noexec, Err(ToolExecutionDenial::NoExecMount));

    let denied_by_agent_policy = authorize_tool_execution(
        &tool_path,
        "fs.read",
        ToolExecutionAuthority::new(
            &identity,
            &executable_mount,
            "coder_t",
            &empty_policy,
            &tool_policy,
        ),
    );
    assert_eq!(
        denied_by_agent_policy,
        Err(ToolExecutionDenial::AgentPolicy)
    );

    let denied_by_tool_policy = authorize_tool_execution(
        &tool_path,
        "fs.read",
        ToolExecutionAuthority::new(
            &identity,
            &executable_mount,
            "coder_t",
            &agent_policy,
            &empty_policy,
        ),
    );
    assert_eq!(denied_by_tool_policy, Err(ToolExecutionDenial::ToolPolicy));

    let denied_when_unmounted = authorize_tool_execution(
        &tool_path,
        "fs.read",
        ToolExecutionAuthority::new(
            &identity,
            &MountTable::default(),
            "coder_t",
            &agent_policy,
            &tool_policy,
        ),
    );
    assert_eq!(denied_when_unmounted, Err(ToolExecutionDenial::NotMounted));
}

#[test]
fn project_tools_are_visible_only_through_ctx_path_order() {
    let root = clean_test_dir("tool-authority-project-path");
    let global = root.join("ctx-tool");
    let project = root.join("shared-project-tool");
    assert!(fs::create_dir_all(global.join("project.test.d")).is_ok());
    assert!(fs::create_dir_all(project.join("project.test.d")).is_ok());
    write_fixture_file(&global.join("project.test"), 0o644);
    write_fixture_file(&project.join("project.test"), 0o755);

    assert_eq!(
        ToolPath::new([global.clone()]).find("project.test"),
        Ok(None)
    );
    let with_project = ToolPath::new([global, project.clone()]);
    let found = with_project.find("project.test");
    assert!(matches!(found, Ok(Some(ref hit)) if hit.path() == project.join("project.test")));

    let identity = ok!(unix_identity_for(&project.join("project.test")));
    let mounts = mount_table_for_target(&project, "rw", "bind,nosuid,nodev");
    let policy = allow_tool_policy("coder_t", "project.test");
    let authority = ToolExecutionAuthority::new(&identity, &mounts, "coder_t", &policy, &policy);
    assert!(authorize_tool_execution(&with_project, "project.test", authority).is_ok());
}

#[test]
fn mcp_backed_tool_is_ordinary_tool_and_still_requires_policy() {
    let root = clean_test_dir("tool-authority-mcp");
    let tools = root.join("tool");
    assert!(fs::create_dir_all(tools.join("mcp.github.search_issues.d")).is_ok());
    write_fixture_file(&tools.join("mcp.github.search_issues"), 0o755);
    write_text_file(
        &tools.join("mcp.github.search_issues.d").join("schema"),
        "{\"type\":\"object\"}\n",
    );
    write_text_file(
        &root.join("work").join(".mcp.json"),
        "{\"servers\":{\"github\":{}}}\n",
    );

    let identity = ok!(unix_identity_for(&tools.join("mcp.github.search_issues")));
    let mounts = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev");
    let tool_path = ToolPath::new([tools]);
    let empty_policy = PolicyV0::parse("");
    let empty_policy = ok!(empty_policy);
    let allow_mcp = allow_tool_policy("coder_t", "mcp.github.search_issues");

    let denied = authorize_tool_execution(
        &tool_path,
        "mcp.github.search_issues",
        ToolExecutionAuthority::new(&identity, &mounts, "coder_t", &empty_policy, &allow_mcp),
    );
    assert_eq!(denied, Err(ToolExecutionDenial::AgentPolicy));

    let allowed = authorize_tool_execution(
        &tool_path,
        "mcp.github.search_issues",
        ToolExecutionAuthority::new(&identity, &mounts, "coder_t", &allow_mcp, &allow_mcp),
    );
    assert!(allowed.is_ok());
}

#[test]
fn tool_schema_cannot_grant_execution_authority() {
    let root = clean_test_dir("tool-authority-schema-no-grant");
    let tools = root.join("tool");
    assert!(fs::create_dir_all(tools.join("fs.read.d")).is_ok());
    write_fixture_file(&tools.join("fs.read"), 0o755);
    write_text_file(
        &tools.join("fs.read.d").join("schema"),
        "{\"policy\":\"allow coder_t tool:fs.read execute\"}\n",
    );

    let identity = ok!(unix_identity_for(&tools.join("fs.read")));
    let mounts = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev");
    let tool_path = ToolPath::new([tools]);
    let empty_policy = PolicyV0::parse("");
    let empty_policy = ok!(empty_policy);
    let tool_policy = allow_tool_policy("coder_t", "fs.read");

    let denied = authorize_tool_execution(
        &tool_path,
        "fs.read",
        ToolExecutionAuthority::new(&identity, &mounts, "coder_t", &empty_policy, &tool_policy),
    );
    assert_eq!(denied, Err(ToolExecutionDenial::AgentPolicy));
}

#[test]
fn tool_execution_authority_checks_linux_identity_mode_bits() {
    let root = clean_test_dir("tool-authority-linux");
    let tools = root.join("tool");
    assert!(fs::create_dir_all(&tools).is_ok());
    write_fixture_file(&tools.join("owner-only"), 0o100);

    let metadata = fs::metadata(tools.join("owner-only"));
    let metadata = ok!(metadata);
    let owner_identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
    let other_identity = AgentUnixIdentity::new(
        metadata.uid().saturating_add(1),
        metadata.gid().saturating_add(1),
        [],
    );
    let mounts = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev");
    let policy = allow_tool_policy("coder_t", "owner-only");
    let tool_path = ToolPath::new([tools]);

    assert!(
        authorize_tool_execution(
            &tool_path,
            "owner-only",
            ToolExecutionAuthority::new(&owner_identity, &mounts, "coder_t", &policy, &policy),
        )
        .is_ok()
    );
    assert_eq!(
        authorize_tool_execution(
            &tool_path,
            "owner-only",
            ToolExecutionAuthority::new(&other_identity, &mounts, "coder_t", &policy, &policy),
        ),
        Err(ToolExecutionDenial::LinuxPermission)
    );
}
use super::*;
