#[test]
fn executable_object_bootstrap_validates_controls_and_agent_socket_boundary() {
    let root = clean_test_dir("object-bootstrap-bad");
    let target = root.join("runtime").join("agent");

    write_fixture_file(&target, 0o755);

    assert_eq!(
        install_executable_object_wrapper(
            &root,
            ObjectClass::Tool,
            "bad/name",
            &target.display().to_string(),
            &[],
        ),
        Err(ObjectBootstrapError::InvalidObjectName)
    );
    assert_eq!(
        install_executable_object_wrapper(&root, ObjectClass::Tool, "fs.read", "bad\ncmd", &[]),
        Err(ObjectBootstrapError::InvalidWrapperTarget)
    );
    assert_eq!(
        install_executable_object_wrapper(
            &root,
            ObjectClass::Tool,
            "fs.read",
            &target.display().to_string(),
            &[("authority", "root")],
        ),
        Err(ObjectBootstrapError::InvalidControlFile)
    );
    assert_eq!(
        install_executable_object_wrapper(
            &root,
            ObjectClass::Tool,
            "fs.read",
            &target.display().to_string(),
            &[("schema", "{\"authority\":\"root\"}")],
        ),
        Err(ObjectBootstrapError::InvalidControlValue)
    );

    let agent = install_executable_object_wrapper(
        &root,
        ObjectClass::Agent,
        "coder",
        &target.display().to_string(),
        &[("uid", "1000"), ("gid", "1000"), ("owner", "1000")],
    );
    assert!(agent.is_ok());
    let report = inspect_object_layout(&root, ObjectClass::Agent, "coder");
    assert!(!report.is_ok());
    assert!(report.issues().contains(&ObjectLayoutIssue::MissingSocket(
        "agent/coder.sock".to_owned()
    )));
    let _agent_socket = bind_socket(&root.join("agent").join("coder.sock"));
    assert!(inspect_object_layout(&root, ObjectClass::Agent, "coder").is_ok());
    assert_eq!(ObjectBootstrapError::InvalidControlValue.errno(), "EINVAL");

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn object_layout_accepts_socket_symlink_to_live_unix_socket() {
    let root = clean_test_dir("object-layout-socket-symlink");
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    let runtime_socket = root.join("runtime").join("coder.sock");
    let _listener = bind_socket(&runtime_socket);
    assert!(symlink(runtime_socket, root.join("agent").join("coder.sock")).is_ok());

    assert!(inspect_object_layout(&root, ObjectClass::Agent, "coder").is_ok());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn object_layout_reports_missing_parts() {
    let root = clean_test_dir("object-layout-bad");
    assert!(fs::create_dir_all(root.join("agent")).is_ok());
    write_text_file(&root.join("agent").join("coder"), "#!/bin/sh\n");

    let report = inspect_object_layout(&root, ObjectClass::Agent, "coder");
    assert!(!report.is_ok());
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::NotExecutable("agent/coder".to_owned())));
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::MissingControlDirectory(
            "agent/coder.d".to_owned()
        )));
    assert!(report.issues().contains(&ObjectLayoutIssue::MissingSocket(
        "agent/coder.sock".to_owned()
    )));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn model_session_control_decides_socket_requirement() {
    let root = clean_test_dir("object-layout-model-session");
    create_complete_object_layout(&root, ObjectClass::Model, "openai/gpt-4o", "none");

    let no_socket = inspect_object_layout(&root, ObjectClass::Model, "openai/gpt-4o");
    assert!(no_socket.is_ok());

    write_text_file(
        &root
            .join("model")
            .join("openai")
            .join("gpt-4o.d")
            .join("session"),
        "socket\n",
    );
    let missing_socket = inspect_object_layout(&root, ObjectClass::Model, "openai/gpt-4o");
    assert!(missing_socket
        .issues()
        .contains(&ObjectLayoutIssue::MissingSocket(
            "model/openai/gpt-4o.sock".to_owned()
        )));

    write_text_file(
        &root
            .join("model")
            .join("openai")
            .join("gpt-4o.d")
            .join("session"),
        "native_thread\n",
    );
    let invalid = inspect_object_layout(&root, ObjectClass::Model, "openai/gpt-4o");
    assert!(invalid
        .issues()
        .contains(&ObjectLayoutIssue::InvalidControlValue {
            path: "model/openai/gpt-4o.d/session".to_owned(),
            value: "native_thread".to_owned()
        }));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn model_capabilities_accept_only_stable_words() {
    let valid = inspect_model_capabilities("chat\nstream\ntool_call_syntax\n\n");
    assert!(valid.is_ok());

    let invalid = inspect_model_capabilities("openai_responses\nnative_thread\nvendor_magic\n");
    assert_eq!(
        invalid.issues(),
        &[
            ModelCapabilityIssue::ProviderPrivate {
                line: 1,
                capability: "openai_responses".to_owned()
            },
            ModelCapabilityIssue::ProviderPrivate {
                line: 2,
                capability: "native_thread".to_owned()
            },
            ModelCapabilityIssue::Unknown {
                line: 3,
                capability: "vendor_magic".to_owned()
            }
        ]
    );
}

#[test]
fn model_driver_routes_support_legacy_and_use_case_specific_drivers() {
    let legacy = parse_model_driver_routes("debug\n");
    assert!(legacy.is_ok());
    let Ok(legacy) = legacy else { return };
    assert_eq!(
        legacy.drivers_for(ModelDriverUseCase::Exec),
        Some([String::from("debug")].as_slice())
    );
    assert_eq!(
        legacy.primary_driver_for(ModelDriverUseCase::Agent),
        Some("debug")
    );

    let routed = parse_model_driver_routes(
        "\
default=openai-chat
exec=openai-chat
socket=openai-chat
agent=openai-responses,openai-chat
",
    );
    assert!(routed.is_ok());
    let Ok(routed) = routed else { return };
    assert_eq!(
        routed.drivers_for(ModelDriverUseCase::Exec),
        Some([String::from("openai-chat")].as_slice())
    );
    assert_eq!(
        routed.drivers_for(ModelDriverUseCase::Agent),
        Some([
            String::from("openai-responses"),
            String::from("openai-chat")
        ]
        .as_slice())
    );
    assert_eq!(
        routed.primary_driver_for(ModelDriverUseCase::Socket),
        Some("openai-chat")
    );
}

#[test]
fn model_driver_routes_reject_invalid_route_tables() {
    assert_eq!(
        parse_model_driver_routes("\n# comment\n"),
        Err(ModelDriverRouteError::Empty)
    );
    assert_eq!(
        parse_model_driver_routes("direct=openai-chat\n"),
        Err(ModelDriverRouteError::UnknownUseCase {
            line: 1,
            value: "direct".to_owned()
        })
    );
    assert_eq!(
        parse_model_driver_routes("agent=openai-chat\nagent=openai-responses\n"),
        Err(ModelDriverRouteError::DuplicateUseCase {
            line: 2,
            value: "agent".to_owned()
        })
    );
    assert_eq!(
        parse_model_driver_routes("agent=openai-chat,,openai-responses\n"),
        Err(ModelDriverRouteError::EmptyDriver { line: 1 })
    );
    assert_eq!(
        parse_model_driver_routes("agent=/bin/sh\n"),
        Err(ModelDriverRouteError::InvalidDriverName {
            line: 1,
            value: "/bin/sh".to_owned()
        })
    );
}

#[test]
fn model_object_layout_rejects_provider_private_capabilities() {
    let root = clean_test_dir("object-layout-model-cap");
    create_complete_object_layout(&root, ObjectClass::Model, "openai/gpt-4o", "none");
    write_text_file(
        &root
            .join("model")
            .join("openai")
            .join("gpt-4o.d")
            .join("cap"),
        "chat\nnative_thread\n",
    );

    let report = inspect_object_layout(&root, ObjectClass::Model, "openai/gpt-4o");
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::InvalidControlValue {
            path: "model/openai/gpt-4o.d/cap".to_owned(),
            value: "native_thread".to_owned()
        }));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn model_object_layout_rejects_invalid_driver_routes() {
    let root = clean_test_dir("object-layout-model-driver");
    create_complete_object_layout(&root, ObjectClass::Model, "openai/gpt-4o", "none");
    write_text_file(
        &root
            .join("model")
            .join("openai")
            .join("gpt-4o.d")
            .join("driver"),
        "agent=/bin/sh\n",
    );

    let report = inspect_object_layout(&root, ObjectClass::Model, "openai/gpt-4o");
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::InvalidControlValue {
            path: "model/openai/gpt-4o.d/driver".to_owned(),
            value: "line 1 invalid driver /bin/sh".to_owned()
        }));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn tool_schema_accepts_json_schema_shape_without_authority() {
    let report = inspect_tool_schema_json(
        r#"{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}"#,
    );
    assert!(report.is_ok());
    assert!(report.issues().is_empty());
}

