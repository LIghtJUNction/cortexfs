#[test]
fn trajectory_from_recorded_session_validates_and_writes() {
    let root = clean_test_dir("trajectory-recorded");
    let session = root.join("default");
    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "");
    write_text_file(&session.join("events.jsonl"), "");

    let send = parse_socket_request_frame(
        r#"{"op":"send","id":"run-1","session":"default","cwd":"/work","input":"read README"}"#,
    );
    let send = ok!(send);
    assert!(record_socket_request_to_session(&session, &send).is_ok());

    write_text_file(
        &session.join("events.jsonl"),
        concat!(
            r#"{"type":"start","id":"run-1","run":"run-1"}"#,
            "\n",
            r#"{"type":"tool_call","run":"run-1","id":"call-1","name":"fs.read","arguments":{"path":"README.md"}}"#,
            "\n",
            r#"{"type":"usage","run":"run-1","input_tokens":12,"output_tokens":4}"#,
            "\n",
        ),
    );

    let tool = record_tool_execution_result_to_session(
        &session,
        "run-1",
        "call-1",
        "fs.read",
        "file contents",
    );
    assert!(tool.is_ok());
    let assistant = record_assistant_response_to_session(&session, "run-1", "README looks good");
    assert!(assistant.is_ok());

    let trajectory = trajectory_from_session_dir(&session);
    let trajectory = ok!(trajectory);
    assert_eq!(trajectory.schema_version, ATIF_SCHEMA_VERSION);
    assert_eq!(trajectory.session_id.as_deref(), Some("default"));
    assert_eq!(trajectory.agent.name, "ctx");
    assert_eq!(trajectory.agent.model_name.as_deref(), Some("debug/echo"));
    assert!(validate_trajectory(&trajectory).is_ok());

    assert!(
        trajectory
            .steps
            .iter()
            .any(|step| step.source == "user" && step.message.contains("read README"))
    );
    assert!(trajectory.steps.iter().any(|step| {
        step.source == "agent"
            && step.observation.as_ref().is_some_and(|observation| {
                observation.results.iter().any(|result| {
                    result.source_call_id.as_deref() == Some("call-1")
                        && result.content.as_deref() == Some("file contents")
                })
            })
    }));
    assert!(trajectory.steps.iter().any(|step| {
        step.source == "agent"
            && step.tool_calls.as_ref().is_some_and(|calls| {
                calls
                    .iter()
                    .any(|call| call.tool_call_id == "call-1" && call.function_name == "fs.read")
            })
    }));
    assert!(
        trajectory
            .steps
            .iter()
            .any(|step| { step.source == "agent" && step.message.contains("README looks good") })
    );
    assert_eq!(
        trajectory
            .final_metrics
            .as_ref()
            .and_then(|metrics| metrics.total_prompt_tokens),
        Some(12)
    );
    assert_eq!(
        trajectory
            .final_metrics
            .as_ref()
            .and_then(|metrics| metrics.total_completion_tokens),
        Some(4)
    );

    let out = session.join("trajectory.json");
    assert!(write_trajectory_json(&out, &trajectory).is_ok());
    let written = fs::read_to_string(&out);
    let written = ok!(written);
    assert!(written.contains("\"schema_version\": \"ATIF-v1.7\""));
    assert!(written.contains("\"session_id\": \"default\""));
    assert!(
        inspect_message_stream_jsonl(&ok!(fs::read_to_string(session.join("messages.jsonl"))))
            .is_ok()
    );
}

