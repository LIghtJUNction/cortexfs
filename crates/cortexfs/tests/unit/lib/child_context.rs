#[test]
fn child_agent_authority_accepts_attenuated_owned_child() {
    let parent_identity = AgentUnixIdentity::new(1000, 100, [10, 20, 30]);
    let child_identity = AgentUnixIdentity::new(1000, 100, [10, 30]);
    let parent_policy = PolicyV0::parse(
        "\
allow coder_t tool:fs.read execute
allow coder_t model:debug/echo use
allow coder_t shared:project-a read
",
    );
    let parent_policy = ok!(parent_policy);
    let child_policy = PolicyV0::parse(
        "\
allow reviewer_t tool:fs.read execute
allow reviewer_t shared:project-a read
",
    );
    let child_policy = ok!(child_policy);
    let parent_mounts = MountTable::parse(
        "\
/work\t/work\trw\trbind,nosuid,nodev
/ctx/shared/project-a\t/shared/project-a\tro\trbind,nosuid,nodev,noexec
",
    );
    let parent_mounts = ok!(parent_mounts);
    let child_mounts = MountTable::parse(
        "\
/work\t/work\tro\tbind,nosuid,nodev,noexec
/ctx/shared/project-a\t/shared/project-a\tro\tbind,nosuid,nodev,noexec
",
    );
    let child_mounts = ok!(child_mounts);

    let request = ChildAgentRequest::new(
        "reviewer",
        "agent:coder session:default run:r123",
        ChildLifecycle::Owned,
        ChildAgentControls::new(&child_identity, "reviewer_t", &child_policy, &child_mounts),
    );
    let authority = ChildAgentAuthority::new(
        "coder",
        &parent_identity,
        "coder_t",
        &parent_policy,
        &parent_mounts,
    );
    assert_eq!(authorize_child_agent(request, authority), Ok(()));

    let temp_request = ChildAgentRequest::new(
        "scratch",
        "agent:coder session:default run:r124",
        ChildLifecycle::Temp,
        ChildAgentControls::new(&child_identity, "reviewer_t", &child_policy, &child_mounts),
    );
    assert_eq!(authorize_child_agent(temp_request, authority), Ok(()));
}

#[test]
fn child_agent_authority_attenuates_worker_prefix_model_policy() {
    let parent_identity = AgentUnixIdentity::new(1000, 100, [10, 20]);
    let child_identity = AgentUnixIdentity::new(1000, 100, [10]);
    let parent_policy = PolicyV0::parse(
        "\
allow coder_t model:api.lmm.best/gpt-5.3-codex-spark use
allow coder_t tool:fs.read execute
",
    );
    let parent_policy = ok!(parent_policy);
    let child_policy = PolicyV0::parse(
        "\
allow worker-fast_t model:api.lmm.best/gpt-5.3-codex-spark use
allow worker-fast_t tool:fs.read execute
",
    );
    let child_policy = ok!(child_policy);
    let expanded_policy = PolicyV0::parse(
        "\
allow worker-fast_t model:openai/gpt-5.5 use
allow worker-fast_t tool:fs.read execute
",
    );
    let expanded_policy = ok!(expanded_policy);
    let parent_mounts = MountTable::parse("/work\t/work\trw\trbind,nosuid,nodev\n");
    let parent_mounts = ok!(parent_mounts);
    let child_mounts = MountTable::parse("/work\t/work\tro\tbind,nosuid,nodev,noexec\n");
    let child_mounts = ok!(child_mounts);
    let authority = ChildAgentAuthority::new(
        "coder",
        &parent_identity,
        "coder_t",
        &parent_policy,
        &parent_mounts,
    );

    let worker = ChildAgentRequest::new(
        "worker-fast",
        "agent:coder session:default run:r125",
        ChildLifecycle::Temp,
        ChildAgentControls::new(&child_identity, "worker-fast_t", &child_policy, &child_mounts),
    );
    assert_eq!(authorize_child_agent(worker, authority), Ok(()));

    let expanded = ChildAgentRequest::new(
        "worker-fast",
        "agent:coder session:default run:r126",
        ChildLifecycle::Temp,
        ChildAgentControls::new(
            &child_identity,
            "worker-fast_t",
            &expanded_policy,
            &child_mounts,
        ),
    );
    assert_eq!(
        authorize_child_agent(expanded, authority),
        Err(ChildAgentDenial::PolicyExpansion)
    );
}

