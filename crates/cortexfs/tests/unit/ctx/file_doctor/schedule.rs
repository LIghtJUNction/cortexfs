#[test]
fn file_check_validates_message_stream_files() {
    let root = clean_test_dir("ctx-messages-check");
    let messages = fixture_path(
        &root,
        &[
            "home",
            "1000",
            "agent",
            "coder",
            "session",
            "default",
            "messages.jsonl",
        ],
    );
    write_text_file(
        &messages,
        "{\"role\":\"assistant\",\"response_id\":\"resp_1\",\"content\":\"hello\"}\n"
    );

    assert_file_check_error_contains(
        &root,
        "home/1000/agent/coder/session/default/messages.jsonl",
        &["provider native field"],
    );

    write_text_file(
        &messages,
        "{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"hello\"}]}\n"
    );
    assert!(file_check(
        &root,
        "home/1000/agent/coder/session/default/messages.jsonl"
    )
    .is_ok());
}

#[test]
fn file_check_validates_context_jsonl_files() {
    let root = clean_test_dir("ctx-context-jsonl-check");
    let context = fixture_path(
        &root,
        &[
            "shared",
            "project-a",
            "agent",
            "coder",
            "session",
            "default",
            "context",
        ],
    );
    assert!(fs::create_dir_all(context.join("swap")).is_ok());
    write_text_file(
        &context.join("facts.jsonl"),
        "{\"id\":\"f1\",\"text\":\"root is frozen\",\"source\":\"messages:1-2\"}\n"
    );
    write_text_file(
        &context.join("swap").join("index.jsonl"),
        "{\"id\":\"sha256-abc\",\"kind\":\"message_range\",\"source\":\"provider_thread\",\"summary\":\"bad\",\"tokens\":\"10\"}\n",
    );

    assert!(file_check(
        &root,
        "shared/project-a/agent/coder/session/default/context/facts.jsonl"
    )
    .is_ok());
    assert_file_check_error_contains(
        &root,
        "shared/project-a/agent/coder/session/default/context/swap/index.jsonl",
        &["invalid context jsonl"],
    );
}

#[test]
fn file_check_validates_agent_schedule_plan_files() {
    let root = clean_test_dir("ctx-agent-schedule-plan-check");
    let control = fixture_path(&root, &["agent", "planner.d"]);
    write_text_file(&control.join("label"), "user_u:agent_r:planner_t:s0\n");
    write_text_file(
        &control.join("policy"),
        "allow planner_t tool:fs.read execute\nallow planner_t agent:reviewer create\n",
    );
    let plan = fixture_path(
        &root,
        &[
            "home",
            "1000",
            "agent",
            "planner",
            "session",
            "default",
            "context",
            "plan.json",
        ],
    );
    write_text_file(
        &plan,
        r#"{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "review",
      "kind": "react",
      "agent": "reviewer",
      "child": "review-child",
      "handoff": "Review the patch.",
      "max_steps": 3,
      "requires": [
        {"class":"tool","name":"fs.read","permission":"execute"},
        {"class":"agent","name":"reviewer","permission":"create"}
      ]
    }
  ]
}
"#,
    );

    assert!(file_check(
        &root,
        "home/1000/agent/planner/session/default/context/plan.json"
    )
    .is_ok());
}

