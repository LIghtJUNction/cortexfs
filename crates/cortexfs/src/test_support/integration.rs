use crate::{CortexFs, LOCAL_USER_ID, ROOT_INODE, STATUS_TEXT};
use fuse3::FileType;
use std::ffi::OsStr;

#[test]
fn read_only_projection_exposes_proc_style_nodes() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    assert!(fs.lookup_child(ROOT_INODE, OsStr::new("status")).is_ok());
    assert!(fs.lookup_child(ROOT_INODE, OsStr::new("api")).is_err());
    assert!(fs.lookup_child(ROOT_INODE, OsStr::new("cap")).is_ok());
    assert!(fs.lookup_child(ROOT_INODE, OsStr::new("format")).is_ok());
    assert!(fs.lookup_child(ROOT_INODE, OsStr::new("provider")).is_ok());
    assert!(fs.lookup_child(ROOT_INODE, OsStr::new("model")).is_ok());
    assert!(fs.lookup_child(ROOT_INODE, OsStr::new("home")).is_ok());
    assert!(fs.lookup_child(ROOT_INODE, OsStr::new("home")).is_ok());
    assert!(fs.lookup_child(ROOT_INODE, OsStr::new("agent")).is_ok());
    assert!(fs.lookup_child(ROOT_INODE, OsStr::new("cluster")).is_ok());
    assert!(fs.lookup_child(ROOT_INODE, OsStr::new("mcp")).is_ok());
    assert!(fs.lookup_child(ROOT_INODE, OsStr::new("skill")).is_ok());
    assert!(fs.lookup_child(ROOT_INODE, OsStr::new("tool")).is_ok());
    assert!(fs.lookup_child(ROOT_INODE, OsStr::new("memory")).is_ok());
    assert!(fs.lookup_child(ROOT_INODE, OsStr::new("vector")).is_ok());
    assert!(fs.lookup_child(ROOT_INODE, OsStr::new("db")).is_ok());
    assert!(fs.lookup_child(ROOT_INODE, OsStr::new("audit")).is_ok());
    assert!(fs.lookup_child(ROOT_INODE, OsStr::new("control")).is_ok());
    assert!(fs.control_file_inode("drain").is_ok());
    assert!(fs.control_file_inode("reload").is_err());
    assert!(fs.control_file_inode("flush").is_ok());
    assert!(fs.control_file_inode("gc").is_ok());
    assert!(fs.control_file_inode("last_control").is_ok());
    assert!(fs.control_file_inode("queue_depth").is_ok());
    assert!(fs.control_file_inode("last_drained").is_ok());
    assert_eq!(
        fs.node_content(fs.audit_usage_inode()?)?,
        "events=0\nstaged=0\nqueued=0\ndrained=0\nerrors=0\ndenied=0\n"
    );
    assert!(
        fs.lookup_path(["audit", "cost"]).is_some(),
        "audit cost view must exist"
    );
    assert_eq!(
        fs.node_content(fs.audit_cost_inode()?)?,
        "usd=0.000000\nbillable_events=0\ndrained=0\ntool_calls=0\nagent_tasks=0\n"
    );
    assert!(fs.lookup_child(ROOT_INODE, OsStr::new("missing")).is_err());

    assert_eq!(
        fs.lookup_path(["status"]).and_then(crate::Node::content),
        Some(STATUS_TEXT)
    );
    Ok(())
}