#[test]
fn trajectory_from_session_jsonl_is_pure_and_validates_minimal_user_turn() {
    let trajectory = trajectory_from_session_jsonl(
        r#"{"role":"user","content":"hello"}
{"role":"assistant","content":[{"type":"text","text":"hi"}]}"#,
        r#"{"type":"start","id":"r1","run":"r1"}
{"type":"message","run":"r1","role":"assistant","content":[{"type":"text","text":"hi"}]}
{"type":"done","run":"r1","status":"ok"}"#,
        Some(r#"{"client":"ctx","model":"debug/echo","scope":"private"}"#),
        Some("sess-a"),
    );
    assert_eq!(trajectory.session_id.as_deref(), Some("sess-a"));
    assert_eq!(trajectory.steps.len(), 2);
    let user = trajectory.steps.first();
    let user = ok!(user.ok_or("missing user step"));
    assert_eq!(user.source, "user");
    assert_eq!(user.message, "hello");
    let agent = trajectory.steps.get(1);
    let agent = ok!(agent.ok_or("missing agent step"));
    assert_eq!(agent.source, "agent");
    assert_eq!(agent.message, "hi");
    assert!(validate_trajectory(&trajectory).is_ok());
}

#[test]
fn validate_trajectory_rejects_bad_step_ids_and_sources() {
    let mut trajectory =
        trajectory_from_session_jsonl(r#"{"role":"user","content":"x"}"#, "", None, None);
    {
        let step = trajectory.steps.first_mut();
        let step = ok!(step.ok_or("missing step"));
        step.step_id = 9;
        step.source = "tool".to_owned();
    }
    let report = validate_trajectory(&trajectory);
    assert!(!report.is_ok());
    assert!(
        report
            .issues()
            .iter()
            .any(|issue| matches!(issue, TrajectoryIssue::InvalidStepId { .. }))
    );
    assert!(
        report
            .issues()
            .iter()
            .any(|issue| matches!(issue, TrajectoryIssue::InvalidStepSource { .. }))
    );
}

#[test]
fn trajectory_from_session_dir_requires_messages_and_events() {
    let root = clean_test_dir("trajectory-missing");
    let session = root.join("default");
    assert!(fs::create_dir_all(&session).is_ok());
    assert_eq!(
        trajectory_from_session_dir(&session),
        Err(TrajectoryMapError::MissingMessages)
    );
    write_text_file(&session.join("messages.jsonl"), "");
    assert_eq!(
        trajectory_from_session_dir(&session),
        Err(TrajectoryMapError::MissingEvents)
    );
}

#[test]
fn orphan_tool_call_without_agent_message_creates_run_step() {
    let trajectory = trajectory_from_session_jsonl(
        "",
        r#"{"type":"tool_call","run":"run-a","id":"call-a","name":"fs.read","arguments":{}}"#,
        None,
        None,
    );

    assert_eq!(trajectory.steps.len(), 1);
    let Some(step) = trajectory.steps.first() else {
        return;
    };
    assert_eq!(step.source, "agent");
    assert_eq!(
        step.extra
            .as_ref()
            .and_then(|extra| extra.get("run"))
            .and_then(serde_json::Value::as_str),
        Some("run-a")
    );
    assert!(
        step.tool_calls
            .as_ref()
            .is_some_and(|calls| { calls.iter().any(|call| call.tool_call_id == "call-a") })
    );
}

#[test]
fn orphan_tool_calls_and_usage_keep_distinct_run_steps() {
    let trajectory = trajectory_from_session_jsonl(
        r#"{"role":"assistant","run":"old","content":"old answer"}"#,
        r#"{"type":"tool_call","run":"run-a","id":"call-a","name":"fs.read","arguments":{}}
{"type":"tool_call","run":"run-b","id":"call-b","name":"fs.write","arguments":{}}
{"type":"usage","run":"run-a","input_tokens":11,"output_tokens":2}
{"type":"usage","run":"run-b","input_tokens":23,"output_tokens":5}"#,
        None,
        None,
    );

    let run_step = |run: &str| {
        trajectory.steps.iter().find(|step| {
            step.extra
                .as_ref()
                .and_then(|extra| extra.get("run"))
                .and_then(serde_json::Value::as_str)
                == Some(run)
        })
    };
    let Some(old) = run_step("old") else {
        return;
    };
    assert!(old.tool_calls.is_none());
    assert!(old.metrics.is_none());
    let Some(run_a) = run_step("run-a") else {
        return;
    };
    assert_eq!(
        run_a
            .metrics
            .as_ref()
            .and_then(|metrics| metrics.prompt_tokens),
        Some(11)
    );
    let Some(run_b) = run_step("run-b") else {
        return;
    };
    assert_eq!(
        run_b
            .metrics
            .as_ref()
            .and_then(|metrics| metrics.prompt_tokens),
        Some(23)
    );
}

#[test]
fn validate_trajectory_rejects_unknown_observation_source_call() {
    let mut trajectory = trajectory_from_session_jsonl(
        r#"{"role":"assistant","run":"run-a","content":"done"}"#,
        r#"{"type":"tool_call","run":"run-a","id":"call-a","name":"fs.read","arguments":{}}"#,
        None,
        None,
    );
    let Some(step) = trajectory.steps.first_mut() else {
        return;
    };
    step.observation = Some(TrajectoryObservation {
        results: vec![TrajectoryObservationResult {
            source_call_id: Some("missing-call".to_owned()),
            content: Some("result".to_owned()),
        }],
    });

    let report = validate_trajectory(&trajectory);
    assert!(report.issues().iter().any(|issue| matches!(
        issue,
        TrajectoryIssue::UnknownObservationSourceCall { source_call_id, .. }
            if source_call_id == "missing-call"
    )));
}

#[test]
fn legacy_unmatched_tool_result_keeps_content_without_unproven_call_id() {
    let trajectory = trajectory_from_session_jsonl(
        r#"{"role":"tool","content":[{"type":"tool_result","tool_call_id":"missing-call","content":"legacy output"}]}"#,
        "",
        None,
        None,
    );

    assert!(validate_trajectory(&trajectory).is_ok());
    let result = trajectory
        .steps
        .first()
        .and_then(|step| step.observation.as_ref())
        .and_then(|observation| observation.results.first());
    let Some(result) = result else {
        return;
    };
    assert_eq!(result.content.as_deref(), Some("legacy output"));
    assert_eq!(result.source_call_id, None);
    assert_eq!(
        trajectory
            .extra
            .as_ref()
            .and_then(|extra| extra.get("legacy_unmatched_tool_results"))
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
}

#[test]
fn legacy_unmatched_empty_tool_result_is_dropped() {
    let trajectory = trajectory_from_session_jsonl(
        r#"{"role":"tool","content":[{"type":"tool_result","tool_call_id":"missing-call"}]}"#,
        "",
        None,
        None,
    );

    assert!(trajectory.steps.is_empty());
    assert!(validate_trajectory(&trajectory).is_ok());
    assert_eq!(
        trajectory
            .extra
            .as_ref()
            .and_then(|extra| extra.get("legacy_unmatched_tool_results"))
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
}

#[test]
fn partially_matched_legacy_tool_results_only_clear_unmatched_id() {
    let trajectory = trajectory_from_session_jsonl(
        r#"{"role":"assistant","run":"run-a","content":"working"}
{"role":"tool","run":"run-a","content":[{"type":"tool_result","tool_call_id":"call-a","content":"matched"},{"type":"tool_result","tool_call_id":"missing-call","content":"legacy"}]}"#,
        r#"{"type":"tool_call","run":"run-a","id":"call-a","name":"fs.read","arguments":{}}"#,
        None,
        None,
    );

    assert!(validate_trajectory(&trajectory).is_ok());
    let results = trajectory
        .steps
        .first()
        .and_then(|step| step.observation.as_ref())
        .map(|observation| observation.results.as_slice())
        .unwrap_or_default();
    assert!(results.iter().any(|result| {
        result.source_call_id.as_deref() == Some("call-a")
            && result.content.as_deref() == Some("matched")
    }));
    assert!(results.iter().any(|result| {
        result.source_call_id.is_none() && result.content.as_deref() == Some("legacy")
    }));
    assert_eq!(
        trajectory
            .extra
            .as_ref()
            .and_then(|extra| extra.get("legacy_unmatched_tool_results"))
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
}

#[test]
fn trajectory_reused_call_ids_match_by_run_without_deduplication() {
    let trajectory = trajectory_from_session_jsonl(
        r#"{"role":"tool","run":"run-a","content":[{"type":"tool_result","tool_call_id":"same","content":"first"}]}
{"role":"tool","run":"run-b","content":[{"type":"tool_result","tool_call_id":"same","content":"second"}]}"#,
        r#"{"type":"tool_call","run":"run-a","id":"same","name":"fs.read","arguments":{"path":"a"}}
{"type":"tool_call","run":"run-b","id":"same","name":"fs.read","arguments":{"path":"b"}}"#,
        None,
        None,
    );

    assert_eq!(trajectory.steps.len(), 2);
    for (run, content) in [("run-a", "first"), ("run-b", "second")] {
        let step = trajectory.steps.iter().find(|step| {
            step.extra
                .as_ref()
                .and_then(|extra| extra.get("run"))
                .and_then(serde_json::Value::as_str)
                == Some(run)
        });
        let Some(step) = step else {
            return;
        };
        assert_eq!(step.tool_calls.as_ref().map(Vec::len), Some(1));
        assert!(step.observation.as_ref().is_some_and(|observation| {
            observation
                .results
                .iter()
                .any(|result| result.content.as_deref() == Some(content))
        }));
    }
    assert!(validate_trajectory(&trajectory).is_ok());
}

#[test]
fn legacy_tool_messages_consume_reused_call_ids_in_stable_order() {
    let trajectory = trajectory_from_session_jsonl(
        r#"{"role":"tool","content":[{"type":"tool_result","tool_call_id":"same","content":"first"}]}
{"role":"tool","content":[{"type":"tool_result","tool_call_id":"same","content":"second"}]}"#,
        r#"{"type":"tool_call","run":"run-a","id":"same","name":"fs.read","arguments":{"path":"a"}}
{"type":"tool_call","run":"run-b","id":"same","name":"fs.read","arguments":{"path":"b"}}"#,
        None,
        None,
    );

    let runs = trajectory
        .steps
        .iter()
        .filter_map(|step| {
            step.extra
                .as_ref()
                .and_then(|extra| extra.get("run"))
                .and_then(serde_json::Value::as_str)
        })
        .collect::<Vec<_>>();
    assert_eq!(runs, vec!["run-a", "run-b"]);
    assert_eq!(
        trajectory
            .steps
            .iter()
            .filter_map(|step| step.tool_calls.as_ref())
            .flatten()
            .count(),
        2
    );
    assert!(validate_trajectory(&trajectory).is_ok());
}

#[test]
fn multi_run_usage_does_not_guess_legacy_agent_steps() {
    let trajectory = trajectory_from_session_jsonl(
        r#"{"role":"assistant","content":"first"}
{"role":"assistant","content":"second"}"#,
        r#"{"type":"usage","run":"run-a","input_tokens":10,"output_tokens":1}
{"type":"usage","run":"run-b","input_tokens":20,"output_tokens":2}"#,
        None,
        None,
    );

    assert!(trajectory.steps.iter().all(|step| step.metrics.is_none()));
    let Some(final_metrics) = trajectory.final_metrics.as_ref() else {
        return;
    };
    assert_eq!(final_metrics.total_prompt_tokens, Some(30));
    assert_eq!(final_metrics.total_completion_tokens, Some(3));
}

#[test]
fn validate_trajectory_rejects_cross_step_observation_reference() {
    let mut trajectory = trajectory_from_session_jsonl(
        r#"{"role":"assistant","run":"run-a","content":"first"}
{"role":"assistant","run":"run-b","content":"second"}"#,
        r#"{"type":"tool_call","run":"run-a","id":"call-a","name":"fs.read","arguments":{}}
{"type":"tool_call","run":"run-b","id":"call-b","name":"fs.read","arguments":{}}"#,
        None,
        None,
    );
    let Some(second) = trajectory.steps.get_mut(1) else {
        return;
    };
    second.observation = Some(TrajectoryObservation {
        results: vec![TrajectoryObservationResult {
            source_call_id: Some("call-a".to_owned()),
            content: Some("wrong step".to_owned()),
        }],
    });

    let report = validate_trajectory(&trajectory);
    assert!(report.issues().iter().any(|issue| matches!(
        issue,
        TrajectoryIssue::UnknownObservationSourceCall {
            step_index: 1,
            source_call_id,
            ..
        } if source_call_id == "call-a"
    )));
}

#[test]
fn trajectory_issue_display_includes_actionable_location() {
    let issue = TrajectoryIssue::UnknownObservationSourceCall {
        step_index: 2,
        result_index: 4,
        source_call_id: "call-a".to_owned(),
    };

    assert_eq!(
        issue.to_string(),
        "unknown observation source call: step=3 result=5 call_id=call-a"
    );
}

#[test]
fn trajectory_issue_display_bounds_and_escapes_session_fields() {
    let source_call_id = format!("bad\n\u{1b}[31m{}", "x".repeat(2_000));
    let issue = TrajectoryIssue::UnknownObservationSourceCall {
        step_index: 0,
        result_index: 0,
        source_call_id,
    };

    let rendered = issue.to_string();

    assert!(!rendered.contains('\n'));
    assert!(!rendered.contains('\u{1b}'));
    assert!(rendered.contains("\\n"));
    assert!(rendered.contains("\\u{1b}"));
    assert!(rendered.chars().count() <= 256, "{}", rendered.len());
}
use super::*;
