#[test]
fn agent_runtime_view_derives_identity_environment_policy_and_view() {
    let root = clean_test_dir("agent-runtime-view");
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    let control = root.join("agent").join("coder.d");
    write_text_file(
        &control.join("env"),
        "CTX_ROOT=/ignored\nHOME=/ignored\nRUST_LOG=info\n",
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
    assert_eq!(view.root(), Path::new("/ctx/home/1000/agent/coder/root"));
    assert_eq!(view.cwd(), Path::new("/work"));
    assert_eq!(view.model(), "main");
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
    assert_eq!(
        env_value(view.env(), "CTX_PATH"),
        Some("/ctx/tool:/ctx/home/1000/tool")
    );
    assert_eq!(env_value(view.env(), "RUST_LOG"), Some("info"));
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
    assert_eq!(env_value(view.env(), "AGENT_RULES"), Some("allow"));

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
        ),
    );
    assert_eq!(denied, Err(ToolExecutionDenial::ToolNotFound));
}

#[test]
fn api_key_resolution_prefers_environment_over_keychain() {
    let resolved = resolve_api_key_with(
        "LMM_API_KEY",
        "cortexfs:lmm",
        "default",
        |_name| Ok("env-secret".to_owned()),
        |_service, _account| Ok(Some("keychain-secret".to_owned())),
    );
    assert_eq!(resolved, Ok(Some("env-secret".to_owned())));
}

#[test]
fn api_key_resolution_uses_keychain_when_environment_is_empty_or_missing() {
    let empty_env = resolve_api_key_with(
        "LMM_API_KEY",
        "cortexfs:lmm",
        "default",
        |_name| Ok(" \n".to_owned()),
        |_service, _account| Ok(Some("keychain-secret".to_owned())),
    );
    assert_eq!(empty_env, Ok(Some("keychain-secret".to_owned())));

    let missing_env = resolve_api_key_with(
        "LMM_API_KEY",
        "cortexfs:lmm",
        "default",
        |_name| Err(std::env::VarError::NotPresent),
        |_service, _account| Ok(Some("keychain-secret".to_owned())),
    );
    assert_eq!(missing_env, Ok(Some("keychain-secret".to_owned())));
}

#[test]
fn api_key_resolution_checks_all_environment_candidates_before_keychain() {
    let env_names = vec![
        "PRIMARY_API_KEY".to_owned(),
        "SECONDARY_API_KEY".to_owned(),
    ];
    let resolved = resolve_api_key_from_env_names_with(
        &env_names,
        "cortexfs:test",
        "default",
        |name| {
            if name == "SECONDARY_API_KEY" {
                Ok("env-secret".to_owned())
            } else {
                Err(std::env::VarError::NotPresent)
            }
        },
        |_service, _account| Ok(Some("keychain-secret".to_owned())),
    );
    assert_eq!(resolved, Ok(Some("env-secret".to_owned())));
}

#[test]
fn api_key_resolution_uses_keychain_after_environment_candidates() {
    let env_names = vec![
        "PRIMARY_API_KEY".to_owned(),
        "SECONDARY_API_KEY".to_owned(),
    ];
    let resolved = resolve_api_key_from_env_names_with(
        &env_names,
        "cortexfs:test",
        "default",
        |_name| Err(std::env::VarError::NotPresent),
        |_service, _account| Ok(Some("keychain-secret".to_owned())),
    );
    assert_eq!(resolved, Ok(Some("keychain-secret".to_owned())));
}

#[test]
fn api_key_resolution_reports_unconfigured_without_environment_or_keychain() {
    let resolved = resolve_api_key_with(
        "LMM_API_KEY",
        "cortexfs:lmm",
        "default",
        |_name| Err(std::env::VarError::NotPresent),
        |_service, _account| Ok(None),
    );
    assert_eq!(resolved, Ok(None));

    let invalid = resolve_api_key_with(
        "BAD-NAME",
        "cortexfs:lmm",
        "default",
        |_name| Err(std::env::VarError::NotPresent),
        |_service, _account| Ok(None),
    );
    assert_eq!(invalid, Err(ApiKeyResolutionError::InvalidName));
}

#[test]
fn socket_peer_credentials_come_from_kernel() {
    let pair = UnixStream::pair();
    let (left, right) = ok!(pair);

    let left_peer = ok!(peer_credentials(&left));
    let right_peer = ok!(peer_credentials(&right));

    assert_eq!(left_peer.uid(), right_peer.uid());
    assert_eq!(left_peer.gid(), right_peer.gid());
    assert!(left_peer.pid().is_some());
    assert!(SocketPeerPolicy::uid(left_peer.uid()).allows(left_peer));
    assert!(SocketPeerPolicy::gid(left_peer.gid()).allows(left_peer));
    assert!(SocketPeerPolicy::uid_gid(left_peer.uid(), left_peer.gid()).allows(left_peer));
}