#[test]
fn child_agent_authority_rejects_identity_group_policy_and_mount_expansion() {
    let parent_identity = AgentUnixIdentity::new(1000, 100, [10]);
    let child_identity = AgentUnixIdentity::new(1000, 100, [10]);
    let expanded_identity = AgentUnixIdentity::new(1001, 100, [10]);
    let expanded_groups = AgentUnixIdentity::new(1000, 100, [10, 20]);
    let parent_policy = allow_tool_policy("coder_t", "fs.read");
    let child_policy = allow_tool_policy("reviewer_t", "fs.read");
    let expanded_policy = allow_tool_policy("reviewer_t", "shell.exec");
    let parent_mounts = MountTable::parse("/work\t/work\tro\tbind,nosuid,nodev,noexec\n");
    let parent_mounts = ok!(parent_mounts);
    let child_mounts = MountTable::parse("/work\t/work\tro\tbind,nosuid,nodev,noexec\n");
    let child_mounts = ok!(child_mounts);
    let expanded_mounts = MountTable::parse("/work\t/work\trw\tbind,nosuid,nodev,noexec\n");
    let expanded_mounts = ok!(expanded_mounts);
    let authority = ChildAgentAuthority::new(
        "coder",
        &parent_identity,
        "coder_t",
        &parent_policy,
        &parent_mounts,
    );

    let base = ChildAgentRequest::new(
        "reviewer",
        "agent:coder",
        ChildLifecycle::Owned,
        ChildAgentControls::new(&child_identity, "reviewer_t", &child_policy, &child_mounts),
    );
    assert_eq!(authorize_child_agent(base, authority), Ok(()));

    let identity_request = ChildAgentRequest::new(
        "reviewer",
        "agent:coder",
        ChildLifecycle::Owned,
        ChildAgentControls::new(
            &expanded_identity,
            "reviewer_t",
            &child_policy,
            &child_mounts,
        ),
    );
    assert_eq!(
        authorize_child_agent(identity_request, authority),
        Err(ChildAgentDenial::IdentityExpansion)
    );

    let group_request = ChildAgentRequest::new(
        "reviewer",
        "agent:coder",
        ChildLifecycle::Owned,
        ChildAgentControls::new(&expanded_groups, "reviewer_t", &child_policy, &child_mounts),
    );
    assert_eq!(
        authorize_child_agent(group_request, authority),
        Err(ChildAgentDenial::GroupExpansion)
    );

    let policy_request = ChildAgentRequest::new(
        "reviewer",
        "agent:coder",
        ChildLifecycle::Owned,
        ChildAgentControls::new(
            &child_identity,
            "reviewer_t",
            &expanded_policy,
            &child_mounts,
        ),
    );
    assert_eq!(
        authorize_child_agent(policy_request, authority),
        Err(ChildAgentDenial::PolicyExpansion)
    );

    let mount_request = ChildAgentRequest::new(
        "reviewer",
        "agent:coder",
        ChildLifecycle::Owned,
        ChildAgentControls::new(
            &child_identity,
            "reviewer_t",
            &child_policy,
            &expanded_mounts,
        ),
    );
    assert_eq!(
        authorize_child_agent(mount_request, authority),
        Err(ChildAgentDenial::MountExpansion)
    );
}

