fn hosted_network_command(network_allowed: bool) -> Option<AgentLaunchCommand> {
    let root = clean_test_dir(if network_allowed {
        "ctx-agent-hosted-network-allow"
    } else {
        "ctx-agent-hosted-network-deny"
    });
    ensure_reference_tree(&root).ok()?;
    ensure_runtime_model_fixture(&root);
    if !network_allowed {
        let path = root.join("agent/executor.d/policy");
        let policy = fs::read_to_string(&path).ok()?;
        let denied = policy
            .lines()
            .filter(|line| !line.contains("network:default connect"))
            .collect::<Vec<_>>()
            .join("\n");
        write_text_file(&path, &format!("{denied}\n"));
    }
    let view = derive_agent_runtime_view(&root, "executor").ok()?;
    let args = AgentStartArgs {
        name: "executor".to_owned(),
        session: "network".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: false,
        mounts: Vec::new(),
        command: vec!["/bin/true".to_owned()],
    };
    Some(agent_start_systemd_command(
        &root,
        &args,
        &[],
        &view,
        Path::new(cortexfs::runtime::terminal::broker::BROKER_SOCKET),
        "cortexfs-agent-executor-network-terminal",
    ))
}

#[test]
fn hosted_network_grant_uses_rootless_pasta_boundary() {
    let Some(command) = hosted_network_command(true) else {
        return;
    };
    let expected = [
        cortexfs::support::command::PASTA,
        "-f",
        "-q",
        "--config-net",
        "--no-map-gw",
        "--no-icmp",
        "-t",
        "none",
        "-u",
        "none",
        "-T",
        "53",
        "-U",
        "53",
        "--",
        cortexfs::support::command::BWRAP,
    ];
    assert!(command.args.windows(expected.len()).any(|window| {
        window
            .iter()
            .map(String::as_str)
            .eq(expected.iter().copied())
    }));
    let bwrap = command
        .args
        .iter()
        .position(|arg| arg == cortexfs::support::command::BWRAP)
        .unwrap_or_default();
    assert!(!command.args[bwrap..].iter().any(|arg| arg == "--unshare-net"));
}

#[test]
fn hosted_network_denial_keeps_private_network_namespace() {
    let Some(command) = hosted_network_command(false) else {
        return;
    };
    assert!(
        !command
            .args
            .iter()
            .any(|arg| arg == cortexfs::support::command::PASTA)
    );
    let bwrap = command
        .args
        .iter()
        .position(|arg| arg == cortexfs::support::command::BWRAP)
        .unwrap_or_default();
    assert!(command.args[bwrap..].iter().any(|arg| arg == "--unshare-net"));
}