#[test]
fn tool_schema_rejects_invalid_json_and_authority_fields() {
    assert_eq!(
        inspect_tool_schema_json("not-json").issues(),
        &[ToolSchemaIssue::InvalidJson]
    );
    assert_eq!(
        inspect_tool_schema_json("[]").issues(),
        &[ToolSchemaIssue::NotObject]
    );
    assert_eq!(
        inspect_tool_schema_json(r#"{"policy":"allow all","permissions":["tool:*"]}"#).issues(),
        &[
            ToolSchemaIssue::AuthorityField("permissions".to_owned()),
            ToolSchemaIssue::AuthorityField("policy".to_owned())
        ]
    );
}

#[test]
fn tool_object_layout_rejects_authority_shaped_schema() {
    let root = clean_test_dir("object-layout-tool-schema");
    create_complete_object_layout(&root, ObjectClass::Tool, "fs.read", "none");
    write_text_file(
        &root.join("tool").join("fs.read.d").join("schema"),
        "{\"policy\":\"allow all\"}\n",
    );

    let report = inspect_object_layout(&root, ObjectClass::Tool, "fs.read");
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::InvalidControlValue {
            path: "tool/fs.read.d/schema".to_owned(),
            value: "policy".to_owned()
        }));

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn agent_controls_accept_fixed_v1_values() {
    assert!(inspect_agent_control(AgentControlKind::Owner, "1000\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Uid, "1000\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Gid, "100\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Groups, "10\n20\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Groups, "").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Iso, "shared\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Iso, "uid\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Life, "owned\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Parent, "\n").is_ok());
    assert!(inspect_agent_control(
        AgentControlKind::Parent,
        "agent:coder session:default run:r1\n"
    )
    .is_ok());
    assert!(inspect_agent_control(AgentControlKind::Status, "idle\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Pid, "\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Pid, "1234\n").is_ok());
}

#[test]
fn agent_controls_reject_invalid_identity_lifecycle_and_parent() {
    assert_eq!(
        inspect_agent_control(AgentControlKind::Uid, "not-a-uid\n").issues(),
        &[AgentControlIssue::InvalidNumber {
            line: 1,
            value: "not-a-uid".to_owned()
        }]
    );
    assert_eq!(
        inspect_agent_control(AgentControlKind::Groups, "10\nbad\n").issues(),
        &[AgentControlIssue::InvalidNumber {
            line: 2,
            value: "bad".to_owned()
        }]
    );
    assert_eq!(
        inspect_agent_control(AgentControlKind::Life, "detached\n").issues(),
        &[AgentControlIssue::InvalidValue {
            line: 1,
            value: "detached".to_owned()
        }]
    );
    assert_eq!(
        inspect_agent_control(AgentControlKind::Parent, "coder session:default\n").issues(),
        &[AgentControlIssue::InvalidValue {
            line: 1,
            value: "coder session:default".to_owned()
        }]
    );
    assert_eq!(
        inspect_agent_control(AgentControlKind::Status, "running\nextra\n").issues(),
        &[
            AgentControlIssue::InvalidValue {
                line: 1,
                value: "running".to_owned()
            },
            AgentControlIssue::MultipleValues { line: 2 }
        ]
    );
}

#[test]
fn agent_object_layout_rejects_invalid_control_values() {
    let root = clean_test_dir("object-layout-agent-controls");
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    let control = root.join("agent").join("coder.d");
    write_text_file(&control.join("iso"), "container\n");
    write_text_file(&control.join("uid"), "bad\n");

    let report = inspect_object_layout(&root, ObjectClass::Agent, "coder");
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::InvalidControlValue {
            path: "agent/coder.d/iso".to_owned(),
            value: "container".to_owned()
        }));
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::InvalidControlValue {
            path: "agent/coder.d/uid".to_owned(),
            value: "bad".to_owned()
        }));

    let _ignored = fs::remove_dir_all(&root);
}
