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
    assert!(inspect_agent_control(AgentControlKind::Life, "temp\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Parent, "\n").is_ok());
    assert!(
        inspect_agent_control(
            AgentControlKind::Parent,
            "agent:coder session:default run:r1\n"
        )
        .is_ok()
    );
    assert!(inspect_agent_control(AgentControlKind::Status, "idle\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Pid, "\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Pid, "1234\n").is_ok());
}

#[test]
fn agent_controls_reject_invalid_identity_lifecycle_and_parent() {
    assert_eq!(
        inspect_agent_control(AgentControlKind::Uid, "not-a-uid\n").issues(),
        &[ControlLineIssue::InvalidNumber {
            line: 1,
            value: "not-a-uid".to_owned()
        }]
    );
    assert_eq!(
        inspect_agent_control(AgentControlKind::Groups, "10\nbad\n").issues(),
        &[ControlLineIssue::InvalidNumber {
            line: 2,
            value: "bad".to_owned()
        }]
    );
    assert_eq!(
        inspect_agent_control(AgentControlKind::Life, "detached\n").issues(),
        &[ControlLineIssue::InvalidValue {
            line: 1,
            value: "detached".to_owned()
        }]
    );
    assert_eq!(
        inspect_agent_control(AgentControlKind::Parent, "coder session:default\n").issues(),
        &[ControlLineIssue::InvalidValue {
            line: 1,
            value: "coder session:default".to_owned()
        }]
    );
    assert_eq!(
        inspect_agent_control(
            AgentControlKind::Parent,
            "agent:coder session:default run:r1 run:r2\n"
        )
        .issues(),
        &[ControlLineIssue::InvalidValue {
            line: 1,
            value: "agent:coder session:default run:r1 run:r2".to_owned()
        }]
    );
    assert_eq!(
        inspect_agent_control(AgentControlKind::Status, "running\nextra\n").issues(),
        &[
            ControlLineIssue::InvalidValue {
                line: 1,
                value: "running".to_owned()
            },
            ControlLineIssue::MultipleValues { line: 2 }
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
    assert!(report.issues().contains(&PathLayoutIssue::invalid_value(
        "agent/coder.d/iso".to_owned(),
        "container".to_owned()
    )));
    assert!(report.issues().contains(&PathLayoutIssue::invalid_value(
        "agent/coder.d/uid".to_owned(),
        "bad".to_owned()
    )));
}
use super::*;
