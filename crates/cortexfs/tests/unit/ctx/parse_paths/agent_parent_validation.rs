#[test]
fn agent_new_request_json_accepts_parent_session_and_run_fields() {
    let command = cmd!(
        "agent",
        "new",
        "worker-fast",
        "--parent",
        "agent:executor session:default run:r123"
    );
    assert!(matches!(command, Ok(Command::Agent(AgentArgs::New(_)))));
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };

    assert_eq!(
        agent_new_request_json(&args),
        Ok(
            "{\"name\":\"worker-fast\",\"parent\":\"agent:executor session:default run:r123\"}"
                .to_owned()
        )
    );
}

#[test]
fn agent_new_request_json_rejects_duplicate_parent_session_field() {
    let command = cmd!(
        "agent",
        "new",
        "worker-fast",
        "--parent",
        "agent:executor session:default session:feature"
    );
    assert!(matches!(command, Ok(Command::Agent(AgentArgs::New(_)))));
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };

    assert_eq!(
        agent_new_request_json(&args),
        Err(CliError::usage(
            "invalid agent parent: agent:executor session:default session:feature"
        ))
    );
}

#[test]
fn agent_new_request_json_rejects_duplicate_parent_run_field() {
    let command = cmd!(
        "agent",
        "new",
        "worker-fast",
        "--parent",
        "agent:executor session:default run:r1 run:r2"
    );
    assert!(matches!(command, Ok(Command::Agent(AgentArgs::New(_)))));
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };

    assert_eq!(
        agent_new_request_json(&args),
        Err(CliError::usage(
            "invalid agent parent: agent:executor session:default run:r1 run:r2"
        ))
    );
}

#[test]
fn agent_new_request_json_rejects_parent_session_before_agent() {
    let command = cmd!(
        "agent",
        "new",
        "worker-fast",
        "--parent",
        "session:default agent:executor"
    );
    assert!(matches!(command, Ok(Command::Agent(AgentArgs::New(_)))));
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };

    assert_eq!(
        agent_new_request_json(&args),
        Err(CliError::usage(
            "invalid agent parent: session:default agent:executor"
        ))
    );
}

#[test]
fn agent_new_request_json_rejects_unknown_parent_field() {
    let command = cmd!(
        "agent",
        "new",
        "worker-fast",
        "--parent",
        "agent:executor task:work"
    );
    assert!(matches!(command, Ok(Command::Agent(AgentArgs::New(_)))));
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };

    assert_eq!(
        agent_new_request_json(&args),
        Err(CliError::usage(
            "invalid agent parent: agent:executor task:work"
        ))
    );
}

#[test]
fn agent_new_request_json_rejects_invalid_parent_run_field() {
    let command = cmd!(
        "agent",
        "new",
        "worker-fast",
        "--parent",
        "agent:executor run:bad/name"
    );
    assert!(matches!(command, Ok(Command::Agent(AgentArgs::New(_)))));
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };

    assert_eq!(
        agent_new_request_json(&args),
        Err(CliError::usage(
            "invalid agent parent: agent:executor run:bad/name"
        ))
    );
}