#[test]
fn child_agent_authority_rejects_bad_parent_reference_and_lifecycle() {
    let parent_identity = AgentUnixIdentity::new(1000, 100, [10]);
    let child_identity = AgentUnixIdentity::new(1000, 100, [10]);
    let parent_policy = allow_tool_policy("coder_t", "fs.read");
    let child_policy = allow_tool_policy("reviewer_t", "fs.read");
    let parent_mounts = MountTable::parse("/work\t/work\tro\tbind,nosuid,nodev,noexec\n");
    let parent_mounts = ok!(parent_mounts);
    let child_mounts = MountTable::parse("/work\t/work\tro\tbind,nosuid,nodev,noexec\n");
    let child_mounts = ok!(child_mounts);
    let authority = ChildAgentAuthority::new(
        "coder",
        &parent_identity,
        "coder_t",
        &parent_policy,
        &parent_mounts,
    );

    let mismatch = ChildAgentRequest::new(
        "reviewer",
        "agent:planner",
        ChildLifecycle::Owned,
        ChildAgentControls::new(&child_identity, "reviewer_t", &child_policy, &child_mounts),
    );
    assert_eq!(
        authorize_child_agent(mismatch, authority),
        Err(ChildAgentDenial::ParentMismatch)
    );

    let bad_ref = ChildAgentRequest::new(
        "reviewer",
        "parent:coder",
        ChildLifecycle::Owned,
        ChildAgentControls::new(&child_identity, "reviewer_t", &child_policy, &child_mounts),
    );
    assert_eq!(
        authorize_child_agent(bad_ref, authority),
        Err(ChildAgentDenial::InvalidParentRef)
    );

    assert_eq!(
        ChildLifecycle::parse("temp"),
        Ok(ChildLifecycle::Temp)
    );
    assert_eq!(
        ChildLifecycle::parse("detached"),
        Err(ChildAgentDenial::UnsupportedLifecycle)
    );
}

#[test]
fn owned_child_cancellation_records_state_and_events_without_deleting_history() {
    let root = clean_test_dir("owned-child-cancel");
    let parent_session = agent_home(&root, "coder");
    let child_session = agent_home(&root, "rev-123");

    write_text_file(&parent_session.join("events.jsonl"), "");
    create_complete_session_layout(&child_session);
    write_text_file(
        &child_session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"review this\"}\n",
    );
    write_text_file(&child_session.join("events.jsonl"), "");

    let recorded =
        record_owned_child_cancellation("coder", "rev-123", &parent_session, &child_session);
    let events = ok!(recorded);
    assert_file_text(&child_session.join("state"), "cancelled\n");
    assert_file_text(
        &child_session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"review this\"}\n",
    );

    assert_file_text(
        &parent_session.join("events.jsonl"),
        &format!("{}\n", events.parent_event()),
    );
    assert_file_text(
        &child_session.join("events.jsonl"),
        &format!("{}\n", events.child_event()),
    );
    assert!(inspect_event_stream_jsonl(&events.jsonl()).is_ok());
}

#[test]
fn owned_child_cancellation_rejects_bad_names_and_missing_history() {
    let root = clean_test_dir("owned-child-cancel-bad");
    let parent_session = root.join("parent");
    let child_session = root.join("child");

    write_text_file(&parent_session.join("events.jsonl"), "");
    write_text_file(&child_session.join("events.jsonl"), "");
    write_text_file(&child_session.join("state"), "idle\n");

    assert_eq!(
        owned_child_cancellation_events("bad/parent", "rev-123"),
        Err(OwnedChildCancellationError::InvalidParentName)
    );
    assert_eq!(
        record_owned_child_cancellation("coder", "bad/child", &parent_session, &child_session),
        Err(OwnedChildCancellationError::InvalidChildName)
    );
    assert_eq!(
        record_owned_child_cancellation("coder", "rev-123", &parent_session, &child_session),
        Err(OwnedChildCancellationError::MissingChildHistory)
    );
    assert_eq!(
        OwnedChildCancellationError::MissingChildHistory.errno(),
        "ENOENT"
    );
}