#[test]
fn schedule_advance_materializes_implicit_worker_handoff() {
    let root = clean_test_dir("ctx-schedule-advance-coder-worker");
    let ensured = ensure_v1_reference_tree(&root);
    assert!(ensured.is_ok());
    write_text_file(&root.join("agent/worker.d/life"), "temp\n");
    let session = fixture_path(
        &root,
        &[
            "home", "1000", "agent", "coder", "session", "default",
        ],
    );
    create_complete_session_layout(&session);
    write_worker_schedule_plan(&session);
    assert_eq!(schedule_status_worker(&root), Ok(()));
    assert_worker_schedule_status(&root, "ready");

    let advanced = schedule_command(
        &root,
        &ScheduleArgs::Advance {
            path: "home/1000/agent/coder/session/default/context/plan.json".to_owned(),
            done: Vec::new(),
        },
    );

    assert_eq!(advanced, Ok(()));
    assert_eq!(schedule_status_worker(&root), Ok(()));
    assert_worker_schedule_status(&root, "pending");
    assert_eq!(
        schedule_handoff_agent_model_life(&root, "worker"),
        Ok((
            "api.lmm.best/gpt-5.3-codex-spark".to_owned(),
            "temp".to_owned()
        ))
    );
    assert_eq!(
        schedule_handoff_agent_parent(&root, "worker"),
        Ok("agent:coder".to_owned())
    );
    let child = session.join("context").join("child").join("work-123");
    assert!(matches!(
        fs::read_to_string(child.join("agent")).as_deref(),
        Ok("worker\n")
    ));
    assert!(matches!(
        fs::read_to_string(child.join("session")).as_deref(),
        Ok("default\n")
    ));
    assert!(matches!(
        fs::read_to_string(child.join("status")).as_deref(),
        Ok("pending\n")
    ));
    assert!(matches!(
        fs::read_to_string(child.join("handoff.md")).as_deref(),
        Ok("Task: implement the accepted plan\n")
    ));
    assert_worker_child_row_status(&root, "pending");

    let pending_wait = agent_wait(&root, "coder", Some("default"), "work-123");
    assert!(matches!(
        pending_wait,
        Err(ref error)
            if error.code == 69
                && error.message == "child work-123 is not terminal: pending"
    ));

    assert_worker_claimed_active(&root, &child);
    assert_eq!(schedule_status_worker(&root), Ok(()));
    assert_worker_schedule_status(&root, "active");

    let recorded = schedule_command(
        &root,
        &ScheduleArgs::Result {
            path: "home/1000/agent/coder/session/default/context/plan.json".to_owned(),
            child: "work-123".to_owned(),
            status: ChildContextStatus::Done,
            result: "Worker finished implementation.\n".to_owned(),
            refs_jsonl: "{\"id\":\"ref-abc\",\"path\":\"artifact/output.md\",\"kind\":\"artifact\",\"summary\":\"implementation touched\"}\n"
                .to_owned(),
        },
    );

    assert_eq!(recorded, Ok(()));
    assert_eq!(schedule_status_worker(&root), Ok(()));
    assert_worker_schedule_status(&root, "done");
    assert_worker_claim_rejects_terminal(&root);
    assert!(matches!(
        fs::read_to_string(child.join("status")).as_deref(),
        Ok("done\n")
    ));
    assert!(matches!(
        fs::read_to_string(child.join("result.md")).as_deref(),
        Ok("Worker finished implementation.\n")
    ));
    assert!(matches!(
        fs::read_to_string(child.join("refs.jsonl")).as_deref(),
        Ok("{\"id\":\"ref-abc\",\"path\":\"artifact/output.md\",\"kind\":\"artifact\",\"summary\":\"implementation touched\"}\n")
    ));

    assert_eq!(
        agent_wait(&root, "coder", Some("default"), "work-123"),
        Ok(ExitCode::SUCCESS)
    );

    assert_worker_wait_status(&root, ChildContextStatus::Error, "Worker failed verification.\n", 1);
    assert_worker_wait_status(&root, ChildContextStatus::Cancelled, "Worker was cancelled.\n", 130);

    assert_worker_child_row_status(&root, "cancelled");
    assert_worker_schedule_status(&root, "cancelled");
}

#[test]
fn schedule_status_reaps_active_child_when_worker_pid_is_stale() {
    let root = clean_test_dir("ctx-schedule-status-reaps-stale-worker");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    write_text_file(&root.join("agent/worker.d/life"), "temp\n");
    let session = fixture_path(
        &root,
        &[
            "home", "1000", "agent", "coder", "session", "default",
        ],
    );
    create_complete_session_layout(&session);
    write_worker_schedule_plan(&session);
    let child = session.join("context").join("child").join("work-123");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "worker\n");
    write_text_file(&child.join("session"), "default\n");
    write_text_file(&child.join("status"), "active\n");
    write_text_file(&child.join("handoff.md"), "Task: implement the accepted plan\n");
    write_text_file(&child.join("result.md"), "");
    write_text_file(&child.join("refs.jsonl"), "");
    write_text_file(
        &root.join("agent/worker.d/parent"),
        "agent:coder session:default run:r1\n",
    );
    write_text_file(&root.join("agent/worker.d/status"), "busy\n");
    write_text_file(&root.join("agent/worker.d/pid"), "999999999\n");

    assert_worker_schedule_status(&root, "cancelled");
    assert!(matches!(
        fs::read_to_string(child.join("status")).as_deref(),
        Ok("cancelled\n")
    ));
}

