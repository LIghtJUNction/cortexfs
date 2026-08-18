#[test]
fn agent_controls_accept_fixed_values() {
    assert!(inspect_agent_control(AgentControlKind::Owner, "1000\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Uid, "1000\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Gid, "100\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Groups, "10\n20\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Groups, "").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Perm, "rw-\n").is_ok());
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
fn agent_window_accepts_auto_or_a_positive_canonical_token_count() {
    assert!(inspect_agent_control(AgentControlKind::Window, "auto\n").is_ok());
    assert!(inspect_agent_control(AgentControlKind::Window, "32768\n").is_ok());
}

#[test]
fn agent_window_rejects_noncanonical_or_out_of_range_values() {
    for invalid in [
        "",
        "0\n",
        "-1\n",
        "+1\n",
        "01\n",
        " 1\n",
        "1 \n",
        "1.0\n",
        "4294967296\n",
        "auto",
        "auto\nextra\n",
    ] {
        assert!(
            !inspect_agent_control(AgentControlKind::Window, invalid).is_ok(),
            "accepted {invalid:?}"
        );
    }
}

#[test]
fn agent_controls_reject_invalid_identity_lifecycle_and_parent() {
    assert!(!inspect_agent_control(AgentControlKind::Perm, "rwx").is_ok());
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
fn agent_tools_control_is_canonical_and_rejects_reserved_or_duplicate_names() {
    assert!(crate::inspect_agent_tools_control("").is_ok());
    assert!(crate::inspect_agent_tools_control("\n").is_ok());
    assert!(crate::inspect_agent_tools_control("fs.read\nexample.echo\n").is_ok());
    for invalid in [
        "fs.read",
        "\nfs.read\n",
        " fs.read\n",
        "fs.read \n",
        "fs.read\nfs.read\n",
        "tsh\n",
        "bad/name\n",
    ] {
        assert!(
            !crate::inspect_agent_tools_control(invalid).is_ok(),
            "{invalid:?}"
        );
    }
}

#[test]
fn agent_object_layout_rejects_invalid_control_values() {
    let root = clean_test_dir("object-layout-agent-controls");
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    let control = root.join("agent").join("coder.d");
    write_text_file(&control.join("iso"), "container\n");
    write_text_file(&control.join("uid"), "bad\n");
    write_text_file(&control.join("approval"), "manual\n");

    let report = inspect_object_layout(&root, ObjectClass::Agent, "coder");
    assert!(report.issues().contains(&PathLayoutIssue::invalid_value(
        "agent/coder.d/iso".to_owned(),
        "invalid content".to_owned()
    )));
    assert!(report.issues().contains(&PathLayoutIssue::invalid_value(
        "agent/coder.d/uid".to_owned(),
        "invalid content".to_owned()
    )));
    assert!(report.issues().contains(&PathLayoutIssue::invalid_value(
        "agent/coder.d/approval".to_owned(),
        "invalid content".to_owned()
    )));
}

#[test]
fn agent_object_layout_rejects_invalid_optional_control_content() {
    let root = clean_test_dir("object-layout-agent-optional-content");
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    let control = root.join("agent/coder.d");
    write_text_file(&control.join("abi"), "future-v1\n");
    write_text_file(&control.join("tools"), "fs.read\nfs.read\n");
    write_text_file(&control.join("meta.json"), "[]\n");
    write_text_file(&control.join("system.md"), "bad\0system\n");
    write_text_file(&control.join("prompt.template.md"), "bad\0prompt\n");

    let report = inspect_object_layout(&root, ObjectClass::Agent, "coder");
    for file in [
        "abi",
        "tools",
        "meta.json",
        "system.md",
        "prompt.template.md",
    ] {
        assert!(report.issues().contains(&PathLayoutIssue::invalid_value(
            format!("agent/coder.d/{file}"),
            "invalid content".to_owned()
        )));
    }
}

#[test]
fn agent_object_layout_reports_bounded_read_failures_stably() {
    let root = clean_test_dir("object-layout-agent-control-read");
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    let control = root.join("agent/coder.d");
    assert!(fs::write(control.join("system.md"), vec![b'x'; 64 * 1024 + 1]).is_ok());
    assert!(fs::write(control.join("prompt.template.md"), [0xff]).is_ok());

    let report = inspect_object_layout(&root, ObjectClass::Agent, "coder");
    for file in ["system.md", "prompt.template.md"] {
        assert!(report.issues().contains(&PathLayoutIssue::invalid_value(
            format!("agent/coder.d/{file}"),
            "invalid content".to_owned()
        )));
    }
}

#[test]
fn agent_object_layout_requires_sdk_envelope_abi() {
    let root = clean_test_dir("object-layout-agent-abi");
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    let control = root.join("agent/coder.d");
    write_text_file(&control.join("abi"), "sdk-envelope-v1\n");
    assert!(inspect_object_layout(&root, ObjectClass::Agent, "coder").is_ok());

    assert!(fs::remove_file(control.join("abi")).is_ok());
    assert!(
        inspect_object_layout(&root, ObjectClass::Agent, "coder")
            .issues()
            .contains(&PathLayoutIssue::missing(
                "agent/coder.d/abi".to_owned(),
                LayoutPathRole::ControlFile,
            ))
    );

    write_text_file(&control.join("abi"), "argv-v1\n");
    assert!(
        inspect_object_layout(&root, ObjectClass::Agent, "coder")
            .issues()
            .contains(&PathLayoutIssue::invalid_value(
                "agent/coder.d/abi".to_owned(),
                "invalid content".to_owned(),
            ))
    );

    write_text_file(&control.join("abi"), "sdk-envelope-v1\n");
    assert!(inspect_object_layout(&root, ObjectClass::Agent, "coder").is_ok());
}

#[test]
fn agent_object_layout_accepts_absent_optional_controls() {
    let root = clean_test_dir("object-layout-agent-optional-absent");
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    let control = root.join("agent/coder.d");
    for file in ["system.md", "prompt.template.md", "meta.json"] {
        assert!(fs::remove_file(control.join(file)).is_ok());
    }

    assert!(inspect_object_layout(&root, ObjectClass::Agent, "coder").is_ok());
}

#[test]
fn agent_object_layout_rejects_wrong_kind_optional_controls() {
    let root = clean_test_dir("object-layout-agent-optional-control-kind");
    create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
    let approval = root.join("agent/coder.d/approval");
    assert!(fs::create_dir_all(&approval).is_ok());

    let report = inspect_object_layout(&root, ObjectClass::Agent, "coder");
    let issue = PathLayoutIssue::wrong_kind(
        "agent/coder.d/approval".to_owned(),
        LayoutPathRole::ControlFile,
    );
    assert!(report.issues().contains(&issue));
    assert_eq!(
        report
            .issues()
            .iter()
            .filter(|candidate| *candidate == &issue)
            .count(),
        1
    );

    assert!(fs::remove_dir(&approval).is_ok());
    let target = root.join("approval-target");
    write_text_file(&target, "auto\n");
    assert!(symlink(target, &approval).is_ok());
    let report = inspect_object_layout(&root, ObjectClass::Agent, "coder");
    assert_eq!(
        report
            .issues()
            .iter()
            .filter(|candidate| *candidate == &issue)
            .count(),
        1
    );
}
use super::*;