#[test]
fn owned_child_cancellation_rejects_symlink_history_files() {
    let root = clean_test_dir("owned-child-cancel-symlink-history");
    let outside = clean_test_dir("owned-child-cancel-symlink-history-outside");
    let parent_session = root.join("parent");
    let child_session = root.join("child");
    assert!(fs::create_dir_all(&parent_session).is_ok());
    write_text_file(&outside.join("events.jsonl"), "");
    write_text_file(&child_session.join("messages.jsonl"), "child message\n");
    write_text_file(&child_session.join("events.jsonl"), "");
    write_text_file(&child_session.join("state"), "idle\n");
    assert!(symlink(
        outside.join("events.jsonl"),
        parent_session.join("events.jsonl")
    )
    .is_ok());

    assert_eq!(
        record_owned_child_cancellation("coder", "rev-123", &parent_session, &child_session),
        Err(OwnedChildCancellationError::MissingParentEvents)
    );
    assert_file_text(&outside.join("events.jsonl"), "");

    assert!(fs::remove_file(parent_session.join("events.jsonl")).is_ok());
    write_text_file(&parent_session.join("events.jsonl"), "");
    assert!(fs::remove_file(child_session.join("messages.jsonl")).is_ok());
    assert!(symlink(
        outside.join("events.jsonl"),
        child_session.join("messages.jsonl")
    )
    .is_ok());

    assert_eq!(
        record_owned_child_cancellation("coder", "rev-123", &parent_session, &child_session),
        Err(OwnedChildCancellationError::MissingChildHistory)
    );
    assert_file_text(&outside.join("events.jsonl"), "");
}

#[test]
fn child_context_recorder_creates_handoff_and_result_channel() {
    let root = clean_test_dir("child-context-record");
    let session = root.join("default");

    create_complete_session_layout(&session);

    let handoff = record_child_handoff_to_parent_context(
        &session,
        "rev-2",
        "reviewer",
        "default",
        "Task: review mount ABI\n",
    );
    assert_eq!(handoff, Ok(()));

    let child = session.join("context").join("child").join("rev-2");
    assert_file_text(&child.join("agent"), "reviewer\n");
    assert_file_text(&child.join("status"), "pending\n");
    assert_file_text(&child.join("handoff.md"), "Task: review mount ABI\n");
    assert!(validate_context_pack_source("context/child/rev-2/handoff.md").is_ok());

    let refs =
        r#"{"id":"r1","path":"artifact/report.md","kind":"artifact","summary":"review report"}"#;
    let result = record_child_result_to_parent_context(
        &session,
        "rev-2",
        ChildContextStatus::Done,
        "Summary: ok",
        refs,
    );
    assert_eq!(result, Ok(()));

    let refs_jsonl = fs::read_to_string(child.join("refs.jsonl"));
    let refs_jsonl = ok!(refs_jsonl);

    assert_file_text(&child.join("result.md"), "Summary: ok\n");
    assert_file_text(&child.join("status"), "done\n");
    assert!(inspect_context_jsonl(ContextJsonlKind::Refs, &refs_jsonl).is_ok());
    assert!(validate_context_pack_source("context/child/rev-2/result.md").is_ok());
    assert!(validate_context_pack_source("context/child/rev-2/refs.jsonl").is_ok());
    assert!(inspect_session_layout(&session).is_ok());
}

#[test]
fn child_context_recorder_rejects_symlink_parent_context() {
    let root = clean_test_dir("child-context-parent-context-symlink");
    let session = root.join("default");
    let outside = clean_test_dir("child-context-parent-context-symlink-outside");
    create_complete_session_layout(&session);
    assert!(fs::remove_dir_all(session.join("context")).is_ok());
    assert!(fs::create_dir_all(outside.join("context")).is_ok());
    assert!(symlink(outside.join("context"), session.join("context")).is_ok());

    assert_eq!(
        record_child_handoff_to_parent_context(
            &session,
            "rev-2",
            "reviewer",
            "default",
            "Task: review mount ABI\n",
        ),
        Err(ChildContextRecordError::MissingParentSession)
    );
    assert!(!outside.join("context").join("child").join("rev-2").exists());
}