#[test]
fn socket_peer_policy_rejects_mismatched_identity() {
    let peer = PeerCredentials::new(Some(1), 1000, 100);
    assert!(SocketPeerPolicy::uid(1000).allows(peer));
    assert!(SocketPeerPolicy::gid(100).allows(peer));
    assert!(SocketPeerPolicy::uid_gid(1000, 100).allows(peer));
    assert!(!SocketPeerPolicy::uid(1001).allows(peer));
    assert!(!SocketPeerPolicy::gid(101).allows(peer));
    assert!(!SocketPeerPolicy::uid_gid(1000, 101).allows(peer));
}

#[test]
fn socket_request_parser_accepts_stable_request_frames() {
    assert_eq!(
        parse_socket_request_frame(
            r#"{"op":"send","id":"msg-1","session":"default","scope":"shared","cwd":"/work","input":"hello","thread_id":"ignored"}
"#
        ),
        Ok(SocketRequest::Send {
            id: "msg-1".to_owned(),
            session: "default".to_owned(),
            scope: SocketSessionScope::Shared,
            cwd: Some("/work".to_owned()),
            input: "hello".to_owned()
        })
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"resume","session":"default","after":"event-123"}"#),
        Ok(SocketRequest::Resume {
            session: "default".to_owned(),
            after: Some("event-123".to_owned())
        })
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"cancel","id":"run-1"}"#),
        Ok(SocketRequest::Cancel {
            id: "run-1".to_owned()
        })
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"ping"}"#),
        Ok(SocketRequest::Ping)
    );
}

#[test]
fn socket_request_parser_defaults_session_and_scope() {
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"send","id":"msg-1","input":"hello"}"#),
        Ok(SocketRequest::Send {
            id: "msg-1".to_owned(),
            session: "default".to_owned(),
            scope: SocketSessionScope::Private,
            cwd: None,
            input: "hello".to_owned()
        })
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"resume"}"#),
        Ok(SocketRequest::Resume {
            session: "default".to_owned(),
            after: None
        })
    );
    assert_eq!(SocketSessionScope::Temp.as_str(), "temp");
}

#[test]
fn socket_request_parser_reports_stable_errno_for_bad_frames() {
    let oversized = "x".repeat(MAX_SOCKET_FRAME_BYTES + 1);
    let error = parse_socket_request_frame(&oversized);
    assert!(matches!(
        error,
        Err(SocketRequestError::FrameTooLarge { bytes }) if bytes == MAX_SOCKET_FRAME_BYTES + 1
    ));
    assert_eq!(
        error.err().as_ref().map(SocketRequestError::errno),
        Some("EMSGSIZE")
    );

    let invalid = parse_socket_request_frame("{}");
    assert_eq!(invalid, Err(SocketRequestError::MissingOp));
    assert_eq!(
        invalid.err().as_ref().map(SocketRequestError::errno),
        Some("EINVAL")
    );
}

#[test]
fn socket_request_parser_rejects_invalid_ops_and_fields() {
    assert_eq!(
        parse_socket_request_frame(""),
        Err(SocketRequestError::EmptyFrame)
    );
    assert_eq!(
        parse_socket_request_frame("{\"op\":\"ping\"}\n{\"op\":\"ping\"}\n"),
        Err(SocketRequestError::MultipleFrames)
    );
    assert_eq!(
        parse_socket_request_frame("[1]"),
        Err(SocketRequestError::RequestNotObject)
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"native_thread"}"#),
        Err(SocketRequestError::UnknownOp("native_thread".to_owned()))
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"send","id":"bad/id","input":"hello"}"#),
        Err(SocketRequestError::InvalidField {
            field: "id",
            value: "bad/id".to_owned()
        })
    );
    assert_eq!(
        parse_socket_request_frame(
            r#"{"op":"send","id":"msg-1","scope":"global","input":"hello"}"#
        ),
        Err(SocketRequestError::InvalidField {
            field: "scope",
            value: "global".to_owned()
        })
    );
    assert_eq!(
        parse_socket_request_frame(
            r#"{"op":"send","id":"msg-1","cwd":"/work/../secret","input":"hello"}"#
        ),
        Err(SocketRequestError::InvalidField {
            field: "cwd",
            value: "/work/../secret".to_owned()
        })
    );
    assert_eq!(
        parse_socket_request_frame(r#"{"op":"send","id":"msg-1","input":42}"#),
        Err(SocketRequestError::MissingStringField("input"))
    );
}
