#[test]
fn loaded_native_tool_executes_directly_after_tsh_load_state()
-> Result<(), Box<dyn std::error::Error>> {
    let root = short_unique_temp_path("loaded-native-tool");
    let _ignored = fs::remove_dir_all(&root);
    let control = root.join("agent").join("coder.d");
    let tool_dir = root.join("tool");
    let bash_control = tool_dir.join("bash.d");
    fs::create_dir_all(&control)?;
    fs::create_dir_all(&bash_control)?;
    fs::create_dir_all(root.join("home").join("1000").join("agent").join("coder"))?;
    fs::write(control.join("owner"), "1000\n")?;
    fs::write(control.join("uid"), "1000\n")?;
    fs::write(control.join("gid"), "1000\n")?;
    fs::write(control.join("groups"), "1000\n")?;
    fs::write(control.join("label"), "user_u:agent_r:coder_t:s0\n")?;
    fs::write(control.join("iso"), "shared\n")?;
    fs::write(control.join("parent"), "\n")?;
    fs::write(control.join("life"), "owned\n")?;
    fs::write(control.join("root"), "/ctx/home/1000/agent/coder/root\n")?;
    fs::write(control.join("cwd"), "/workspace\n")?;
    fs::write(control.join("env"), "\n")?;
    fs::write(control.join("model"), "main\n")?;
    fs::write(control.join("status"), "idle\n")?;
    fs::write(control.join("pid"), "\n")?;
    fs::write(control.join("log"), "\n")?;
    fs::write(control.join("meta.json"), "{}\n")?;
    fs::write(control.join("path"), format!("{}\n", tool_dir.display()))?;
    fs::write(
        control.join("mount"),
        format!(
            "{}\t{}\tro\trbind,nosuid,nodev\n",
            root.display(),
            root.display()
        ),
    )?;
    fs::write(
        control.join("policy"),
        "allow coder_t model:main use\nallow coder_t tool:bash execute\n",
    )?;
    fs::write(
        bash_control.join("policy"),
        "allow coder_t tool:bash execute\n",
    )?;
    write_executable_script(&tool_dir.join("bash"), "#!/bin/sh\nprintf loaded-direct\n")?;

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

    let before_load = execute_agent_tool_call(&config, &call);
    assert!(matches!(before_load, Err(ref error) if error.contains("load it through tsh first")));

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
    cortexfs::write_tsh_context_state(&cortexfs::tsh_context_state_path(view.home()), &state)?;

    let after_load = execute_agent_tool_call(&config, &call)?;
    assert_eq!(after_load, "loaded-direct");

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}
use super::runtime::test_agent_run_config;
use super::*;
