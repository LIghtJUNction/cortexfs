#[test]
fn agent_executable_socket_runtime_returns_visible_message() {
    let root = unique_test_dir("agent-executable-socket-runtime");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let session_root = root
        .join("home")
        .join("1000")
        .join("agent")
        .join("coder")
        .join("session");
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        r#"#!/bin/sh
run="$CTX_RUN_ID"
if [ -z "$run" ]; then
  run="r1"
fi
input="$*"
printf '{"type":"start","run":"%s","model":"debug/echo"}\n' "$run"
printf '{"type":"delta","run":"%s","text":"%s"}\n' "$run" "$input"
printf '{"type":"done","run":"%s","status":"ok"}\n' "$run"
"#,
    );
    let permissions = fs::metadata(&agent_executable);
    assert!(permissions.is_ok());
    let Ok(metadata) = permissions else { return };
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o755);
    assert!(fs::set_permissions(&agent_executable, permissions).is_ok());
    let pair = UnixStream::pair();
    assert!(pair.is_ok());
    let Ok((mut client, mut socket)) = pair else {
        return;
    };

    assert!(client
        .write_all(
            br#"{"op":"send","id":"msg-1","session":"default","input":"hi"}
"#,
        )
        .is_ok());
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let outcome = serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        AgentExecutableSocketRuntime {
            ctx_root: &root,
            source_root: &root,
            session_root: &session_root,
            default_cwd: "/work",
            model: Some("debug/echo"),
            agent_name: "coder",
            agent_executable: &agent_executable,
        },
    );
    assert!(outcome.is_ok());
    let Ok(outcome) = outcome else { return };
    assert_eq!(outcome.frames().len(), 3);
    assert!(outcome.jsonl().contains("\"type\":\"start\""));
    assert!(outcome.jsonl().contains("\"type\":\"delta\""));
    assert!(outcome.jsonl().contains("\"text\":\"hi\""));
    assert!(outcome.jsonl().contains("\"type\":\"done\""));

    let mut buffer = [0_u8; 512];
    let read = client.read(&mut buffer);
    assert!(read.is_ok());
    let Ok(read) = read else { return };
    let Some(bytes) = buffer.get(..read) else {
        return;
    };
    let response = String::from_utf8_lossy(bytes);
    assert!(response.contains("\"type\":\"delta\""));
    assert!(response.contains("\"text\":\"hi\""));
    let latest = fs::read_to_string(session_root.join("default").join("latest.md"));
    assert!(latest.is_ok());
    assert_eq!(latest.unwrap_or_default(), "hi\n");

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn agent_executable_socket_runtime_passes_source_root() {
    let root = unique_test_dir("agent-executable-socket-runtime-source-root");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let session_root = root
        .join("home")
        .join("1000")
        .join("agent")
        .join("coder")
        .join("session");
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        r#"#!/bin/sh
printf '{"type":"start","run":"%s","agent":"coder"}\n' "$CTX_RUN_ID"
printf '{"type":"delta","run":"%s","text":"%s"}\n' "$CTX_RUN_ID" "$CTX_SOURCE"
printf '{"type":"done","run":"%s","status":"ok"}\n' "$CTX_RUN_ID"
"#,
    );
    let permissions = fs::metadata(&agent_executable);
    assert!(permissions.is_ok());
    let Ok(metadata) = permissions else { return };
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o755);
    assert!(fs::set_permissions(&agent_executable, permissions).is_ok());

    let pair = UnixStream::pair();
    assert!(pair.is_ok());
    let Ok((mut client, mut socket)) = pair else {
        return;
    };
    assert!(client
        .write_all(
            br#"{"op":"send","id":"msg-1","session":"default","input":"hi"}
"#,
        )
        .is_ok());
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let outcome = serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        AgentExecutableSocketRuntime {
            ctx_root: &root,
            source_root: &root,
            session_root: &session_root,
            default_cwd: "/work",
            model: Some("debug/echo"),
            agent_name: "coder",
            agent_executable: &agent_executable,
        },
    );
    assert!(outcome.is_ok());
    let Ok(outcome) = outcome else { return };
    assert!(outcome.jsonl().contains(&format!(
        r#""text":"{}""#,
        root.to_string_lossy()
    )));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn agent_executable_socket_runtime_preserves_jsonl_error_output() {
    let root = unique_test_dir("agent-executable-socket-runtime-error-output");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let session_root = root
        .join("home")
        .join("1000")
        .join("agent")
        .join("coder")
        .join("session");
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        r#"#!/bin/sh
printf '{"type":"start","run":"%s","agent":"coder"}\n' "$CTX_RUN_ID"
printf '{"type":"error","run":"%s","code":"EHOSTDOWN","message":"model unavailable"}\n' "$CTX_RUN_ID"
printf '{"type":"done","run":"%s","status":"error"}\n' "$CTX_RUN_ID"
exit 1
"#,
    );
    let permissions = fs::metadata(&agent_executable).map(|metadata| metadata.permissions());
    assert!(permissions.is_ok());
    let Ok(mut permissions) = permissions else {
        return;
    };
    permissions.set_mode(0o755);
    assert!(fs::set_permissions(&agent_executable, permissions).is_ok());

    let pair = UnixStream::pair();
    assert!(pair.is_ok());
    let Ok((mut client, mut socket)) = pair else {
        return;
    };

    assert!(client
        .write_all(
            br#"{"op":"send","id":"msg-1","session":"default","input":"hi"}
"#,
        )
        .is_ok());
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let outcome = serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        AgentExecutableSocketRuntime {
            ctx_root: &root,
            source_root: &root,
            session_root: &session_root,
            default_cwd: "/work",
            model: Some("debug/echo"),
            agent_name: "coder",
            agent_executable: &agent_executable,
        },
    );
    assert!(outcome.is_ok());
    let Ok(outcome) = outcome else { return };
    assert!(outcome.jsonl().contains("\"code\":\"EHOSTDOWN\""));
    assert!(outcome
        .jsonl()
        .contains("\"message\":\"model unavailable\""));
    assert!(outcome.jsonl().contains("\"status\":\"error\""));

    let mut buffer = [0_u8; 512];
    let read = client.read(&mut buffer);
    assert!(read.is_ok());
    let Ok(read) = read else { return };
    let Some(bytes) = buffer.get(..read) else {
        return;
    };
    let response = String::from_utf8_lossy(bytes);
    assert!(response.contains("\"code\":\"EHOSTDOWN\""));
    assert!(response.contains("\"message\":\"model unavailable\""));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn policy_v0_allows_only_exact_rules() {
    let parsed = PolicyV0::parse(
        "\
allow coder_t tool:fs.read execute
allow coder_t model:debug/echo use
allow coder_t shared:project-a read
",
    );
    assert!(parsed.is_ok());
    let Ok(policy) = parsed else { return };

    assert!(policy.allows(
        "coder_t",
        PolicyObjectClass::Tool,
        "fs.read",
        PolicyPermission::Execute
    ));
    assert!(policy.allows(
        "coder_t",
        PolicyObjectClass::Model,
        "debug/echo",
        PolicyPermission::Use
    ));
    assert!(!policy.allows(
        "coder_t",
        PolicyObjectClass::Tool,
        "shell.exec",
        PolicyPermission::Execute
    ));
    assert!(!policy.allows(
        "reviewer_t",
        PolicyObjectClass::Tool,
        "fs.read",
        PolicyPermission::Execute
    ));
    assert!(!policy.allows(
        "coder_t",
        PolicyObjectClass::Shared,
        "project-a",
        PolicyPermission::Write
    ));
}

