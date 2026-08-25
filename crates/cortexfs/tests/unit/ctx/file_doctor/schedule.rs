#[test]
fn file_check_validates_message_stream_files() {
    let root = clean_test_dir("ctx-messages-check");
    let messages = fixture_path(
        &root,
        &[
            "home",
            "1000",
            "agent",
            "executor",
            "session",
            "default",
            "messages.jsonl",
        ],
    );
    write_text_file(
        &messages,
        "{\"role\":\"assistant\",\"response_id\":\"resp_1\",\"content\":\"hello\"}\n",
    );
    write_text_file(&messages.with_file_name("events.jsonl"), "");

    assert_file_check_error_contains(
        &root,
        "home/1000/agent/executor/session/default/messages.jsonl",
        &["provider native field"],
    );

    write_text_file(
        &messages,
        "{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"hello\"}]}\n",
    );
    assert!(
        file_check(
            &root,
            "home/1000/agent/executor/session/default/messages.jsonl"
        )
        .is_ok()
    );
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
            "executor",
            "session",
            "default",
            "context",
        ],
    );
    assert!(fs::create_dir_all(context.join("swap")).is_ok());
    write_text_file(
        &context.join("facts.jsonl"),
        "{\"id\":\"f1\",\"text\":\"root is frozen\",\"source\":\"messages:1-2\"}\n",
    );
    write_text_file(
        &context.join("swap").join("index.jsonl"),
        "{\"id\":\"sha256-abc\",\"kind\":\"message_range\",\"source\":\"provider_thread\",\"summary\":\"bad\",\"tokens\":\"10\"}\n",
    );

    assert!(
        file_check(
            &root,
            "shared/project-a/agent/executor/session/default/context/facts.jsonl"
        )
        .is_ok()
    );
    assert_file_check_error_contains(
        &root,
        "shared/project-a/agent/executor/session/default/context/swap/index.jsonl",
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

    assert!(
        file_check(
            &root,
            "home/1000/agent/planner/session/default/context/plan.json"
        )
        .is_ok()
    );
}

#[test]
fn schedule_advance_materializes_implicit_worker_handoff() {
    let root = clean_test_dir("ctx-schedule-advance-executor-worker");
    let ensured = ensure_reference_tree(&root);
    assert!(ensured.is_ok());
    enable_dynamic_worker_fixture(&root);
    write_text_file(&root.join("agent/worker.d/life"), "temp\n");
    let session = fixture_path(
        &root,
        &["home", "1000", "agent", "executor", "session", "default"],
    );
    create_complete_session_layout(&session);
    write_worker_schedule_plan(&session);
    assert_eq!(schedule_status_worker(&root), Ok(()));
    assert_worker_schedule_status(&root, "ready");

    let advanced = schedule_command(
        &root,
        &ScheduleArgs::Advance {
            path: "home/1000/agent/executor/session/default/context/plan.json".to_owned(),
            done: Vec::new(),
        },
    );

    assert_eq!(advanced, Ok(()));
    assert_eq!(schedule_status_worker(&root), Ok(()));
    assert_worker_schedule_status(&root, "pending");
    assert_eq!(
        schedule_handoff_agent_details(&root, "worker"),
        Ok((
            "openai/gpt-5.6".to_owned(),
            "temp".to_owned(),
            "agent:executor".to_owned()
        ))
    );
    write_text_file(
        &root.join("agent/worker.d/parent"),
        "agent:executor run:r1 session:default\n",
    );
    assert_eq!(
        schedule_handoff_agent_details(&root, "worker").map(|(_, _, parent)| parent),
        Ok("agent:executor session:default run:r1".to_owned())
    );
    write_text_file(&root.join("agent/worker.d/parent"), "agent:executor\n");
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

    let pending_wait = agent_wait(&root, "executor", Some("default"), "work-123");
    assert!(matches!(
        pending_wait,
        Err(ref error)
            if error.code == 69
                && error.message == "child work-123 is not terminal: pending"
    ));

    assert_worker_claimed_active(&root, &child);
    assert_eq!(schedule_status_worker(&root), Ok(()));
    assert_worker_schedule_status(&root, "active");

    let result = "Worker finished implementation.\n";
    let refs = "{\"id\":\"ref-abc\",\"path\":\"artifact/output.md\",\"kind\":\"artifact\",\"summary\":\"implementation touched\"}\n";
    let recorded = schedule_result_worker(&root, ChildContextStatus::Done, result, refs);

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
        Ok(value) if value == result
    ));
    assert!(matches!(
        fs::read_to_string(child.join("refs.jsonl")).as_deref(),
        Ok(value) if value == refs
    ));

    assert_eq!(
        agent_wait(&root, "executor", Some("default"), "work-123"),
        Ok(ExitCode::SUCCESS)
    );

    let before = child_terminal_snapshot(&child);
    assert_eq!(
        schedule_result_worker(&root, ChildContextStatus::Done, result, refs),
        Ok(())
    );
    assert_eq!(
        child_terminal_snapshot(&child),
        before,
        "exact replay must not rewrite terminal files"
    );

    for (status, changed_result, changed_refs) in [
        (ChildContextStatus::Error, result, refs),
        (ChildContextStatus::Done, "different result\n", refs),
        (ChildContextStatus::Done, result, ""),
    ] {
        assert!(matches!(
            schedule_result_worker(&root, status, changed_result, changed_refs),
            Err(ref error)
                if error.code == 2
                    && error.message == "invalid child context: invalid status transition"
        ));
    }
    assert_worker_child_row_status(&root, "done");
    assert_worker_schedule_status(&root, "done");
}

