use super::super::*;

fn receipts() -> (AgentLaunchReceipt, SystemAgentSocketReceipt) {
    (
        AgentLaunchReceipt {
            unit: "terminal-unit".to_owned(),
            pid: 42,
            identity: AgentUnixIdentity::new(1000, 1000, [10, 20]),
            invocation: "terminal-invocation".to_owned(),
            socket: PathBuf::from("/run/user/1000/default/terminal.sock"),
        },
        SystemAgentSocketReceipt {
            unit: "system-unit".to_owned(),
            was_active: false,
            owned_start: true,
            invocation: "system-invocation".to_owned(),
        },
    )
}

#[test]
fn missing_agent_meta_is_created_with_runtime_receipt() {
    let Ok(root) = tempfile::tempdir() else {
        return;
    };
    let control = root.path().join("agent/child.d");
    assert!(fs::create_dir_all(&control).is_ok());
    let (terminal, system) = receipts();

    assert_eq!(
        persist_agent_launch_meta(root.path(), "child", &terminal, &system),
        Ok(())
    );
    let meta_path = control.join("meta.json");
    let value = fs::read_to_string(&meta_path)
        .ok()
        .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok());
    assert_eq!(
        value
            .as_ref()
            .and_then(|value| value.pointer("/runtime_receipt/terminal/pid"))
            .and_then(serde_json::Value::as_u64),
        Some(42)
    );
    assert!(matches!(
        fs::metadata(meta_path),
        Ok(metadata) if metadata.permissions().mode() & 0o7777 == 0o644
    ));
}

#[test]
fn malformed_or_non_object_agent_meta_is_rejected() {
    let Ok(root) = tempfile::tempdir() else {
        return;
    };
    let control = root.path().join("agent/child.d");
    assert!(fs::create_dir_all(&control).is_ok());
    let meta_path = control.join("meta.json");
    let (terminal, system) = receipts();

    for content in ["{malformed\n", "[]\n"] {
        assert!(fs::write(&meta_path, content).is_ok());
        assert_eq!(
            persist_agent_launch_meta(root.path(), "child", &terminal, &system),
            Err(AgentLaunchError::CannotExecute)
        );
        assert_eq!(
            fs::read_to_string(&meta_path).ok().as_deref(),
            Some(content)
        );
    }
}