#[test]
fn schedule_handoff_agent_model_rejects_invalid_model_reference() {
    let root = clean_test_dir("ctx-schedule-invalid-worker-model");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    write_text_file(&root.join("agent/worker.d/model"), "../bad\n");

    assert!(matches!(
        schedule_handoff_agent_model_life(&root, "worker"),
        Err(ref error)
            if error.code == 2
                && error.message == "invalid handoff agent model for worker: ../bad"
    ));
}

#[test]
fn schedule_handoff_agent_model_defaults_missing_worker_model_to_spark() {
    let root = clean_test_dir("ctx-schedule-missing-worker-model");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    assert!(fs::remove_file(root.join("agent/worker.d/model")).is_ok());
    write_text_file(&root.join("agent/worker.d/life"), "temp\n");

    assert_eq!(
        schedule_handoff_agent_model_life(&root, "worker"),
        Ok((
            "api.lmm.best/gpt-5.3-codex-spark".to_owned(),
            "temp".to_owned()
        ))
    );
}

#[test]
fn schedule_handoff_agent_model_requires_backing_agent_object() {
    let root = clean_test_dir("ctx-schedule-missing-worker-object");

    assert!(matches!(
        schedule_handoff_agent_model_life(&root, "worker"),
        Err(ref error)
            if error.code == 2
                && error.message.contains("missing handoff agent object worker")
    ));
}

#[test]
fn schedule_status_uses_dash_model_for_local_nodes() {
    let root = clean_test_dir("ctx-schedule-status-local-model");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let session = fixture_path(
        &root,
        &[
            "home", "1000", "agent", "coder", "session", "default",
        ],
    );
    create_complete_session_layout(&session);
    write_text_file(
        &session.join("context").join("plan.json"),
        r#"{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {"id": "plan", "kind": "dag", "agent": "coder"}
  ]
}
"#,
    );

    assert_eq!(
        assert_schedule_status_rows(&root, &["plan\tdag\tcoder\t-\t-\t-\t-\tready"]),
        Ok(())
    );
}

#[test]
fn schedule_parent_ref_output_names_parent_agent_and_session() {
    let root = clean_test_dir("ctx-schedule-parent-ref-output");
    let session = fixture_path(
        &root,
        &[
            "home", "1000", "agent", "coder", "session", "default",
        ],
    );

    assert_eq!(
        schedule_parent_ref_for_output("coder", &session),
        Ok("agent:coder session:default".to_owned())
    );
}

#[test]
fn schedule_child_context_output_paths_name_stable_abi_files() {
    let context = "home/1000/agent/coder/session/default/context";
    assert_eq!(
        schedule_context_abi_path(&format!("{context}/plan.json"), "advance"),
        Ok(context.to_owned())
    );
    assert_eq!(
        schedule_child_context_abi_paths(context, "work-123"),
        Ok(ScheduleChildContextAbiPaths {
            status: "home/1000/agent/coder/session/default/context/child/work-123/status"
                .to_owned(),
            handoff: "home/1000/agent/coder/session/default/context/child/work-123/handoff.md"
                .to_owned(),
            result: "home/1000/agent/coder/session/default/context/child/work-123/result.md"
                .to_owned(),
            refs: "home/1000/agent/coder/session/default/context/child/work-123/refs.jsonl"
                .to_owned(),
        })
    );
    assert!(matches!(
        schedule_child_context_abi_paths(context, "../worker"),
        Err(ref error)
            if error.code == 2
                && error.message == "invalid child context path: invalid child name"
    ));
}
