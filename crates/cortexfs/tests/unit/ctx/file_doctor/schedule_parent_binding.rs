#[test]
fn schedule_claim_rejects_wrong_backing_parent_without_claiming() {
    let root = clean_test_dir("ctx-schedule-claim-wrong-backing-parent");
    let child = create_pending_worker_handoff(&root, "claim wrong backing parent");
    write_text_file(&root.join("agent/worker.d/parent"), "agent:planner session:default\n");

    assert!(matches!(
        schedule_claim_worker(&root),
        Err(ref error)
            if error.code == 2
                && error.message
                    == "handoff agent parent mismatch for worker: agent:planner session:default"
    ));
    assert!(matches!(
        fs::read_to_string(child.join("status")).as_deref(),
        Ok("pending\n")
    ));
}

#[test]
fn schedule_result_rejects_wrong_backing_session_without_recording() {
    let root = clean_test_dir("ctx-schedule-result-wrong-backing-session");
    let child = create_pending_worker_handoff(&root, "result wrong backing session");
    write_text_file(&root.join("agent/worker.d/parent"), "agent:coder session:feature\n");

    assert!(matches!(
        schedule_command(&root, &ScheduleArgs::Result {
            path: "home/1000/agent/coder/session/default/context/plan.json".to_owned(),
            child: "work-123".to_owned(),
            status: ChildContextStatus::Done,
            result: "done\n".to_owned(),
            refs_jsonl: String::new(),
        }),
        Err(ref error)
            if error.code == 2
                && error.message
                    == "handoff agent parent mismatch for worker: agent:coder session:feature"
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
fn schedule_parent_binding_requires_matching_run_when_parent_run_is_known() {
    assert!(matches!(
        schedule_require_handoff_parent(
            "agent:coder session:default run:r1",
            "worker",
            "agent:coder session:default"
        ),
        Err(ref error)
            if error.code == 2
                && error.message
                    == "handoff agent parent mismatch for worker: agent:coder session:default"
    ));
    assert_eq!(
        schedule_require_handoff_parent(
            "agent:coder session:default run:r1",
            "worker",
            "agent:coder session:default run:r1"
        ),
        Ok(())
    );
}
