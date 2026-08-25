use super::*;
use cortexfs_runtime_client::interaction::InteractionOrigin;
use std::os::unix::fs::PermissionsExt;

fn executable(path: &Path) -> std::io::Result<()> {
    fs::write(path, "#!/bin/sh\n")?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

fn origin(channel: &str) -> InteractionOrigin {
    InteractionOrigin {
        transport: "channel".to_owned(),
        endpoint: Some(channel.to_owned()),
        ..InteractionOrigin::default()
    }
}

#[test]
fn channel_context_prefers_user_tools_and_exports_caps() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let uid = nix::unistd::geteuid().as_raw();
    let global = cortexfs_paths::channel_tool_path(root.path(), "discord");
    let user = cortexfs_paths::home_channel_tool_path(root.path(), &uid.to_string(), "discord");
    fs::create_dir_all(&global)?;
    fs::create_dir_all(&user)?;
    executable(&global.join("channel.reply"))?;
    executable(&user.join("channel.reply"))?;
    fs::create_dir_all(cortexfs_paths::channel_control_path(root.path(), "discord"))?;
    fs::write(
        cortexfs_paths::channel_control_file_path(root.path(), "discord", "cap"),
        "tool.channel.reply\n",
    )?;
    let base = ToolPath::new([cortexfs_paths::tool_root_path(root.path())]);
    let context =
        crate::runtime::channelenv::resolve(root.path(), uid, &base, Some(&origin("discord")))?
            .ok_or("missing channel context")?;
    assert_eq!(context.tool_path().dirs().first(), Some(&user));
    assert!(context.is_channel_tool("channel.reply"));
    assert!(context.allows_tool("channel.reply"));
    assert_eq!(context.caps(), "tool.channel.reply");
    Ok(())
}

#[test]
fn channel_context_rejects_collision_with_base_tool() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let uid = nix::unistd::geteuid().as_raw();
    let channel = cortexfs_paths::channel_tool_path(root.path(), "telegram");
    let base = cortexfs_paths::tool_root_path(root.path());
    fs::create_dir_all(&channel)?;
    fs::create_dir_all(&base)?;
    executable(&channel.join("channel.reply"))?;
    executable(&base.join("channel.reply"))?;
    let base = ToolPath::new([base]);
    let result =
        crate::runtime::channelenv::resolve(root.path(), uid, &base, Some(&origin("telegram")));
    assert!(
        matches!(result, Err(crate::runtime::types::ChannelRuntimeError::ToolCollision(name)) if name == "channel.reply")
    );
    Ok(())
}

#[test]
fn channel_environment_is_scoped_to_one_run() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let uid = nix::unistd::geteuid().as_raw();
    let gid = nix::unistd::getegid().as_raw();
    let tool_dir = cortexfs_paths::channel_tool_path(root.path(), "discord");
    fs::create_dir_all(&tool_dir)?;
    executable(&tool_dir.join("channel.reply"))?;
    fs::create_dir_all(cortexfs_paths::channel_control_path(root.path(), "discord"))?;
    fs::write(
        cortexfs_paths::channel_control_file_path(root.path(), "discord", "cap"),
        "tool.*\n",
    )?;
    let base = ToolPath::new([cortexfs_paths::tool_root_path(root.path())]);
    let origin = InteractionOrigin {
        conversation: Some("room-1".to_owned()),
        ..origin("discord")
    };
    let channel = crate::runtime::channelenv::resolve(root.path(), uid, &base, Some(&origin))?
        .ok_or("missing channel context")?;
    let env = [("CTX_PATH".to_owned(), base.to_env())];
    let identity = AgentUnixIdentity::new(uid, gid, []);
    let session_root = root.path().join("home/1000/agent/executor/session");
    fs::create_dir_all(&session_root)?;
    let runtime = AgentExecutableSocketRuntime {
        ctx_root: root.path(),
        source_root: root.path(),
        identity: &identity,
        env: &env,
        session_root: &session_root,
        default_cwd: "/workspace",
        model: None,
        network_allowed: false,
        agent_name: "executor",
        agent_executable: Path::new("/agent"),
        environment: RunEnvironment::Native,
    };
    let request = crate::runtime::socket::exec::AgentExecutableRunRequest {
        request_id: "request-1",
        run_id: "run-1",
        cancellation_id: "run-1",
        session: "default",
        cwd: None,
        input: "hello",
        event: None,
        origin: Some(&origin),
        channel: Some(&channel),
        history_messages: "",
        tool_context: "",
        debug: None,
    };
    let values =
        crate::runtime::socket::bwrap::agent_executable_socket_env(runtime, request, None, 0);
    let values = values
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        values.get("CTX_CHANNEL_ID").map(String::as_str),
        Some("discord")
    );
    assert_eq!(
        values.get("CTX_CHANNEL_SESSION").map(String::as_str),
        Some("default")
    );
    assert_eq!(
        values.get("CTX_CHANNEL_CAPS").map(String::as_str),
        Some("tool.*")
    );
    assert_eq!(
        values.get("CTX_CHANNEL_CONVERSATION").map(String::as_str),
        Some("room-1")
    );
    assert!(
        values
            .get("CTX_PATH")
            .is_some_and(|path| path.contains("channel/discord/tool"))
    );
    Ok(())
}