#[test]
fn schedule_terminal_cancelled_result_cannot_be_replaced_by_done() {
    let root = clean_test_dir("ctx-schedule-cancelled-terminal-replay");
    let child = create_pending_worker_handoff(&root, "cancelled terminal replay");
    assert_worker_claimed_active(&root, &child);
    assert_eq!(
        schedule_result_worker(
            &root,
            ChildContextStatus::Cancelled,
            "Worker was cancelled.\n",
            ""
        ),
        Ok(())
    );

    assert!(matches!(
        schedule_result_worker(&root, ChildContextStatus::Done, "done\n", ""),
        Err(ref error)
            if error.code == 2
                && error.message == "invalid child context: invalid status transition"
    ));
    assert!(matches!(
        fs::read_to_string(child.join("status")).as_deref(),
        Ok("cancelled\n")
    ));
    assert!(matches!(
        fs::read_to_string(child.join("result.md")).as_deref(),
        Ok("Worker was cancelled.\n")
    ));
}

#[test]
fn schedule_claim_and_result_reject_channel_removed_from_current_plan() {
    let root = clean_test_dir("ctx-schedule-current-plan-channel");
    let child = create_pending_worker_handoff(&root, "current plan channel");
    let session = child
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .unwrap_or(&child);
    write_text_file(
        &session.join("context").join("plan.json"),
        r#"{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {"id": "local", "kind": "dag", "agent": "executor"}
  ]
}
"#,
    );

    assert!(matches!(
        schedule_claim_worker(&root),
        Err(ref error)
            if error.code == 2
                && error.message
                    == "invalid child context: child is not delegated by current plan"
    ));
    assert!(matches!(
        schedule_result_worker(&root, ChildContextStatus::Done, "done\n", ""),
        Err(ref error)
            if error.code == 2
                && error.message
                    == "invalid child context: child is not delegated by current plan"
    ));
    assert!(matches!(
        fs::read_to_string(child.join("status")).as_deref(),
        Ok("pending\n")
    ));
}

#[test]
fn schedule_claim_rejects_plan_replaced_after_channel_lease() {
    let root = clean_test_dir("ctx-schedule-claim-plan-race");
    let child = create_pending_worker_handoff(&root, "claim plan race");
    let before = child_terminal_snapshot(&child);
    let plan = worker_plan_path(&child);

    let result = schedule_claim_with_hook(&root, WORKER_PLAN_ABI_PATH, "work-123", || {
        replace_file_for_schedule_race(&plan, REPLACED_WORKER_PLAN)
    });

    assert!(matches!(
        result,
        Err(ref error)
            if error.code == 2
                && error.message
                    == "invalid child context: current plan changed during operation"
    ));
    assert_eq!(child_terminal_snapshot(&child), before);
}

