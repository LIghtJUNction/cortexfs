use super::*;

use cortexfs_runtime_client::interaction::InteractionOrigin;

#[test]
fn channel_name_is_stable_and_scope_visible() {
    let origin = InteractionOrigin {
        transport: "discord".to_owned(),
        endpoint: Some("primary/room".to_owned()),
        ..InteractionOrigin::default()
    };
    assert_eq!(
        crate::runtime::channel::canonical_channel_name(
            &origin,
            "executor",
            "default",
            SocketSessionScope::Private,
        ),
        "primary_room_executor_default"
    );
    assert_eq!(
        crate::runtime::channel::canonical_channel_name(
            &origin,
            "executor",
            "default",
            SocketSessionScope::Shared,
        ),
        "shared_primary_room_executor_default"
    );
}

#[test]
fn channel_registration_is_a_plain_filesystem_record() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let session_root = root.path().join("home/1000/agent/executor/session");
    fs::create_dir_all(&session_root)?;
    let origin = InteractionOrigin {
        transport: "terminal".to_owned(),
        ..InteractionOrigin::default()
    };
    let path = crate::runtime::channel::register_channel(
        &session_root,
        "default",
        SocketSessionScope::Private,
        &origin,
    )?;
    assert_eq!(
        path.strip_prefix(root.path())?,
        Path::new("home/1000/agent/executor/session/index/channel/terminal_executor_default")
    );
    let record: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
    assert_eq!(record.get("version"), Some(&serde_json::json!(1)));
    assert_eq!(
        record.get("name"),
        Some(&serde_json::json!("terminal_executor_default"))
    );
    assert_eq!(record.get("scope"), Some(&serde_json::json!("private")));
    assert!(fs::symlink_metadata(&path)?.is_file());
    Ok(())
}
