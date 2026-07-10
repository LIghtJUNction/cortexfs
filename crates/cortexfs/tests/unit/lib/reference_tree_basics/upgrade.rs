#[test]
fn reference_tree_bootstrap_writes_bootstrap_state() {
    let root = clean_test_dir("reference-tree-bootstrap-state");
    assert!(ensure_v1_reference_tree(&root).is_ok());

    let state = read_bootstrap_state(&root);
    assert!(state.is_some(), "bootstrap state");
    let Some(state) = state else {
        return;
    };
    assert_eq!(state.schema, 1);
    assert_eq!(state.tree_version, REFERENCE_TREE_VERSION);
    assert_eq!(
        state.managed_agents,
        vec![
            "architect".to_owned(),
            "coder".to_owned(),
            "reviewer".to_owned()
        ]
    );
    assert!(
        state
            .applied_migrations
            .iter()
            .any(|value| value == MIGRATION_RETIRED_AGENTS_V1)
    );
    assert!(root.join(BOOTSTRAP_STATE_REL).is_file());
}

#[test]
fn reference_tree_bootstrap_keeps_retired_agents_for_manual_review() {
    let root = clean_test_dir("reference-tree-retired-managed-gc");
    assert!(ensure_v1_reference_tree(&root).is_ok());

    // Simulate leftover agents from older reference trees.
    for name in ["base", "worker", "executor"] {
        assert!(install_executable_object_wrapper(
            &root,
            ObjectClass::Agent,
            name,
            "/bin/false",
            &[]
        )
        .is_ok());
        let control = root.join("agent").join(format!("{name}.d"));
        write_text_file(
            &root.join("agent").join(name),
            &format!(
                "#!/bin/sh\n# CortexFS generated object wrapper.\n# cortexfs.object=agent\n# cortexfs.name={name}\nexec /ctx/bin/cortexfs-object-runner \"$0\" \"$@\"\n"
            ),
        );
        write_text_file(&control.join("system.md"), &reference_agent_system_prompt(name));
        assert!(control.is_dir());
        assert!(root.join("agent").join(name).is_file());
    }

    assert!(ensure_v1_reference_tree(&root).is_ok());

    let plan = plan_reference_tree_upgrade(&root);
    for name in ["base", "worker", "executor"] {
        assert!(root.join("agent").join(name).exists(), "{name} exec");
        assert!(root.join("agent").join(format!("{name}.d")).exists(), "{name}.d");
        assert!(plan.actions.iter().any(|action| matches!(
            action,
            BootstrapAction::SkipAgent { name: skipped, reason }
                if skipped == name && reason.contains("cannot be proven")
        )));
    }
    // Current agents remain.
    for name in ["architect", "coder", "reviewer"] {
        assert!(root.join("agent").join(name).is_file());
        assert!(root.join("agent").join(format!("{name}.d")).is_dir());
    }
}

#[test]
fn reference_tree_bootstrap_keeps_diverged_retired_agent() {
    let root = clean_test_dir("reference-tree-retired-diverged");
    assert!(ensure_v1_reference_tree(&root).is_ok());

    assert!(install_executable_object_wrapper(
        &root,
        ObjectClass::Agent,
        "base",
        "/bin/false",
        &[]
    )
    .is_ok());
    write_text_file(
        &root.join("agent").join("base"),
        "#!/bin/sh\n# CortexFS generated object wrapper.\n# cortexfs.object=agent\n# cortexfs.name=base\nexec /ctx/bin/cortexfs-object-runner \"$0\" \"$@\"\n",
    );
    write_text_file(
        &root.join("agent/base.d/system.md"),
        "User customized base persona.\n",
    );

    let plan = plan_reference_tree_upgrade(&root);
    assert!(plan.actions.iter().any(|action| matches!(
        action,
        BootstrapAction::SkipAgent { name, reason }
            if name == "base" && reason.contains("cannot be proven")
    )));

    assert!(ensure_v1_reference_tree(&root).is_ok());
    assert!(root.join("agent/base").is_file());
    assert!(root.join("agent/base.d").is_dir());
    assert_eq!(
        fs::read_to_string(root.join("agent/base.d/system.md")).unwrap_or_default(),
        "User customized base persona.\n"
    );
}

#[test]
fn successful_bootstrap_has_clean_followup_upgrade_plan() {
    let root = clean_test_dir("reference-tree-bootstrap-clean-followup");
    assert!(ensure_v1_reference_tree(&root).is_ok());

    let plan = plan_reference_tree_upgrade(&root);
    assert!(!plan
        .actions
        .iter()
        .any(|action| matches!(action, BootstrapAction::WriteState { .. })));
}

#[test]
fn upgrade_plan_writes_state_when_required_fields_drift() {
    let root = clean_test_dir("reference-tree-bootstrap-state-drift");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    write_text_file(
        &root.join(BOOTSTRAP_STATE_REL),
        r#"{"schema":2,"tree_version":1,"managed_agents":["architect"],"applied_migrations":[]}"#,
    );

    let plan = plan_reference_tree_upgrade(&root);
    assert!(plan
        .actions
        .iter()
        .any(|action| matches!(action, BootstrapAction::WriteState { .. })));
    assert!(apply_reference_tree_upgrade(&root).is_ok());
    let refreshed = plan_reference_tree_upgrade(&root);
    assert!(!refreshed
        .actions
        .iter()
        .any(|action| matches!(action, BootstrapAction::WriteState { .. })));
}

#[test]
fn plan_reference_tree_upgrade_reports_missing_agents() {
    let root = clean_test_dir("reference-tree-plan-missing");
    assert!(fs::create_dir_all(root.join("agent")).is_ok());

    let plan = plan_reference_tree_upgrade(&root);
    let missing: Vec<_> = plan
        .actions
        .iter()
        .filter_map(|action| match *action {
            BootstrapAction::EnsureAgent { ref name } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert!(missing.contains(&"architect"));
    assert!(missing.contains(&"coder"));
    assert!(missing.contains(&"reviewer"));
    assert_eq!(plan.target_version, REFERENCE_TREE_VERSION);
    assert!(plan.current_version.is_none());
}
