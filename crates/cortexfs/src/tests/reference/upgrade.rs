#[test]
fn reference_tree_bootstrap_writes_bootstrap_state() {
    let root = clean_test_dir("reference-tree-bootstrap-state");
    assert!(ensure_reference_tree(&root).is_ok());

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
            "reviewer".to_owned(),
            "worker".to_owned()
        ]
    );
    assert_eq!(
        state.applied_migrations,
        vec![
            MIGRATION_RETIRED_AGENTS.to_owned(),
            MIGRATION_ROLLING_TREE.to_owned(),
            crate::reference::bootstrap::MIGRATION_AGENT_UPDATE.to_owned(),
            crate::reference::bootstrap::MIGRATION_CURRENT_MODELS.to_owned()
        ]
    );
    assert!(root.join(BOOTSTRAP_STATE_REL).is_file());
}

#[test]
fn version_six_plan_records_current_model_refresh() {
    let root = reference_tree("reference-tree-plan-v6-v7");
    write_text_file(
        &root.join(BOOTSTRAP_STATE_REL),
        r#"{"schema":1,"tree_version":6,"managed_agents":["architect","coder","reviewer","worker"],"applied_migrations":["retired-agents","rolling-tree","agent-update"]}"#,
    );

    let plan = plan_reference_tree_upgrade(&root);
    let migrations = plan
        .actions
        .iter()
        .filter_map(|action| match *action {
            BootstrapAction::ApplyMigration { version, id } => Some((version, id)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        migrations,
        vec![(7, crate::reference::bootstrap::MIGRATION_CURRENT_MODELS)]
    );
    assert_eq!(plan.current_version, Some(6));
    assert_eq!(plan.target_version, 7);
    assert!(ensure_reference_tree(&root).is_ok());
    assert!(matches!(
        read_bootstrap_state(&root),
        Some(state)
            if state.tree_version == 7
                && state.applied_migrations
                    == [
                        MIGRATION_RETIRED_AGENTS,
                        MIGRATION_ROLLING_TREE,
                        crate::reference::bootstrap::MIGRATION_AGENT_UPDATE,
                        crate::reference::bootstrap::MIGRATION_CURRENT_MODELS
                    ]
    ));
}

#[test]
fn skipped_version_plan_orders_migrations_through_version_seven() {
    let root = reference_tree("reference-tree-plan-v3-v7");
    write_text_file(
        &root.join(BOOTSTRAP_STATE_REL),
        r#"{"schema":1,"tree_version":3,"managed_agents":["architect","coder","reviewer","worker"],"applied_migrations":[]}"#,
    );

    let migrations = plan_reference_tree_upgrade(&root)
        .actions
        .into_iter()
        .filter_map(|action| match action {
            BootstrapAction::ApplyMigration { version, id } => Some((version, id)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        migrations,
        vec![
            (4, MIGRATION_RETIRED_AGENTS),
            (5, MIGRATION_ROLLING_TREE),
            (6, crate::reference::bootstrap::MIGRATION_AGENT_UPDATE),
            (7, crate::reference::bootstrap::MIGRATION_CURRENT_MODELS)
        ]
    );
}

#[test]
fn bootstrap_retry_rebuilds_unique_deterministic_migration_audit() {
    let root = reference_tree("reference-tree-migration-audit");
    write_text_file(
        &root.join(BOOTSTRAP_STATE_REL),
        r#"{"schema":1,"tree_version":3,"managed_agents":["architect","coder","reviewer","worker"],"applied_migrations":["unknown","retired-agents","retired-agents"]}"#,
    );

    assert!(ensure_reference_tree(&root).is_ok());
    let first = read_bootstrap_state(&root);
    assert!(ensure_reference_tree(&root).is_ok());
    let second = read_bootstrap_state(&root);
    assert_eq!(first, second);
    assert!(matches!(
        second,
        Some(BootstrapState { applied_migrations, .. })
            if applied_migrations == [
                MIGRATION_RETIRED_AGENTS,
                MIGRATION_ROLLING_TREE,
                crate::reference::bootstrap::MIGRATION_AGENT_UPDATE,
                crate::reference::bootstrap::MIGRATION_CURRENT_MODELS
            ]
    ));
}

#[test]
fn future_version_rejects_before_mutating_tree() {
    let root = clean_test_dir("reference-tree-future-version");
    let state = r#"{"schema":1,"tree_version":8,"managed_agents":["future"],"applied_migrations":["future"]}"#;
    write_text_file(&root.join(BOOTSTRAP_STATE_REL), state);
    write_text_file(&root.join("sentinel"), "keep\n");

    let plan = plan_reference_tree_upgrade(&root);
    assert_eq!(
        plan.actions,
        vec![BootstrapAction::RejectVersion {
            current: 8,
            target: REFERENCE_TREE_VERSION
        }]
    );
    assert_eq!(
        ensure_reference_tree(&root),
        Err(ReferenceTreeError::UnsupportedVersion)
    );
    assert_eq!(
        apply_reference_tree_upgrade(&root),
        Err(ReferenceTreeError::UnsupportedVersion)
    );
    assert_eq!(
        crate::write_bootstrap_state(&root),
        Err(ReferenceTreeError::UnsupportedVersion)
    );
    assert_file_text(&root.join("sentinel"), "keep\n");
    assert_file_text(&root.join(BOOTSTRAP_STATE_REL), state);
}

#[test]
fn reference_tree_bootstrap_preserves_unmanaged_worker_for_manual_review() {
    let root = reference_tree("reference-tree-custom-worker");
    write_text_file(
        &root.join(BOOTSTRAP_STATE_REL),
        r#"{"schema":1,"tree_version":3,"managed_agents":["architect","coder","reviewer"],"applied_migrations":["retired-agents"]}"#,
    );
    write_text_file(&root.join("agent/worker"), "custom worker wrapper\n");
    write_text_file(
        &root.join("agent/worker.d/system.md"),
        "custom worker prompt\n",
    );

    assert!(ensure_reference_tree(&root).is_ok());
    assert_file_text(&root.join("agent/worker"), "custom worker wrapper\n");
    assert_file_text(
        &root.join("agent/worker.d/system.md"),
        "custom worker prompt\n",
    );
    let plan = plan_reference_tree_upgrade(&root);
    assert!(plan.actions.iter().any(|action| matches!(
        action,
        BootstrapAction::SkipAgent { name, reason }
            if name == "worker" && reason.contains("existing worker requires manual review")
    )));
    let state = read_bootstrap_state(&root);
    assert!(matches!(
        state,
        Some(state) if state.tree_version == 3
            && !state.managed_agents.iter().any(|name| name == "worker")
    ));
}

#[test]
fn reference_tree_bootstrap_promotes_missing_worker() {
    let root = reference_tree("reference-tree-missing-worker");
    write_text_file(
        &root.join(BOOTSTRAP_STATE_REL),
        r#"{"schema":1,"tree_version":3,"managed_agents":["architect","coder","reviewer"],"applied_migrations":["retired-agents"]}"#,
    );
    assert!(fs::remove_file(root.join("agent/worker")).is_ok());
    assert!(fs::remove_file(root.join("agent/worker.sock")).is_ok());
    assert!(fs::remove_dir_all(root.join("agent/worker.d")).is_ok());

    assert!(ensure_reference_tree(&root).is_ok());
    assert!(root.join("agent/worker").is_file());
    let state = read_bootstrap_state(&root);
    assert!(matches!(
        state,
        Some(state) if state.tree_version == REFERENCE_TREE_VERSION
            && state.managed_agents.iter().any(|name| name == "worker")
    ));
}

#[test]
fn direct_upgrade_does_not_promote_state_before_missing_worker_is_materialized() {
    let root = reference_tree("reference-tree-direct-upgrade-missing-worker");
    write_text_file(
        &root.join(BOOTSTRAP_STATE_REL),
        r#"{"schema":1,"tree_version":3,"managed_agents":["architect","coder","reviewer"],"applied_migrations":["retired-agents"]}"#,
    );
    assert!(fs::remove_file(root.join("agent/worker")).is_ok());
    assert!(fs::remove_file(root.join("agent/worker.sock")).is_ok());
    assert!(fs::remove_dir_all(root.join("agent/worker.d")).is_ok());

    let plan = apply_reference_tree_upgrade(&root);
    assert!(matches!(
        plan,
        Ok(ref plan) if plan.actions.iter().any(|action| matches!(
            action,
            BootstrapAction::EnsureAgent { name } if name == "worker"
        ))
    ));
    let state = read_bootstrap_state(&root);
    assert!(matches!(
        state,
        Some(state) if state.tree_version == 3
            && !state.managed_agents.iter().any(|name| name == "worker")
    ));
}

#[test]
fn reference_tree_bootstrap_keeps_retired_agents_for_manual_review() {
    let root = clean_test_dir("reference-tree-retired-managed-gc");
    assert!(ensure_reference_tree(&root).is_ok());

    // Simulate leftover agents from older reference trees.
    for name in ["base", "executor"] {
        assert!(
            install_executable_object_wrapper(&root, ObjectClass::Agent, name, "/bin/false", &[])
                .is_ok()
        );
        let control = root.join("agent").join(format!("{name}.d"));
        write_text_file(
            &root.join("agent").join(name),
            &format!(
                "#!/bin/sh\n# CortexFS generated object wrapper.\n# cortexfs.object=agent\n# cortexfs.name={name}\nexec /ctx/bin/cortexfs-object-runner \"$0\" \"$@\"\n"
            ),
        );
        write_text_file(
            &control.join("system.md"),
            &reference_agent_system_prompt(name),
        );
        assert!(control.is_dir());
        assert!(root.join("agent").join(name).is_file());
    }
    write_text_file(
        &root.join(BOOTSTRAP_STATE_REL),
        r#"{"schema":1,"tree_version":4,"managed_agents":["architect","coder","reviewer","worker"],"applied_migrations":["retired-agents"]}"#,
    );

    assert!(ensure_reference_tree(&root).is_ok());
    assert!(matches!(
        read_bootstrap_state(&root),
        Some(state) if state.tree_version == REFERENCE_TREE_VERSION
    ));

    let plan = plan_reference_tree_upgrade(&root);
    for name in ["base", "executor"] {
        assert!(root.join("agent").join(name).exists(), "{name} exec");
        assert!(
            root.join("agent").join(format!("{name}.d")).exists(),
            "{name}.d"
        );
        assert!(plan.actions.iter().any(|action| matches!(
            action,
            BootstrapAction::SkipAgent { name: skipped, reason }
                if skipped == name && reason.contains("cannot be proven")
        )));
    }
    // Current agents remain.
    for name in ["architect", "coder", "reviewer", "worker"] {
        assert!(root.join("agent").join(name).is_file());
        assert!(root.join("agent").join(format!("{name}.d")).is_dir());
    }
}

#[test]
fn reference_tree_bootstrap_keeps_diverged_retired_agent() {
    let root = clean_test_dir("reference-tree-retired-diverged");
    assert!(ensure_reference_tree(&root).is_ok());

    assert!(
        install_executable_object_wrapper(&root, ObjectClass::Agent, "base", "/bin/false", &[])
            .is_ok()
    );
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

    assert!(ensure_reference_tree(&root).is_ok());
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
    assert!(ensure_reference_tree(&root).is_ok());

    let plan = plan_reference_tree_upgrade(&root);
    assert!(
        !plan
            .actions
            .iter()
            .any(|action| matches!(action, BootstrapAction::WriteState { .. }))
    );
}

#[test]
fn upgrade_plan_writes_state_when_required_fields_drift() {
    let root = clean_test_dir("reference-tree-bootstrap-state-drift");
    assert!(ensure_reference_tree(&root).is_ok());
    write_text_file(
        &root.join(BOOTSTRAP_STATE_REL),
        r#"{"schema":2,"tree_version":4,"managed_agents":["architect","coder","reviewer","worker"],"applied_migrations":[]}"#,
    );

    let plan = plan_reference_tree_upgrade(&root);
    assert!(
        plan.actions
            .iter()
            .any(|action| matches!(action, BootstrapAction::WriteState { .. }))
    );
    assert!(apply_reference_tree_upgrade(&root).is_ok());
    let refreshed = plan_reference_tree_upgrade(&root);
    assert!(
        !refreshed
            .actions
            .iter()
            .any(|action| matches!(action, BootstrapAction::WriteState { .. }))
    );
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
    assert!(missing.contains(&"worker"));
    assert_eq!(plan.target_version, REFERENCE_TREE_VERSION);
    assert!(plan.current_version.is_none());
}

#[test]
fn reference_tree_worker_uses_default_worker_model() {
    assert_eq!(reference_agent_model("worker"), DEFAULT_WORKER_MODEL);
}

#[test]
fn reference_tree_worker_policy_can_write_source() {
    let policy = reference_agent_policy("worker_t", "worker");
    assert!(
        ["tool:fs.write", "tool:fs.replace", "tool:shell.exec"]
            .iter()
            .all(|permission| policy.contains(permission)),
        "{policy}"
    );
}

#[test]
fn reference_tree_architect_children_include_worker() {
    let children = reference_agent_children("architect");
    assert!(children.contains(&"worker"), "{children:?}");
}

#[test]
fn reference_tree_architect_prompt_assigns_bounded_worker_execution() {
    let prompt = reference_agent_system_prompt("architect");
    assert!(
        prompt.contains("simple bounded execution to `worker`"),
        "{prompt}"
    );
}

#[test]
fn reference_tree_worker_is_not_retired() {
    let root = reference_tree("reference-tree-worker-current");
    let plan = plan_reference_tree_upgrade(&root);
    assert!(!plan.actions.iter().any(|action| matches!(
        action,
        BootstrapAction::SkipAgent { name, .. } if name == "worker"
    )));
}

#[test]
fn reference_tree_worker_prompt_does_not_mention_executor() {
    let prompt = reference_agent_system_prompt("worker");
    assert!(!prompt.contains("executor"), "{prompt}");
}
use super::*;