#[test]
fn schedule_claim_rejects_handoff_mutated_after_channel_lease() {
    let root = clean_test_dir("ctx-schedule-claim-handoff-race");
    let child = create_pending_worker_handoff(&root, "claim handoff race");
    let before = child_terminal_snapshot(&child);
    let handoff = child.join("handoff.md");
    let handoff_inode = fs::metadata(&handoff)
        .map(|metadata| metadata.ino())
        .unwrap_or_default();

    let result = schedule_claim_with_hook(&root, WORKER_PLAN_ABI_PATH, "work-123", || {
        write_text_file(&handoff, "Task: mutated after validation\n");
        Ok(())
    });

    assert!(result.is_err());
    assert_eq!(child_terminal_snapshot(&child), before);
    assert_eq!(
        fs::metadata(&handoff)
            .map(|metadata| metadata.ino())
            .unwrap_or_default(),
        handoff_inode
    );
}

#[test]
fn schedule_result_rejects_plan_replaced_after_channel_lease() {
    let root = clean_test_dir("ctx-schedule-result-plan-race");
    let child = create_pending_worker_handoff(&root, "result plan race");
    assert_eq!(schedule_claim_worker(&root), Ok(()));
    let before = child_terminal_snapshot(&child);
    let plan = worker_plan_path(&child);

    let result = schedule_result_with_hook(
        &root,
        WORKER_PLAN_ABI_PATH,
        "work-123",
        ScheduleResultInput {
            status: ChildContextStatus::Done,
            result: "done\n",
            refs_jsonl: "",
        },
        || replace_file_for_schedule_race(&plan, REPLACED_WORKER_PLAN),
    );

    assert!(matches!(
        result,
        Err(ref error)
            if error.code == 2
                && error.message
                    == "invalid child context: current plan changed during operation"
    ));
    assert_eq!(child_terminal_snapshot(&child), before);
}

#[test]
fn schedule_result_rejects_handoff_replaced_after_channel_lease() {
    let root = clean_test_dir("ctx-schedule-result-handoff-race");
    let child = create_pending_worker_handoff(&root, "result handoff race");
    assert_eq!(schedule_claim_worker(&root), Ok(()));
    let before = child_terminal_snapshot(&child);
    let handoff = child.join("handoff.md");
    let handoff_inode = fs::metadata(&handoff)
        .map(|metadata| metadata.ino())
        .unwrap_or_default();

    let result = schedule_result_with_hook(
        &root,
        WORKER_PLAN_ABI_PATH,
        "work-123",
        ScheduleResultInput {
            status: ChildContextStatus::Done,
            result: "done\n",
            refs_jsonl: "",
        },
        || replace_handoff_for_schedule_race(&handoff, "Task: replaced after validation\n"),
    );

    assert!(result.is_err());
    assert_eq!(child_terminal_snapshot(&child), before);
    assert_ne!(
        fs::metadata(&handoff)
            .map(|metadata| metadata.ino())
            .unwrap_or_default(),
        handoff_inode
    );
}

#[test]
fn schedule_claim_rejects_handoff_that_does_not_match_current_plan() {
    let root = clean_test_dir("ctx-schedule-current-plan-handoff");
    let child = create_pending_worker_handoff(&root, "current plan handoff");
    write_text_file(&child.join("handoff.md"), "Task: stale plan\n");

    assert!(matches!(
        schedule_claim_worker(&root),
        Err(ref error)
            if error.code == 2
                && error.message == "invalid child context: current plan handoff mismatch"
    ));
    assert!(matches!(
        fs::read_to_string(child.join("status")).as_deref(),
        Ok("pending\n")
    ));
}

#[test]
fn schedule_claim_rejects_child_agent_that_does_not_match_current_plan() {
    let root = clean_test_dir("ctx-schedule-current-plan-agent");
    let child = create_pending_worker_handoff(&root, "current plan agent");
    write_text_file(&child.join("agent"), "reviewer\n");

    assert!(matches!(
        schedule_claim_worker(&root),
        Err(ref error)
            if error.code == 2
                && error.message == "invalid child context: current plan handoff mismatch"
    ));
    assert!(matches!(
        fs::read_to_string(child.join("status")).as_deref(),
        Ok("pending\n")
    ));
}

#[test]
fn schedule_result_rejects_child_session_that_does_not_match_current_plan() {
    let root = clean_test_dir("ctx-schedule-current-plan-session");
    let child = create_pending_worker_handoff(&root, "current plan session");
    write_text_file(&child.join("session"), "stale\n");

    assert!(matches!(
        schedule_result_worker(&root, ChildContextStatus::Done, "done\n", ""),
        Err(ref error)
            if error.code == 2
                && error.message == "invalid child context: current plan handoff mismatch"
    ));
    assert!(matches!(
        fs::read_to_string(child.join("status")).as_deref(),
        Ok("pending\n")
    ));
    assert!(matches!(
        fs::read_to_string(child.join("result.md")).as_deref(),
        Ok("")
    ));
}

