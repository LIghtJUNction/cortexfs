#[test]
fn agent_tool_loop_supports_multiple_distinct_tsh_calls() {
    let mut config = test_agent_run_config();
    let mut output = Vec::new();
    let mut step = 0_u8;
    let mut executed = Vec::new();

    let result = run_agent_tool_loop(
        &mut config,
        "test tools",
        &mut output,
        |config, _input, _stdout| {
            step = step.saturating_add(1);
            match step {
                1 => {
                    assert!(config.tool_context.is_empty());
                    Ok(AgentModelRunOutcome {
                        frames: vec![
                            r#"{"type":"start","run":"r1","model":"main"}"#.to_owned(),
                            r#"{"type":"tool_call","run":"r1","id":"call-1","name":"tsh","arguments":{"args":["tools"]}}"#.to_owned(),
                            r#"{"type":"done","run":"r1"}"#.to_owned(),
                        ],
                        success: true,
                        streamed: false,
                    })
                }
                2 => {
                    assert!(config.tool_context.contains("Tool result call-1 from tsh"));
                    assert!(config.tool_context.contains(r#"args ["tools"]"#));
                    assert!(config.tool_context.contains("fs.read"));
                    Ok(AgentModelRunOutcome {
                        frames: vec![
                            r#"{"type":"tool_call","run":"r1","id":"call-2","name":"tsh","arguments":{"args":["load","fs.read"]}}"#.to_owned(),
                        ],
                        success: true,
                        streamed: false,
                    })
                }
                3 => {
                    assert!(config.tool_context.contains("Tool result call-2 from tsh"));
                    assert!(config.tool_context.contains("loaded fs.read"));
                    Ok(AgentModelRunOutcome {
                        frames: vec![r#"{"type":"delta","run":"r1","text":"ready"}"#.to_owned()],
                        success: true,
                        streamed: false,
                    })
                }
                _ => Err(format!("unexpected model iteration {step}")),
            }
        },
        |_config, tool_call| {
            let args = tool_call
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            executed.push(args.clone());
            if args.first().is_some_and(|arg| arg == "tools") && args.len() == 1 {
                return Ok("fs.read\ntsh\n".to_owned());
            }
            if args.first().is_some_and(|arg| arg == "load")
                && args.get(1).is_some_and(|arg| arg == "fs.read")
                && args.len() == 2
            {
                return Ok("loaded fs.read\t/ctx/tool/fs.read\tmetadata\n".to_owned());
            }
            Ok(format!("unexpected args: {args:?}"))
        },
    );

    assert_eq!(result, Ok(()));
    assert_eq!(
        executed,
        vec![
            vec!["tools".to_owned()],
            vec!["load".to_owned(), "fs.read".to_owned()]
        ]
    );
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""id":"call-1""#));
    assert!(output.contains(r#""tool_call_id":"call-1""#));
    assert!(output.contains(r#""arguments":{"args":["tools"]}"#));
    assert!(output.contains(r#""id":"call-2""#));
    assert!(output.contains(r#""tool_call_id":"call-2""#));
    assert!(
        output.contains(r#""text":"ready""#),
        "missing ready frame in output:\n{output}\ncontext:\n{}",
        config.tool_context
    );
}

#[test]
fn agent_tool_loop_does_not_execute_initial_tool_discovery_request() {
    let mut config = test_agent_run_config();
    let mut output = Vec::new();
    let mut step = 0_u8;
    let mut executed = Vec::new();

    let result = run_agent_tool_loop(
        &mut config,
        "请立刻调用 tsh tools 探索你有哪些工具",
        &mut output,
        |config, _input, _stdout| {
            step = step.saturating_add(1);
            assert_eq!(step, 1);
            assert!(!config.tool_context.contains("Tool result"));
            Ok(AgentModelRunOutcome {
                frames: vec![r#"{"type":"delta","run":"r1","text":"model decides"}"#.to_owned()],
                success: true,
                streamed: false,
            })
        },
        |_config, tool_call| {
            executed.push(
                tool_call
                    .args
                    .iter()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect::<Vec<_>>(),
            );
            Ok("fs.read\nfs.write\ntsh\n".to_owned())
        },
    );

    assert_eq!(result, Ok(()));
    assert!(executed.is_empty());
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(!output.contains(r#""tool_call_id":"#));
    assert!(output.contains(r#""text":"model decides""#));
}

#[test]
fn agent_tool_loop_does_not_execute_backtick_tsh_from_user_prompt() {
    let mut config = test_agent_run_config();
    let mut output = Vec::new();
    let mut executed = Vec::new();

    let result = run_agent_tool_loop(
        &mut config,
        "执行 `tsh fs.read /workspace/smoke.txt` 然后 `tsh fs.write /workspace/smoke.txt status=done`",
        &mut output,
        |config, _input, _stdout| {
            assert!(!config.tool_context.contains("Tool result"));
            Ok(AgentModelRunOutcome {
                frames: vec![r#"{"type":"delta","run":"r1","text":"done"}"#.to_owned()],
                success: true,
                streamed: false,
            })
        },
        |_config, tool_call| {
            let args = tool_call
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            executed.push(args.clone());
            Ok(format!("executed {args:?}\n"))
        },
    );

    assert_eq!(result, Ok(()));
    assert!(executed.is_empty());
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(!output.contains(r#""tool_call_id":"#));
    assert!(output.contains(r#""text":"done""#));
}

#[test]
fn agent_tool_loop_falls_back_on_followup_model_error_after_tool_result() {
    let mut config = test_agent_run_config();
    let mut output = Vec::new();
    let mut step = 0_u8;

    let result = run_agent_tool_loop(
        &mut config,
        "test tools",
        &mut output,
        |config, _input, _stdout| {
            step = step.saturating_add(1);
            match step {
                1 => {
                    assert!(!config.suppress_model_error_events);
                    Ok(AgentModelRunOutcome {
                        frames: vec![
                            r#"{"type":"tool_call","run":"r1","id":"call-1","name":"tsh","arguments":{"args":["tools"]}}"#.to_owned(),
                        ],
                        success: true,
                        streamed: false,
                    })
                }
                2 => {
                    assert!(config.suppress_model_error_events);
                    Ok(AgentModelRunOutcome {
                        frames: vec![
                            r#"{"type":"error","run":"r1","code":"EIO","message":"model failed after tool result"}"#.to_owned(),
                        ],
                        success: false,
                        streamed: false,
                    })
                }
                _ => Err(format!("unexpected model iteration {step}")),
            }
        },
        |_config, tool_call| {
            assert_eq!(tool_call.name, "tsh");
            Ok("fs.read\nshell.exec\ntsh\n".to_owned())
        },
    );

    assert_eq!(result, Ok(()));
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""tool_call_id":"call-1""#));
    assert!(output.contains("fs.read"));
    assert!(output.contains("shell.exec"));
    assert!(output.contains("工具 `tsh` 已执行"), "{output}");
    assert!(output.contains(r#""status":"ok""#), "{output}");
    assert!(!output.contains(r#""type":"error""#), "{output}");
    assert!(
        !output.contains("model failed after tool result"),
        "{output}"
    );
}

#[test]
fn agent_tool_loop_feeds_failed_verification_args_back_for_repair() {
    let mut config = test_agent_run_config();
    let mut output = Vec::new();
    let mut step = 0_u8;
    let mut executed = Vec::new();

    let result = run_agent_tool_loop(
        &mut config,
        "repair and verify",
        &mut output,
        |config, _input, _stdout| {
            step = step.saturating_add(1);
            match step {
                1 => Ok(AgentModelRunOutcome {
                    frames: vec![
                        r#"{"type":"tool_call","run":"r1","id":"verify-1","name":"tsh","arguments":{"args":["shell.exec","cargo test -p cortexfs"]}}"#.to_owned(),
                    ],
                    success: true,
                    streamed: false,
                }),
                2 => {
                    assert!(config.tool_context.contains("Tool result verify-1 from tsh"));
                    assert!(
                        config
                            .tool_context
                            .contains(r#"args ["shell.exec","cargo test -p cortexfs"]"#)
                    );
                    assert!(config.tool_context.contains("ERROR: compile failed"));
                    Ok(AgentModelRunOutcome {
                        frames: vec![
                            r#"{"type":"tool_call","run":"r1","id":"repair-1","name":"tsh","arguments":{"args":["fs.replace","/workspace/src/lib.rs","bad","good"]}}"#.to_owned(),
                        ],
                        success: true,
                        streamed: false,
                    })
                }
                3 => {
                    assert!(config.tool_context.contains("Tool result repair-1 from tsh"));
                    assert!(
                        config
                            .tool_context
                            .contains(r#"args ["fs.replace","/workspace/src/lib.rs","bad","good"]"#)
                    );
                    Ok(AgentModelRunOutcome {
                        frames: vec![r#"{"type":"delta","run":"r1","text":"fixed"}"#.to_owned()],
                        success: true,
                        streamed: false,
                    })
                }
                _ => Err(format!("unexpected model iteration {step}")),
            }
        },
        |_config, tool_call| {
            let args = tool_call
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            executed.push(args.clone());
            if args.first().is_some_and(|tool| tool == "shell.exec") && args.len() == 2 {
                return Err("compile failed\n".to_owned());
            }
            if args.first().is_some_and(|tool| tool == "fs.replace") && args.len() == 4 {
                return Ok("replaced\n".to_owned());
            }
            Err(format!("unexpected args: {args:?}"))
        },
    );

    assert_eq!(result, Ok(()));
    assert_eq!(
        executed,
        vec![
            vec!["shell.exec".to_owned(), "cargo test -p cortexfs".to_owned()],
            vec![
                "fs.replace".to_owned(),
                "/workspace/src/lib.rs".to_owned(),
                "bad".to_owned(),
                "good".to_owned()
            ],
        ]
    );
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""tool_call_id":"verify-1""#));
    assert!(output.contains(r#""arguments":{"args":["shell.exec","cargo test -p cortexfs"]}"#));
    assert!(output.contains("ERROR: compile failed"));
    assert!(output.contains(r#""tool_call_id":"repair-1""#));
    assert!(output.contains(r#""text":"fixed""#));
}

#[test]
fn agent_tool_loop_allows_verification_rerun_after_edit() {
    let mut config = test_agent_run_config();
    let mut output = Vec::new();
    let mut step = 0_u8;
    let mut executed = Vec::new();

    let result = run_agent_tool_loop(
        &mut config,
        "repair then rerun verification",
        &mut output,
        |_config, _input, _stdout| {
            step = step.saturating_add(1);
            match step {
                1 => Ok(AgentModelRunOutcome {
                    frames: vec![
                        r#"{"type":"tool_call","run":"r1","id":"verify-1","name":"tsh","arguments":{"args":["shell.exec","cargo test -p cortexfs"]}}"#.to_owned(),
                    ],
                    success: true,
                    streamed: false,
                }),
                2 => Ok(AgentModelRunOutcome {
                    frames: vec![
                        r#"{"type":"tool_call","run":"r1","id":"edit-1","name":"tsh","arguments":{"args":["fs.replace","/workspace/src/lib.rs","bad","good"]}}"#.to_owned(),
                    ],
                    success: true,
                    streamed: false,
                }),
                3 => Ok(AgentModelRunOutcome {
                    frames: vec![
                        r#"{"type":"tool_call","run":"r1","id":"verify-2","name":"tsh","arguments":{"args":["shell.exec","cargo test -p cortexfs"]}}"#.to_owned(),
                    ],
                    success: true,
                    streamed: false,
                }),
                4 => Ok(AgentModelRunOutcome {
                    frames: vec![r#"{"type":"delta","run":"r1","text":"verified"}"#.to_owned()],
                    success: true,
                    streamed: false,
                }),
                _ => Err(format!("unexpected model iteration {step}")),
            }
        },
        |_config, tool_call| {
            let args = tool_call
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            executed.push(args.clone());
            Ok(format!("executed {args:?}\n"))
        },
    );

    assert_eq!(result, Ok(()));
    assert_eq!(
        executed,
        vec![
            vec!["shell.exec".to_owned(), "cargo test -p cortexfs".to_owned()],
            vec![
                "fs.replace".to_owned(),
                "/workspace/src/lib.rs".to_owned(),
                "bad".to_owned(),
                "good".to_owned(),
            ],
            vec!["shell.exec".to_owned(), "cargo test -p cortexfs".to_owned()],
        ]
    );
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""tool_call_id":"verify-1""#));
    assert!(output.contains(r#""tool_call_id":"edit-1""#));
    assert!(output.contains(r#""tool_call_id":"verify-2""#));
    assert!(output.contains(r#""text":"verified""#));
}

#[test]
fn agent_tool_loop_falls_back_when_followup_has_no_visible_reply() {
    let mut config = test_agent_run_config();
    let mut output = Vec::new();
    let mut step = 0_u8;

    let result = run_agent_tool_loop(
        &mut config,
        "test tools",
        &mut output,
        |config, _input, _stdout| {
            step = step.saturating_add(1);
            match step {
                1 => Ok(AgentModelRunOutcome {
                    frames: vec![
                        r#"{"type":"tool_call","run":"r1","id":"call-1","name":"tsh","arguments":{"args":["tools"]}}"#.to_owned(),
                    ],
                    success: true,
                    streamed: false,
                }),
                2 => {
                    assert!(config.suppress_model_error_events);
                    Ok(AgentModelRunOutcome {
                        frames: vec![
                            r#"{"type":"usage","run":"r1","input_tokens":10,"output_tokens":0}"#.to_owned(),
                            r#"{"type":"done","run":"r1","status":"ok"}"#.to_owned(),
                        ],
                        success: true,
                        streamed: false,
                    })
                }
                _ => Err(format!("unexpected model iteration {step}")),
            }
        },
        |_config, tool_call| {
            assert_eq!(tool_call.name, "tsh");
            Ok("fs.read\nfs.write\ntsh\n".to_owned())
        },
    );

    assert_eq!(result, Ok(()));
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""tool_call_id":"call-1""#));
    assert!(output.contains("fs.read"));
    assert!(output.contains("fs.write"));
    assert!(output.contains("工具 `tsh` 已执行"));
    assert!(output.contains(r#""status":"ok""#));
}

#[test]
fn agent_tool_loop_falls_back_when_followup_model_errors_after_tool_result() {
    let mut config = test_agent_run_config();
    let mut output = Vec::new();
    let mut step = 0_u8;

    let result = run_agent_tool_loop(
        &mut config,
        "test tools",
        &mut output,
        |config, _input, _stdout| {
            step = step.saturating_add(1);
            match step {
                1 => {
                    assert!(!config.suppress_model_error_events);
                    Ok(AgentModelRunOutcome {
                        frames: vec![
                            r#"{"type":"tool_call","run":"r1","id":"call-1","name":"tsh","arguments":{"args":["tools"]}}"#.to_owned(),
                        ],
                        success: true,
                        streamed: false,
                    })
                }
                2 => {
                    assert!(config.suppress_model_error_events);
                    Ok(AgentModelRunOutcome {
                        frames: vec![
                            r#"{"type":"error","run":"r1","code":"ETIMEDOUT","message":"agent model timed out"}"#.to_owned(),
                        ],
                        success: false,
                        streamed: false,
                    })
                }
                _ => Err(format!("unexpected model iteration {step}")),
            }
        },
        |_config, tool_call| {
            assert_eq!(tool_call.name, "tsh");
            Ok("fs.read\nfs.write\ntsh\n".to_owned())
        },
    );

    assert_eq!(result, Ok(()));
    assert!(!config.suppress_model_error_events);
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""tool_call_id":"call-1""#));
    assert!(output.contains("工具 `tsh` 已执行"));
    assert!(output.contains("fs.read"));
    assert!(output.contains(r#""status":"ok""#));
    assert!(!output.contains(r#""code":"ETIMEDOUT""#), "{output}");
}

#[test]
fn agent_tool_loop_falls_back_when_followup_model_call_fails_after_tool_result() {
    let mut config = test_agent_run_config();
    let mut output = Vec::new();
    let mut step = 0_u8;

    let result = run_agent_tool_loop(
        &mut config,
        "test tools",
        &mut output,
        |config, _input, _stdout| {
            step = step.saturating_add(1);
            match step {
                1 => Ok(AgentModelRunOutcome {
                    frames: vec![
                        r#"{"type":"tool_call","run":"r1","id":"call-1","name":"tsh","arguments":{"args":["tools"]}}"#.to_owned(),
                    ],
                    success: true,
                    streamed: false,
                }),
                2 => {
                    assert!(config.suppress_model_error_events);
                    Err("model unavailable".to_owned())
                }
                _ => Err(format!("unexpected model iteration {step}")),
            }
        },
        |_config, tool_call| {
            assert_eq!(tool_call.name, "tsh");
            Ok("fs.read\nfs.write\ntsh\n".to_owned())
        },
    );

    assert_eq!(result, Ok(()));
    assert!(!config.suppress_model_error_events);
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""tool_call_id":"call-1""#));
    assert!(output.contains("工具 `tsh` 已执行"));
    assert!(output.contains("fs.write"));
    assert!(output.contains(r#""status":"ok""#));
}

#[test]
fn agent_tool_loop_wraps_followup_plain_text_as_event() {
    let mut config = test_agent_run_config();
    let mut output = Vec::new();
    let mut step = 0_u8;

    let result = run_agent_tool_loop(
        &mut config,
        "test tools",
        &mut output,
        |_config, _input, _stdout| {
            step = step.saturating_add(1);
            match step {
                1 => Ok(AgentModelRunOutcome {
                    frames: vec![
                        r#"{"type":"tool_call","run":"r1","id":"call-1","name":"tsh","arguments":{"args":["tools"]}}"#.to_owned(),
                    ],
                    success: true,
                    streamed: false,
                }),
                2 => Ok(AgentModelRunOutcome {
                    frames: vec!["工具已经列出。".to_owned()],
                    success: true,
                    streamed: false,
                }),
                _ => Err(format!("unexpected model iteration {step}")),
            }
        },
        |_config, _tool_call| Ok("fs.read\ntsh\n".to_owned()),
    );

    assert_eq!(result, Ok(()));
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""type":"delta""#), "{output}");
    assert!(output.contains("工具已经列出。"), "{output}");
    assert!(output.contains(r#""status":"ok""#), "{output}");
}

#[test]
fn agent_tool_loop_falls_back_on_repeated_identical_tsh_call() {
    let mut config = test_agent_run_config();
    let mut output = Vec::new();
    let mut step = 0_u8;
    let mut executions = 0_u8;

    let result = run_agent_tool_loop(
        &mut config,
        "repeat tools",
        &mut output,
        |_config, _input, _stdout| {
            step = step.saturating_add(1);
            assert!(step <= 2);
            Ok(AgentModelRunOutcome {
                frames: vec![
                    r#"{"type":"tool_call","run":"r1","id":"call-1","name":"tsh","arguments":{"args":["tools"]}}"#.to_owned(),
                ],
                success: true,
                streamed: false,
            })
        },
        |_config, _tool_call| {
            executions = executions.saturating_add(1);
            Ok("fs.read\n".to_owned())
        },
    );

    assert_eq!(result, Ok(()));
    assert_eq!(executions, 1);
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""status":"ok""#));
    assert!(output.contains("工具 `tsh` 已执行"));
    assert!(output.contains("fs.read"));
    assert!(!output.contains(r#""code":"ELOOP""#), "{output}");
}

#[test]
fn agent_tool_loop_passes_tool_context_to_model_process_across_iterations()
-> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-agent-model-context")?;
    let model = root.join("model-script");
    write_executable_script(
        &model,
        r#"#!/bin/sh
case "$CTX_AGENT_TOOL_CONTEXT" in
  "")
    printf '{"type":"tool_call","run":"%s","id":"call-1","name":"tsh","arguments":{"args":["tools"]}}\n' "$CTX_RUN_ID"
    ;;
  *"Tool result call-2 from tsh"*"loaded fs.read"*)
    printf '{"type":"delta","run":"%s","text":"ready"}\n' "$CTX_RUN_ID"
    printf '{"type":"done","run":"%s","status":"ok"}\n' "$CTX_RUN_ID"
    ;;
  *"Tool result call-1 from tsh"*fs.read*)
    printf '{"type":"tool_call","run":"%s","id":"call-2","name":"tsh","arguments":{"args":["load","fs.read"]}}\n' "$CTX_RUN_ID"
    ;;
  *)
    printf '{"type":"error","run":"%s","code":"EIO","message":"missing tool context"}\n' "$CTX_RUN_ID"
    exit 2
    ;;
esac
"#,
    )?;
    let mut config = AgentModelRunConfig {
        model_path: model,
        ..test_agent_run_config()
    };
    let mut output = Vec::new();
    let mut executed = Vec::new();

    let result = run_agent_tool_loop(
        &mut config,
        "test tools",
        &mut output,
        run_agent_model_once,
        |_config, tool_call| {
            let args = tool_call
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            executed.push(args.clone());
            if args.first().is_some_and(|arg| arg == "tools") && args.len() == 1 {
                return Ok("fs.read\ntsh\n".to_owned());
            }
            if args.first().is_some_and(|arg| arg == "load")
                && args.get(1).is_some_and(|arg| arg == "fs.read")
                && args.len() == 2
            {
                return Ok("loaded fs.read\t/ctx/tool/fs.read\tmetadata\n".to_owned());
            }
            Err(format!("unexpected args: {args:?}"))
        },
    );

    assert_eq!(result, Ok(()));
    assert_eq!(
        executed,
        vec![
            vec!["tools".to_owned()],
            vec!["load".to_owned(), "fs.read".to_owned()]
        ]
    );
    assert!(config.tool_context.contains("Tool result call-1 from tsh"));
    assert!(config.tool_context.contains("Tool result call-2 from tsh"));
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(output.contains(r#""id":"call-1""#));
    assert!(output.contains(r#""id":"call-2""#));
    assert!(
        output.contains(r#""text":"ready""#),
        "missing ready frame in output:\n{output}\ncontext:\n{}",
        config.tool_context
    );
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn agent_tool_context_keeps_recent_results_under_limit() {
    let mut config = test_agent_run_config();

    for index in 0..12 {
        let call = AgentToolCall {
            id: format!("call-{index}"),
            name: "tsh".to_owned(),
            args: Vec::new(),
        };
        config.push_tool_result(&call, &"x".repeat(8 * 1024));
    }

    assert!(config.tool_context.len() <= MAX_AGENT_TOOL_CONTEXT_BYTES);
    assert!(
        config
            .tool_context
            .contains("[earlier tool context truncated]")
    );
    assert!(config.tool_context.contains("Tool result call-11 from tsh"));
    assert!(config.tool_context.contains("args []"));
    assert!(!config.tool_context.contains("Tool result call-0 from tsh"));
}

#[test]
fn agent_tool_context_truncation_preserves_utf8_boundaries() {
    let mut config = test_agent_run_config();
    config.tool_context = "€".repeat((MAX_AGENT_TOOL_CONTEXT_BYTES / "€".len()) + 64);
    let call = AgentToolCall {
        id: "call-final".to_owned(),
        name: "tsh".to_owned(),
        args: Vec::new(),
    };

    config.push_tool_result(&call, "done\n");

    assert!(config.tool_context.len() <= MAX_AGENT_TOOL_CONTEXT_BYTES);
    assert!(
        config
            .tool_context
            .is_char_boundary(config.tool_context.len())
    );
    assert!(
        config
            .tool_context
            .contains("Tool result call-final from tsh")
    );
}

#[test]
fn terminal_tool_line_shows_tool_name_and_quoted_args() {
    let call = AgentToolCall {
        id: "call-1".to_owned(),
        name: "bash".to_owned(),
        args: vec![
            OsString::from("shell.exec"),
            OsString::from("bash -lc 'date'"),
        ],
    };

    assert_eq!(
        tool_terminal_running_line(&call),
        "\r\ntool bash running shell.exec 'bash -lc '\\''date'\\'''\r\n"
    );
    assert_eq!(
        tool_terminal_done_line(&call, "ok\n", true),
        "\r\ntool bash done 3 bytes\r\n"
    );
    assert_eq!(
        tool_terminal_done_line(&call, "ERROR: bad\n", false),
        "\r\ntool bash error 11 bytes\r\n"
    );
}

#[test]
fn agent_tool_loop_hands_off_when_tool_iteration_limit_is_exceeded() {
    let mut config = test_agent_run_config();
    let mut output = Vec::new();
    let mut step = 0_u8;
    let mut executions = 0_u8;

    let result = run_agent_tool_loop(
        &mut config,
        "keep iterating",
        &mut output,
        |_config, _input, _stdout| {
            step = step.saturating_add(1);
            Ok(AgentModelRunOutcome {
                frames: vec![format!(
                    r#"{{"type":"tool_call","run":"r1","id":"call-{step}","name":"tsh","arguments":{{"args":["shell.exec","printf {step}"]}}}}"#
                )],
                success: true,
                streamed: false,
            })
        },
        |_config, _tool_call| {
            executions = executions.saturating_add(1);
            Ok("ok\n".to_owned())
        },
    );

    assert_eq!(result, Ok(()));
    assert!(executions > 1);
    let output = String::from_utf8(output).unwrap_or_default();
    assert!(
        output.contains("agent tool loop limit exceeded"),
        "{output}"
    );
    assert!(output.contains("上下文干净的 agent"), "{output}");
    assert!(output.contains("默认由人工决定"), "{output}");
    assert!(output.contains(r#""status":"ok""#), "{output}");
    assert!(!output.contains(r#""code":"ELOOP""#), "{output}");
}
use super::runtime::test_agent_run_config;
use super::*;

#[test]
fn tool_context_growth_rejects_second_iteration_before_model_spawn()
-> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("tool-loop-window-spawn-count")?;
    let count = root.join("count");
    let model = root.join("model");
    write_executable_script(
        &model,
        format!(
            "#!/bin/sh\nprintf x >> '{}'\nprintf '{{\"type\":\"tool_call\",\"run\":\"%s\",\"id\":\"call-1\",\"name\":\"tsh\",\"arguments\":{{\"args\":[\"status\"]}}}}\\n' \"$CTX_RUN_ID\"\n",
            count.display()
        ),
    )?;
    let mut config = AgentModelRunConfig {
        model_path: model,
        context_budget: AgentWindowBudget::from_effective(
            ModelContextLimit::known(4_096).unwrap_or(ModelContextLimit::Unknown),
        ),
        ..test_agent_run_config()
    };
    let mut output = Vec::new();

    run_agent_tool_loop(
        &mut config,
        "hello",
        &mut output,
        run_agent_model_once,
        |_config, _call| Ok("x".repeat(16_384)),
    )?;

    assert_eq!(fs::read_to_string(&count)?, "x");
    assert!(!String::from_utf8(output)?.contains(r#""code":"E2BIG""#));
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}