#[test]
fn projection_exposes_local_api_fast_path_metadata() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    for (path, expected) in [
        (
            &["home", LOCAL_USER_ID, "api", "status"][..],
            "configured\n",
        ),
        (
            &["home", LOCAL_USER_ID, "api", "http", "status"][..],
            "daemon_required\n",
        ),
        (
            &["home", LOCAL_USER_ID, "api", "unix", "status"][..],
            "daemon_required\n",
        ),
    ] {
        assert!(
            fs.tree.path_inode(path).is_none(),
            "{} must be runtime-owned, not a static placeholder",
            path.join("/")
        );
        assert_eq!(
            fs.node_content(fs.resolve_path_inode(path)?)?,
            expected,
            "{} must expose the expected runtime status",
            path.join("/")
        );
    }
    assert_eq!(
        fs.lookup_path(["home", LOCAL_USER_ID, "api", "http", "listen"])
            .and_then(crate::Node::content),
        Some(crate::LOCAL_API_LISTEN_TEXT)
    );
    assert_eq!(
        fs.lookup_path(["home", LOCAL_USER_ID, "api", "unix", "path"])
            .and_then(crate::Node::content),
        Some(crate::LOCAL_API_SOCKET_TEXT)
    );
    assert_eq!(
        fs.lookup_path(["home", LOCAL_USER_ID, "api", "source"])
            .and_then(crate::Node::content),
        Some(crate::LOCAL_API_SOURCE_TEXT)
    );
    assert_eq!(
        fs.lookup_path(["home", LOCAL_USER_ID, "api", "transport"])
            .and_then(crate::Node::content),
        Some(crate::LOCAL_API_TRANSPORT_TEXT)
    );
    assert_eq!(
        fs.lookup_path(["home", LOCAL_USER_ID, "api", "store"])
            .and_then(crate::Node::content),
        Some(crate::LOCAL_API_STORE_TEXT)
    );
    assert_eq!(
        fs.lookup_path(["home", LOCAL_USER_ID, "api", "policy"])
            .and_then(crate::Node::content),
        Some(crate::LOCAL_API_POLICY_TEXT)
    );
    assert_eq!(
        fs.lookup_path(["home", LOCAL_USER_ID, "api", "audit"])
            .and_then(crate::Node::content),
        Some(crate::LOCAL_API_AUDIT_TEXT)
    );
    assert_eq!(
        fs.lookup_path(["home", LOCAL_USER_ID, "api", "unix", "api.sock"])
            .map(crate::Node::kind),
        Some(FileType::Socket)
    );
    let socket = fs
        .tree
        .path_inode(&["home", LOCAL_USER_ID, "api", "unix", "api.sock"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(
        fs.node_content(socket).is_err(),
        "local API socket is a realtime endpoint, not a regular file"
    );
    let endpoints = fs
        .lookup_path(["home", LOCAL_USER_ID, "api", "endpoints"])
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(endpoints.contains("GET /v1/models"));
    assert!(endpoints.contains("POST /v1/chat/completions"));
    assert!(endpoints.contains("POST /v1/generateContent"));
    let pipeline = fs
        .lookup_path(["home", LOCAL_USER_ID, "api", "pipeline"])
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(pipeline.contains("policy check"));
    assert!(pipeline.contains("secret resolve"));
    assert!(pipeline.contains("audit"));
    assert_eq!(
        fs.lookup_path(["home", LOCAL_USER_ID, "api", "http", "pipeline"])
            .and_then(crate::Node::content),
        Some("../pipeline\n")
    );
    assert_eq!(
        fs.lookup_path(["home", LOCAL_USER_ID, "api", "unix", "pipeline"])
            .and_then(crate::Node::content),
        Some("../pipeline\n")
    );
    Ok(())
}

#[test]
fn projection_exposes_formats_and_capability_indexes() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    assert_eq!(
        fs.lookup_path(["format", "google.generate_content", "name"])
            .and_then(crate::Node::content),
        Some("google.generate_content\n")
    );
    assert_eq!(
        fs.lookup_path(["format", "openai.chat", "model", "count"])
            .and_then(crate::Node::content),
        Some(crate::model_count_for_format("openai.chat").as_str())
    );
    assert_eq!(
        fs.lookup_path(["format", "openai.chat", "model", "list"])
            .and_then(crate::Node::content),
        Some(crate::model_list_for_format("openai.chat").as_str())
    );
    assert_eq!(
        fs.lookup_path(["cap", "model"])
            .and_then(crate::Node::content),
        Some(crate::global_model_list().as_str())
    );
    assert_eq!(
        fs.lookup_path(["format", "openai.chat", "provider", "count"])
            .and_then(crate::Node::content),
        Some(crate::provider_count_for_format("openai.chat").as_str())
    );
    assert_eq!(
        fs.lookup_path(["format", "openai.chat", "provider", "list"])
            .and_then(crate::Node::content),
        Some(crate::provider_list_for_format("openai.chat").as_str())
    );
    for format in [
        "openai.responses",
        "anthropic.messages",
        "google.generate_content",
    ] {
        assert_eq!(
            fs.lookup_path(["format", format, "model", "count"])
                .and_then(crate::Node::content),
            Some(crate::model_count_for_format(format).as_str())
        );
        assert_eq!(
            fs.lookup_path(["format", format, "model", "list"])
                .and_then(crate::Node::content),
            Some(crate::model_list_for_format(format).as_str())
        );
        assert_eq!(
            fs.lookup_path(["format", format, "provider", "count"])
                .and_then(crate::Node::content),
            Some(crate::provider_count_for_format(format).as_str())
        );
        assert_eq!(
            fs.lookup_path(["format", format, "provider", "list"])
                .and_then(crate::Node::content),
            Some(crate::provider_list_for_format(format).as_str())
        );
    }
    assert_eq!(
        fs.lookup_path(["cap", "provider"])
            .and_then(crate::Node::content),
        Some(crate::provider_list().as_str())
    );
    let capability_providers_inode = fs
        .tree
        .path_inode(&["cap", "provider"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert_eq!(fs.node_attr(capability_providers_inode)?.perm, 0o444);
    let chat_providers_list_inode = fs
        .tree
        .path_inode(&["format", "openai.chat", "provider", "list"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert_eq!(fs.node_attr(chat_providers_list_inode)?.perm, 0o444);
    assert!(
        fs.lookup_path(["mcp", "server"]).is_some(),
        "mcp server namespace must exist"
    );
    assert_eq!(
        fs.lookup_path(["cap", "mcp"])
            .and_then(crate::Node::content),
        Some("local-fs\n")
    );
    assert!(
        fs.lookup_path(["skill", "index", "by-trigger"]).is_some(),
        "skill trigger index must exist"
    );
    assert_eq!(
        fs.lookup_path(["cap", "skill"])
            .and_then(crate::Node::content),
        Some("cortexfs-test\n")
    );
    assert!(
        fs.lookup_path(["tool", "shell.exec"]).is_some(),
        "tool projection must exist"
    );
    assert_eq!(
        fs.lookup_path(["cap", "tool"])
            .and_then(crate::Node::content),
        Some("shell.exec\nfilesystem.read\nmcp.local-fs.read_file\n")
    );
    Ok(())
}

#[test]
fn format_schema_files_expose_protocol_request_shapes() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    assert_format_schema_requires(&fs, "openai.chat", "messages")?;
    assert_format_schema_requires(&fs, "openai.responses", "input")?;
    assert_format_schema_requires(&fs, "anthropic.messages", "messages")?;
    assert_format_schema_requires(&fs, "google.generate_content", "contents")?;

    let chat = format_schema(&fs, "openai.chat")?;
    assert!(
        schema_path(&chat, &["properties", "messages", "items", "required"])
            .and_then(serde_json::Value::as_array)
            .is_some_and(|required| required.iter().any(|field| field.as_str() == Some("role")))
    );
    assert!(
        schema_path(&chat, &["properties", "messages", "items", "required"])
            .and_then(serde_json::Value::as_array)
            .is_some_and(|required| required
                .iter()
                .any(|field| field.as_str() == Some("content")))
    );

    let google = format_schema(&fs, "google.generate_content")?;
    assert!(
        schema_path(
            &google,
            &["properties", "contents", "items", "properties", "parts"]
        )
        .is_some_and(serde_json::Value::is_object)
    );
    Ok(())
}

fn assert_format_schema_requires(
    fs: &CortexFs,
    format: &str,
    required_field: &str,
) -> fuse3::Result<()> {
    let schema = format_schema(fs, format)?;
    assert_eq!(
        schema.get("type").and_then(serde_json::Value::as_str),
        Some("object")
    );
    assert!(
        schema
            .get("required")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|required| required
                .iter()
                .any(|field| field.as_str() == Some(required_field))),
        "{format} schema must require {required_field}"
    );
    Ok(())
}

fn format_schema(fs: &CortexFs, format: &str) -> fuse3::Result<serde_json::Value> {
    let schema = fs
        .lookup_path(["format", format, "schema.json"])
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    serde_json::from_str(schema).map_err(|_error| libc::EIO.into())
}

fn schema_path<'schema>(
    schema: &'schema serde_json::Value,
    path: &[&str],
) -> Option<&'schema serde_json::Value> {
    path.iter().try_fold(schema, |value, key| value.get(key))
}