#[test]
fn schedule_claim_rejects_invalid_child_agent_without_claiming() {
    let root = clean_test_dir("ctx-schedule-claim-invalid-child-agent");
    let child = create_pending_worker_handoff(&root, "claim invalid child agent");
    write_text_file(&child.join("agent"), "../bad\n");

    assert!(matches!(
        schedule_claim_worker(&root),
        Err(ref error)
            if error.code == 2
                && error.message == "invalid child context: invalid agent name"
    ));
    assert!(matches!(
        fs::read_to_string(child.join("status")).as_deref(),
        Ok("pending\n")
    ));
}

#[test]
fn schedule_result_rejects_invalid_child_session_without_recording() {
    let root = clean_test_dir("ctx-schedule-result-invalid-child-session");
    let child = create_pending_worker_handoff(&root, "result invalid child session");
    write_text_file(&child.join("session"), "../bad\n");

    assert!(matches!(
        schedule_command(&root, &ScheduleArgs::Result {
            path: "home/1000/agent/executor/session/default/context/plan.json".to_owned(),
            child: "work-123".to_owned(),
            status: ChildContextStatus::Done,
            result: "done\n".to_owned(),
            refs_jsonl: String::new(),
        }),
        Err(ref error)
            if error.code == 2
                && error.message == "invalid child context: invalid session name"
    ));
    assert!(matches!(
        fs::read_to_string(child.join("status")).as_deref(),
        Ok("pending\n")
    ));
    assert!(matches!(
        fs::read_to_string(child.join("result.md")).as_deref(),
        Ok("")
    ));
    assert!(matches!(
        fs::read_to_string(child.join("refs.jsonl")).as_deref(),
        Ok("")
    ));
}

#[test]
fn schedule_result_rejects_invalid_backing_parent_without_recording() {
    let root = clean_test_dir("ctx-schedule-bad-parent");
    let child = create_pending_worker_handoff(&root, "result invalid backing parent");
    write_text_file(&root.join("agent/worker.d/parent"), "session:default\n");

    assert!(matches!(
        schedule_command(&root, &ScheduleArgs::Result {
            path: "home/1000/agent/executor/session/default/context/plan.json".to_owned(),
            child: "work-123".to_owned(),
            status: ChildContextStatus::Done,
            result: "done\n".to_owned(),
            refs_jsonl: String::new(),
        }),
        Err(ref error)
            if error.code == 2
                && error.message == "invalid agent parent for worker: session:default"
    ));
    assert!(matches!(
        fs::read_to_string(child.join("status")).as_deref(),
        Ok("pending\n")
    ));
    assert!(matches!(
        fs::read_to_string(child.join("result.md")).as_deref(),
        Ok("")
    ));
    assert!(matches!(
        fs::read_to_string(child.join("refs.jsonl")).as_deref(),
        Ok("")
    ));
}

