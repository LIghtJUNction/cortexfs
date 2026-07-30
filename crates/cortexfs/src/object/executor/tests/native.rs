#[test]
fn only_declared_native_tool_executes_without_bypassing_policy()
-> Result<(), Box<dyn std::error::Error>> {
    let (root, bash_control) = agent_tool_fixture("loaded-native-tool", "bash")?;
    let control = root.join("agent/coder.d");
    let tool_dir = root.join("tool");
    fs::create_dir_all(root.join("home").join("1000").join("agent").join("coder"))?;
    fs::write(
        bash_control.join("policy"),
        "allow coder_t tool:bash execute\n",
    )?;
    write_sdk_tool(&tool_dir.join("bash"), "bash", "loaded-direct")?;

    let config = AgentModelRunConfig {
        ctx_root: root.clone(),
        source: root.clone(),
        ..test_agent_run_config()
    };
    let call = AgentToolCall {
        id: "call-1".to_owned(),
        name: "bash".to_owned(),
        args: Vec::new(),
    };

    let before_declaration = execute_prepared_agent_tool_call(&config, &call);
    assert!(
        matches!(before_declaration, Err(ref error) if error.message().contains("declare it in the agent tools control"))
    );

    let view = cortexfs::derive_agent_runtime_view(&root, "coder")
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let mut state = cortexfs::TshContextState::default();
    state.tools = vec![cortexfs::TshLoadedToolState {
        name: "bash".to_owned(),
        path: tool_dir.join("bash"),
        description: String::new(),
        schema: None,
        dynamic_resident: true,
        pinned: false,
        last_used: 1,
    }];
    let session_state = cortexfs::tsh_context_state_path(&view.home().join("session/default"));
    cortexfs::write_tsh_context_state(&session_state, &state)?;

    let after_session_load = execute_prepared_agent_tool_call(&config, &call);
    assert!(
        matches!(after_session_load, Err(ref error) if error.message().contains("declare it in the agent tools control"))
    );

    fs::write(control.join("tools"), "bash\n")?;
    let after_declaration = execute_prepared_agent_tool_call(&config, &call)?;
    assert_eq!(after_declaration, "loaded-direct");

    cortexfs::write_tsh_context_state(&session_state, &cortexfs::TshContextState::default())?;
    let after_tsh_unload = execute_prepared_agent_tool_call(&config, &call)?;
    assert_eq!(after_tsh_unload, "loaded-direct");

    assert_sdk_output_contract(&tool_dir.join("bash"), &config, &call)?;
    write_sdk_tool(&tool_dir.join("bash"), "bash", "loaded-direct")?;

    fs::write(control.join("policy"), "allow coder_t model:main use\n")?;
    let declared_but_denied = execute_prepared_agent_tool_call(&config, &call);
    assert_eq!(
        declared_but_denied,
        Err(ExecError::new("cannot execute tool:bash: EACCES"))
    );

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

fn assert_sdk_output_contract(
    tool: &Path,
    config: &AgentModelRunConfig,
    call: &AgentToolCall,
) -> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        (
            concat!(
                "#!/bin/sh\n",
                "printf '%s\\n' '{\"type\":\"start\",\"run\":\"r1\",\"tool\":\"bash\"}' ",
                "'{\"type\":\"error\",\"run\":\"r1\",\"code\":\"EINVAL\",\"message\":\"bad input\"}' ",
                "'{\"type\":\"done\",\"run\":\"r1\",\"status\":\"error\"}'\n",
            ),
            "EINVAL: bad input",
        ),
        (
            "#!/bin/sh\nprintf '%s\\n' '{\"type\":\"start\",\"run\":\"r1\",\"tool\":\"bash\"}' 'not-json'\n",
            "invalid CortexFS Tool SDK JSONL",
        ),
        (
            "#!/bin/sh\nprintf '%s\\n' '{\"type\":\"start\",\"run\":\"r1\",\"tool\":\"bash\"}' '{\"type\":\"done\",\"run\":\"r2\",\"status\":\"ok\"}'\n",
            "run mismatch",
        ),
        (
            "#!/bin/sh\nprintf '%s\\n' '{\"type\":\"start\",\"run\":\"r1\",\"tool\":\"bash\"}'\n",
            "terminal status",
        ),
    ];
    for (script, expected) in cases {
        write_executable_script(tool, script)?;
        let result = execute_prepared_agent_tool_call(config, call);
        assert!(matches!(result, Err(ref error) if error.message().contains(expected)));
    }
    write_executable_script(
        tool,
        concat!(
            "#!/bin/sh\n",
            "printf '%s\\n' '{\"type\":\"start\",\"run\":\"r1\",\"tool\":\"bash\"}' ",
            "'{\"type\":\"message\",\"run\":\"r1\",\"role\":\"tool\",\"content\":[{\"type\":\"text\",\"text\":\"remote detail\"}]}' ",
            "'{\"type\":\"error\",\"run\":\"r1\",\"code\":\"EIO\",\"message\":\"remote MCP tool returned an error\"}' ",
            "'{\"type\":\"done\",\"run\":\"r1\",\"status\":\"error\"}'\n",
        ),
    )?;
    assert_eq!(
        execute_prepared_agent_tool_call(config, call),
        Err(ExecError::new(
            "remote detail\nEIO: remote MCP tool returned an error"
        ))
    );
    Ok(())
}
use super::runtime::test_agent_run_config;
use super::*;
