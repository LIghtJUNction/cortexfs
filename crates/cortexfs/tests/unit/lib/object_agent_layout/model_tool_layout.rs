#[test]
fn model_session_control_decides_socket_requirement() {
    let root = clean_test_dir("object-layout-model-session");
    create_complete_object_layout(&root, ObjectClass::Model, "openai/gpt-4o", "none");
    let control = root.join("model").join("openai").join("gpt-4o.d");

    let no_socket = inspect_object_layout(&root, ObjectClass::Model, "openai/gpt-4o");
    assert!(no_socket.is_ok());

    write_text_file(&control.join("session"), "socket\n");
    let missing_socket = inspect_object_layout(&root, ObjectClass::Model, "openai/gpt-4o");
    assert!(missing_socket
        .issues()
        .contains(&ObjectLayoutIssue::MissingSocket(
            "model/openai/gpt-4o.sock".to_owned()
        )));

    write_text_file(&control.join("session"), "native_thread\n");
    let invalid = inspect_object_layout(&root, ObjectClass::Model, "openai/gpt-4o");
    assert!(invalid
        .issues()
        .contains(&ObjectLayoutIssue::InvalidControlValue {
            path: "model/openai/gpt-4o.d/session".to_owned(),
            value: "native_thread".to_owned()
        }));
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
    let legacy = ok!(legacy);
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
    let routed = ok!(routed);
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
    let control = root.join("model").join("openai").join("gpt-4o.d");
    write_text_file(&control.join("cap"), "chat\nnative_thread\n");

    let report = inspect_object_layout(&root, ObjectClass::Model, "openai/gpt-4o");
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::InvalidControlValue {
            path: "model/openai/gpt-4o.d/cap".to_owned(),
            value: "native_thread".to_owned()
        }));
}

#[test]
fn model_object_layout_rejects_invalid_driver_routes() {
    let root = clean_test_dir("object-layout-model-driver");
    create_complete_object_layout(&root, ObjectClass::Model, "openai/gpt-4o", "none");
    let control = root.join("model").join("openai").join("gpt-4o.d");
    write_text_file(&control.join("driver"), "agent=/bin/sh\n");

    let report = inspect_object_layout(&root, ObjectClass::Model, "openai/gpt-4o");
    assert!(report
        .issues()
        .contains(&ObjectLayoutIssue::InvalidControlValue {
            path: "model/openai/gpt-4o.d/driver".to_owned(),
            value: "line 1 invalid driver /bin/sh".to_owned()
        }));
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
}