#[test]
fn schedule_status_reaps_active_child_when_worker_pid_is_stale() {
    let root = clean_test_dir("ctx-schedule-status-reaps-stale-worker");
    assert!(ensure_reference_tree(&root).is_ok());
    enable_dynamic_worker_fixture(&root);
    write_text_file(&root.join("agent/worker.d/life"), "temp\n");
    let session = fixture_path(
        &root,
        &["home", "1000", "agent", "executor", "session", "default"],
    );
    create_complete_session_layout(&session);
    write_worker_schedule_plan(&session);
    let child = session.join("context").join("child").join("work-123");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    write_text_file(&child.join("agent"), "worker\n");
    write_text_file(&child.join("session"), "default\n");
    write_text_file(&child.join("status"), "active\n");
    write_text_file(
        &child.join("handoff.md"),
        "Task: implement the accepted plan\n",
    );
    write_text_file(&child.join("result.md"), "");
    write_text_file(&child.join("refs.jsonl"), "");
    write_text_file(
        &root.join("agent/worker.d/parent"),
        "agent:executor session:default run:r1\n",
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
    assert!(ensure_reference_tree(&root).is_ok());
    enable_dynamic_worker_fixture(&root);
    write_text_file(&root.join("agent/worker.d/model"), "../bad\n");

    assert!(matches!(
        schedule_handoff_agent_details(&root, "worker"),
        Err(ref error)
            if error.code == 2
                && error.message == "invalid handoff agent model for worker: ../bad"
    ));
}

#[test]
fn schedule_handoff_agent_model_defaults_missing_worker_model_to_default_worker_model() {
    let root = clean_test_dir("ctx-schedule-missing-worker-model");
    assert!(ensure_reference_tree(&root).is_ok());
    enable_dynamic_worker_fixture(&root);
    assert!(fs::remove_file(root.join("agent/worker.d/model")).is_ok());
    write_text_file(&root.join("agent/worker.d/life"), "temp\n");

    assert_eq!(
        schedule_handoff_agent_details(&root, "worker"),
        Ok((
            "openai/gpt-5.6".to_owned(),
            "temp".to_owned(),
            "agent:executor".to_owned()
        ))
    );
}

#[test]
fn schedule_handoff_agent_rejects_invalid_lifecycle() {
    let root = clean_test_dir("ctx-schedule-handoff-invalid-life");
    assert!(ensure_reference_tree(&root).is_ok());
    enable_dynamic_worker_fixture(&root);
    write_text_file(&root.join("agent/worker.d/life"), "detached\n");

    assert!(matches!(
        schedule_handoff_agent_details(&root, "worker"),
        Err(ref error)
            if error.code == 2
                && error.message == "invalid handoff agent life for worker: detached"
    ));
}

#[test]
fn schedule_handoff_agent_rejects_invalid_parent_ref() {
    let root = clean_test_dir("ctx-schedule-handoff-invalid-parent");
    assert!(ensure_reference_tree(&root).is_ok());
    enable_dynamic_worker_fixture(&root);
    write_text_file(&root.join("agent/worker.d/parent"), "session:default\n");

    assert!(matches!(
        schedule_handoff_agent_details(&root, "worker"),
        Err(ref error)
            if error.code == 2
                && error.message == "invalid agent parent for worker: session:default"
    ));
}

#[test]
fn schedule_handoff_agent_model_requires_backing_agent_object() {
    let root = clean_test_dir("ctx-schedule-missing-worker-object");

    assert!(matches!(
        schedule_handoff_agent_details(&root, "worker"),
        Err(ref error)
            if error.code == 2
                && error.message.contains("missing handoff agent object worker")
    ));
}

#[test]
fn schedule_status_uses_dash_model_for_local_nodes() {
    let root = clean_test_dir("ctx-schedule-status-local-model");
    assert!(ensure_reference_tree(&root).is_ok());
    let session = fixture_path(
        &root,
        &["home", "1000", "agent", "executor", "session", "default"],
    );
    create_complete_session_layout(&session);
    write_text_file(
        &session.join("context").join("plan.json"),
        r#"{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {"id": "plan", "kind": "dag", "agent": "executor"}
  ]
}
"#,
    );

    assert_eq!(
        assert_schedule_status_rows(&root, &["plan\tdag\texecutor\t-\t-\t-\t-\t-\t-\tready"]),
        Ok(())
    );
}

#[test]
fn schedule_parent_ref_output_names_parent_agent_and_session() {
    let root = clean_test_dir("ctx-schedule-parent-ref-output");
    let session = fixture_path(
        &root,
        &["home", "1000", "agent", "executor", "session", "default"],
    );

    assert_eq!(
        schedule_parent_ref_for_output("executor", &session),
        Ok("agent:executor session:default".to_owned())
    );
}

#[test]
fn schedule_child_context_output_paths_name_stable_abi_files() {
    let context = "home/1000/agent/executor/session/default/context";
    assert_eq!(
        schedule_context_abi_path(&format!("{context}/plan.json"), "advance"),
        Ok(context.to_owned())
    );
    assert_eq!(
        schedule_child_context_abi_paths(context, "work-123"),
        Ok(ScheduleChildContextAbiPaths {
            status: "home/1000/agent/executor/session/default/context/child/work-123/status"
                .to_owned(),
            handoff: "home/1000/agent/executor/session/default/context/child/work-123/handoff.md"
                .to_owned(),
            result: "home/1000/agent/executor/session/default/context/child/work-123/result.md"
                .to_owned(),
            refs: "home/1000/agent/executor/session/default/context/child/work-123/refs.jsonl"
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