#[test]
fn policy_v0_checks_child_authority_subset() {
    let parent = PolicyV0::parse(
        "\
allow coder_t tool:fs.read execute
allow coder_t model:debug/echo use
allow coder_t shared:project-a read
allow coder_t session:default resume
",
    );
    assert!(parent.is_ok());
    let Ok(parent) = parent else { return };

    let child = PolicyV0::parse(
        "\
allow reviewer_t tool:fs.read execute
allow reviewer_t model:debug/echo use
allow reviewer_t shared:project-a read
",
    );
    assert!(child.is_ok());
    let Ok(child) = child else { return };
    assert!(child.is_authority_subset_of(&parent, "reviewer_t", "coder_t"));
    assert!(!child.is_exact_subset_of(&parent));

    let expanded_tool = PolicyV0::parse(
        "\
allow reviewer_t tool:shell.exec execute
",
    );
    assert!(expanded_tool.is_ok());
    let Ok(expanded_tool) = expanded_tool else {
        return;
    };
    assert!(!expanded_tool.is_authority_subset_of(&parent, "reviewer_t", "coder_t"));

    let wrong_subject = PolicyV0::parse(
        "\
allow other_t tool:fs.read execute
",
    );
    assert!(wrong_subject.is_ok());
    let Ok(wrong_subject) = wrong_subject else {
        return;
    };
    assert!(!wrong_subject.is_authority_subset_of(&parent, "reviewer_t", "coder_t"));
}

