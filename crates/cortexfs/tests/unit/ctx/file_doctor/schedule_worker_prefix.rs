#[test]
fn schedule_handoff_agent_model_defaults_worker_prefixes_to_spark() {
    let root = clean_test_dir("ctx-schedule-missing-worker-prefix-model");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    enable_dynamic_worker_fixture(&root);

    for agent in ["worker-fast", "executor-fast"] {
        let executable = root.join("agent").join(agent);
        assert!(fs::copy(root.join("agent/worker"), &executable).is_ok());
        let metadata = fs::metadata(&executable);
        assert!(metadata.is_ok());
        let Ok(mut permissions) = metadata.map(|metadata| metadata.permissions()) else {
            return;
        };
        permissions.set_mode(0o755);
        assert!(fs::set_permissions(&executable, permissions).is_ok());
        write_text_file(&root.join("agent").join(format!("{agent}.d/life")), "temp\n");

        assert_eq!(
            schedule_handoff_agent_details(&root, agent),
            Ok((
                "api.lmm.best/gpt-5.3-codex-spark".to_owned(),
                "temp".to_owned(),
                "-".to_owned()
            ))
        );
    }
}

#[test]
fn schedule_status_defaults_worker_prefix_to_spark_when_policy_allows_it() {
    for agent in ["worker-fast", "executor-fast"] {
        let root = clean_test_dir(&format!("ctx-schedule-{agent}-status"));
        assert!(ensure_v1_reference_tree(&root).is_ok());
        enable_dynamic_worker_fixture(&root);
        assert!(fs::copy(root.join("agent/worker"), root.join(format!("agent/{agent}"))).is_ok());
        let executable = root.join("agent").join(agent);
        let metadata = fs::metadata(&executable);
        assert!(metadata.is_ok());
        let Ok(mut permissions) = metadata.map(|metadata| metadata.permissions()) else {
            return;
        };
        permissions.set_mode(0o755);
        assert!(fs::set_permissions(&executable, permissions).is_ok());
        write_text_file(&root.join(format!("agent/{agent}.d/life")), "temp\n");
        write_text_file(&root.join("agent/coder.d/label"), "coder\n");
        write_text_file(
            &root.join("agent/coder.d/policy"),
            &format!("allow coder agent:{agent} create\n"),
        );
        let session = fixture_path(
            &root,
            &[
                "home", "1000", "agent", "coder", "session", "default",
            ],
        );
        create_complete_session_layout(&session);
        write_text_file(
            &session.join("context").join("plan.json"),
            &format!(
                r#"{{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {{
      "id": "implement",
      "kind": "react",
      "agent": "{agent}",
      "child": "work-123",
      "handoff": "Task: implement the accepted plan\n",
      "max_steps": 8,
      "requires": [
        {{"class":"agent","name":"{agent}","permission":"create"}}
      ]
    }}
  ]
}}
"#
            ),
        );

        assert_eq!(
            assert_schedule_status_rows(
                &root,
                &[&format!(
                    "implement\treact\t{agent}\twork-123\tdefault\tapi.lmm.best/gpt-5.3-codex-spark\ttemp\tworker\t-\tready"
                )],
            ),
            Ok(())
        );
    }
}