#[test]
fn policy_v0_rejects_invalid_rules() {
    assert_eq!(
        PolicyRule::parse("deny coder_t tool:fs.read execute"),
        Err(PolicyError::ExpectedAllow)
    );
    assert_eq!(
        PolicyRule::parse("allow coder_t provider:openai use"),
        Err(PolicyError::UnknownClass)
    );
    assert_eq!(
        PolicyRule::parse("allow coder_t tool:fs.read use"),
        Err(PolicyError::UnknownPermission)
    );
    assert_eq!(
        PolicyRule::parse("allow coder_t tool:* execute"),
        Err(PolicyError::InvalidName)
    );
    assert_eq!(
        PolicyRule::parse("allow coder_t tool:fs.read execute extra"),
        Err(PolicyError::WrongFieldCount)
    );
}

#[test]
fn mount_table_parses_fixed_v0_format() {
    let parsed = MountTable::parse(
        "\
/ctx\t/ctx\tro\trbind,nosuid,nodev,noexec
/home/me/project\t/work\trw\trbind,nosuid,nodev
/tmp\t/tmp\trw\t-
",
    );
    assert!(parsed.is_ok());
    let Ok(table) = parsed else { return };
    assert_eq!(table.entries().len(), 3);

    let Some(first) = table.entries().first() else {
        return;
    };
    assert_eq!(first.source(), "/ctx");
    assert_eq!(first.target(), "/ctx");
    assert_eq!(first.mode(), MountMode::ReadOnly);
    assert_eq!(
        first.options(),
        [
            MountOption::RecursiveBind,
            MountOption::NoSuid,
            MountOption::NoDev,
            MountOption::NoExec
        ]
    );

    let Some(last) = table.entries().last() else {
        return;
    };
    assert!(last.options().is_empty());
}

#[test]
fn mount_table_checks_child_attenuation() {
    let parent = MountTable::parse(
        "\
/home/me/project\t/work\trw\trbind,nosuid,nodev
/ctx/shared/project-a\t/shared/project-a\tro\trbind,nosuid,nodev,noexec
",
    );
    assert!(parent.is_ok());
    let Ok(parent) = parent else { return };

    let narrowed = MountTable::parse(
        "\
/home/me/project\t/work\tro\tbind,nosuid,nodev,noexec
/ctx/shared/project-a\t/shared/project-a\tro\tbind,nosuid,nodev,noexec
",
    );
    assert!(narrowed.is_ok());
    let Ok(narrowed) = narrowed else { return };
    assert!(narrowed.is_subset_of(&parent));

    let write_expansion = MountTable::parse(
        "\
/ctx/shared/project-a\t/shared/project-a\trw\tbind,nosuid,nodev,noexec
",
    );
    assert!(write_expansion.is_ok());
    let Ok(write_expansion) = write_expansion else {
        return;
    };
    assert!(!write_expansion.is_subset_of(&parent));

    let removed_safety = MountTable::parse(
        "\
/ctx/shared/project-a\t/shared/project-a\tro\tbind,nosuid,nodev
",
    );
    assert!(removed_safety.is_ok());
    let Ok(removed_safety) = removed_safety else {
        return;
    };
    assert!(!removed_safety.is_subset_of(&parent));

    let hidden_parent_path = MountTable::parse(
        "\
/secret\t/secret\tro\tbind,nosuid,nodev,noexec
",
    );
    assert!(hidden_parent_path.is_ok());
    let Ok(hidden_parent_path) = hidden_parent_path else {
        return;
    };
    assert!(!hidden_parent_path.is_subset_of(&parent));
}

#[test]
fn mount_table_rejects_invalid_v0_format() {
    assert_eq!(
        MountEntry::parse("ctx\t/ctx\tro\trbind"),
        Err(MountError::InvalidPath)
    );
    assert_eq!(
        MountEntry::parse("/ctx\tctx\tro\trbind"),
        Err(MountError::InvalidPath)
    );
    assert_eq!(
        MountEntry::parse("/ctx\t/ctx\tbad\trbind"),
        Err(MountError::InvalidMode)
    );
    assert_eq!(
        MountEntry::parse("/ctx\t/ctx\tro\tbind,rbind"),
        Err(MountError::ConflictingBindOption)
    );
    assert_eq!(
        MountEntry::parse("/ctx\t/ctx\tro\trbind,rbind"),
        Err(MountError::DuplicateOption)
    );
    assert_eq!(
        MountEntry::parse("/ctx\t/ctx\tro\tdev"),
        Err(MountError::InvalidOption)
    );
    assert_eq!(
        MountEntry::parse("/ctx\t/ctx\tro"),
        Err(MountError::WrongFieldCount)
    );
}

