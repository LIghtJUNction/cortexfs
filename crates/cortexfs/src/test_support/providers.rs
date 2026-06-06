use crate::CortexFs;

use super::support::{
    assert_format_route, assert_model_metadata, assert_openai_chat_route, provider_model_text,
    user_models_file_content,
};

#[test]
fn provider_runtime_files_follow_specs() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let primary = crate::PROVIDER_SPECS
        .first()
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let primary_url = fs.provider_child_dir_inode(primary.id, "url")?;
    let primary_enabled = fs.provider_child_dir_inode(primary.id, "enabled")?;
    let primary_health = fs.provider_child_dir_inode(primary.id, "health")?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert_eq!(
        runtime
            .lookup_child(primary_url, "effective")
            .and_then(crate::Node::content),
        Some(primary.default_base_url)
    );
    assert_eq!(
        runtime
            .lookup_child(primary_enabled, "effective")
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        runtime
            .lookup_child(primary_health, "status")
            .and_then(crate::Node::content),
        Some("unknown\n")
    );
    assert_eq!(
        runtime
            .lookup_child(primary_health, "latency_ms")
            .and_then(crate::Node::content),
        Some("\n")
    );
    assert_eq!(
        runtime
            .lookup_child(primary_health, "last_error")
            .and_then(crate::Node::content),
        Some("\n")
    );
    drop(runtime);
    Ok(())
}

#[test]
fn provider_model_views_follow_specs() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let primary = crate::PROVIDER_SPECS
        .first()
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let compatible = crate::PROVIDER_SPECS
        .get(1)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let compatible_url = fs.provider_child_dir_inode(compatible.id, "url")?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert_eq!(
        runtime
            .lookup_child(compatible_url, "effective")
            .and_then(crate::Node::content),
        Some(compatible.default_base_url)
    );
    drop(runtime);
    assert_eq!(
        fs.lookup_path(["providers", primary.id, "context"])
            .and_then(crate::Node::content),
        Some("local:provider_r:provider_t:s0\n")
    );
    assert_eq!(
        fs.lookup_path(["providers", "count"])
            .and_then(crate::Node::content),
        Some(crate::provider_count().as_str())
    );
    assert_eq!(
        fs.lookup_path(["provider", primary.id, "model", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["provider", primary.id, "model", "list"])
            .and_then(crate::Node::content),
        Some(format!("{}\n", primary.default_model).as_str())
    );
    assert_eq!(
        fs.lookup_path(["providers", primary.id, "models", "count"])
            .and_then(crate::Node::content),
        Some("1\n"),
        "provider models compatibility path must expose the same index content"
    );
    assert_eq!(
        fs.lookup_path(["providers", compatible.id, "family"])
            .and_then(crate::Node::content),
        Some(compatible.family)
    );
    assert_eq!(
        fs.lookup_path(["provider", compatible.id, "model", "list"])
            .and_then(crate::Node::content),
        Some(format!("{}\n", compatible.default_model).as_str())
    );
    assert_eq!(
        fs.lookup_path([
            "provider",
            compatible.id,
            "model",
            compatible.default_model,
            "format"
        ])
        .and_then(crate::Node::content),
        Some(format!("{}\n", crate::default_format(compatible)).as_str())
    );
    assert_eq!(
        fs.lookup_path([
            "provider",
            primary.id,
            "model",
            primary.default_model,
            "format"
        ])
        .and_then(crate::Node::content),
        Some(format!("{}\n", crate::default_format(primary)).as_str())
    );
    assert_model_metadata(
        &fs,
        &[
            "provider".to_owned(),
            primary.id.to_owned(),
            "model".to_owned(),
            primary.default_model.to_owned(),
        ],
        primary,
    );
    assert_model_metadata(
        &fs,
        &[
            "provider".to_owned(),
            compatible.id.to_owned(),
            "model".to_owned(),
            compatible.default_model.to_owned(),
        ],
        compatible,
    );
    Ok(())
}

#[test]
fn global_model_index_follows_specs() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let primary = crate::PROVIDER_SPECS
        .first()
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let compatible = crate::PROVIDER_SPECS
        .get(1)
        .ok_or_else(fuse3::Errno::new_not_exist)?;

    assert_eq!(
        fs.lookup_path_owned(&[
            "model".to_owned(),
            crate::provider_model_id(primary),
            "model".to_owned()
        ])
        .and_then(crate::Node::content),
        Some(format!("{}\n", primary.default_model).as_str())
    );
    assert_eq!(
        fs.lookup_path_owned(&[
            "model".to_owned(),
            crate::provider_model_id(compatible),
            "provider".to_owned()
        ])
        .and_then(crate::Node::content),
        Some(format!("{}\n", compatible.id).as_str())
    );
    assert_eq!(
        fs.lookup_path_owned(&[
            "model".to_owned(),
            crate::provider_model_id(compatible),
            "model".to_owned()
        ])
        .and_then(crate::Node::content),
        Some(format!("{}\n", compatible.default_model).as_str())
    );
    assert_eq!(
        fs.lookup_path(["model", "count"])
            .and_then(crate::Node::content),
        Some(crate::global_model_count().as_str())
    );
    assert_eq!(
        fs.lookup_path(["model", "list"])
            .and_then(crate::Node::content),
        Some(crate::global_model_list().as_str())
    );
    assert_eq!(
        fs.lookup_path(["models", "list"])
            .and_then(crate::Node::content),
        Some(crate::global_model_list().as_str()),
        "global models compatibility path must expose the same index content"
    );
    assert_model_metadata(
        &fs,
        &["model".to_owned(), crate::provider_model_id(primary)],
        primary,
    );
    assert_model_metadata(
        &fs,
        &["model".to_owned(), crate::provider_model_id(compatible)],
        compatible,
    );
    let model_list_inode = fs
        .tree
        .path_inode(&["model", "list"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert_eq!(fs.node_attr(model_list_inode)?.perm, 0o444);
    Ok(())
}

#[test]
fn projection_exposes_provider_supported_formats_from_specs() {
    let fs = CortexFs::new();

    for provider in crate::PROVIDER_SPECS {
        let expected = crate::newline_list(provider.formats.iter());
        assert_eq!(
            fs.lookup_path(["providers", provider.id, "formats"])
                .and_then(crate::Node::content),
            Some(expected.as_str())
        );
    }
}

#[test]
fn projection_exposes_provider_root_index() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    assert_eq!(
        fs.lookup_path(["providers", "count"])
            .and_then(crate::Node::content),
        Some(crate::provider_count().as_str())
    );
    assert_eq!(
        fs.lookup_path(["providers", "list"])
            .and_then(crate::Node::content),
        Some(crate::provider_list().as_str())
    );
    let providers_list_inode = fs
        .tree
        .path_inode(&["providers", "list"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert_eq!(fs.node_attr(providers_list_inode)?.perm, 0o444);
    Ok(())
}

#[test]
fn provider_models_refresh_updates_control_and_audit() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    for provider in crate::PROVIDER_SPECS {
        let refresh = fs.provider_models_file_inode(provider.id, "refresh")?;
        {
            let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
            assert_eq!(runtime.write(refresh, 0, b"1\n")?, 2);
            drop(runtime);
        }

        assert_eq!(fs.node_attr(refresh)?.perm, 0o222);
        assert_eq!(
            fs.node_content(refresh),
            Err(fuse3::Errno::from(libc::EACCES))
        );
        assert_eq!(
            fs.node_content(fs.control_file_inode("last_control")?)?,
            format!("provider/{}/model/refresh\n", provider.id)
        );
        let audit = fs.node_content(fs.audit_events_inode()?)?;
        assert!(audit.contains(&format!("\"format\":\"provider.{}.model\"", provider.id)));
        assert!(audit.contains("\"name\":\"refresh\""));
        assert!(audit.contains("\"event\":\"refreshed\""));
    }
    Ok(())
}

#[test]
fn provider_health_check_updates_status_control_and_audit() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    for provider in crate::PROVIDER_SPECS {
        let check = fs.provider_health_file_inode(provider.id, "check")?;
        {
            let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
            assert_eq!(runtime.write(check, 0, b"1\n")?, 2);
            drop(runtime);
        }

        assert_eq!(fs.node_attr(check)?.perm, 0o222);
        assert_eq!(
            fs.node_content(check),
            Err(fuse3::Errno::from(libc::EACCES))
        );
        assert_eq!(
            fs.node_content(fs.control_file_inode("last_control")?)?,
            format!("providers/{}/health/check\n", provider.id)
        );
        let status = fs.provider_health_file_inode(provider.id, "status")?;
        assert_eq!(fs.node_content(status)?, "queued\n");
        let latency = fs.provider_health_file_inode(provider.id, "latency_ms")?;
        let last_error = fs.provider_health_file_inode(provider.id, "last_error")?;
        assert_eq!(fs.node_content(latency)?, "\n");
        assert_eq!(fs.node_content(last_error)?, "daemon pending\n");
        let audit = fs.node_content(fs.audit_events_inode()?)?;
        assert!(audit.contains(&format!("\"format\":\"provider.{}.health\"", provider.id)));
        assert!(audit.contains("\"name\":\"check\""));
        assert!(audit.contains("\"event\":\"queued\""));
    }
    Ok(())
}

#[test]
fn provider_health_check_reports_disabled_provider_without_network() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let provider = crate::PROVIDER_SPECS
        .first()
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let enabled_dir = fs.provider_child_dir_inode(provider.id, "enabled")?;
    let check = fs.provider_health_file_inode(provider.id, "check")?;
    let status = fs.provider_health_file_inode(provider.id, "status")?;
    let latency = fs.provider_health_file_inode(provider.id, "latency_ms")?;
    let last_error = fs.provider_health_file_inode(provider.id, "last_error")?;

    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let current = runtime
            .lookup_child(enabled_dir, "current")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        runtime.write(current, 0, b"0\n")?;
        runtime.write(check, 0, b"1\n")?;
        drop(runtime);
    }

    assert_eq!(fs.node_content(status)?, "disabled\n");
    assert_eq!(fs.node_content(latency)?, "\n");
    assert_eq!(fs.node_content(last_error)?, "provider disabled\n");
    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains(&format!("\"format\":\"provider.{}.enabled\"", provider.id)));
    assert!(audit.contains(&format!("\"format\":\"provider.{}.health\"", provider.id)));
    Ok(())
}

#[test]
fn provider_secret_rotate_updates_only_secret_status_view() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    for provider in crate::PROVIDER_SPECS {
        let rotate = fs.provider_secrets_file_inode(provider.id, "rotate")?;
        {
            let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
            assert_eq!(runtime.write(rotate, 0, b"1\n")?, 2);
            drop(runtime);
        }

        assert_eq!(fs.node_attr(rotate)?.perm, 0o222);
        assert_eq!(
            fs.node_content(rotate),
            Err(fuse3::Errno::from(libc::EACCES))
        );
        assert_eq!(
            fs.node_content(fs.control_file_inode("last_control")?)?,
            format!("providers/{}/secrets/rotate\n", provider.id)
        );
        let last_rotated = fs.provider_secrets_file_inode(provider.id, "last_rotated")?;
        let next_rotation = fs.provider_secrets_file_inode(provider.id, "next_rotation")?;
        assert_eq!(fs.node_content(last_rotated)?, "pending\n");
        assert_eq!(fs.node_content(next_rotation)?, "\n");
        let audit = fs.node_content(fs.audit_events_inode()?)?;
        assert!(audit.contains(&format!("\"format\":\"provider.{}.secrets\"", provider.id)));
        assert!(audit.contains("\"name\":\"rotate\""));
        assert!(audit.contains("\"event\":\"requested\""));
        assert!(!audit.contains("api_key"));
        assert!(!audit.contains("token"));
        assert!(!audit.contains("secret-password"));
    }
    Ok(())
}

#[test]
fn user_models_refresh_recomputes_space_access_routes_and_audit() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let primary = crate::PROVIDER_SPECS
        .first()
        .ok_or_else(fuse3::Errno::new_not_exist)?;

    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let enabled_dir = fs.provider_child_dir_inode(primary.id, "enabled")?;
        let enabled = runtime
            .lookup_child(enabled_dir, "current")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        runtime.write(enabled, 0, b"0\n")?;
        drop(runtime);
    }

    let refresh = fs.user_models_file_inode("refresh")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        assert_eq!(runtime.write(refresh, 0, b"1\n")?, 2);
        drop(runtime);
    }

    assert_eq!(fs.node_attr(refresh)?.perm, 0o222);
    assert_eq!(
        fs.node_content(refresh),
        Err(fuse3::Errno::from(libc::EACCES))
    );
    assert_eq!(
        fs.node_content(fs.control_file_inode("last_control")?)?,
        crate::LOCAL_USER_MODELS_REFRESH_DISPLAY_TEXT
    );
    let user_model = fs.user_model_dir_inode(primary)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert_eq!(
        runtime
            .lookup_child(user_model, "allowed")
            .and_then(crate::Node::content),
        Some("0\n")
    );
    assert_eq!(
        runtime
            .lookup_child(user_model, "reason")
            .and_then(crate::Node::content),
        Some("provider_disabled\n")
    );
    drop(runtime);
    assert_eq!(
        user_models_file_content(&fs, "count")?,
        format!("{}\n", crate::PROVIDER_SPECS.len().saturating_sub(1))
    );
    assert!(!user_models_file_content(&fs, "list")?.contains(&crate::provider_model_id(primary)));
    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains("\"format\":\"space.users.1000.model\""));
    assert!(audit.contains("\"name\":\"refresh\""));
    assert!(audit.contains("\"event\":\"refreshed\""));
    Ok(())
}

#[test]
fn models_refresh_nodes_reject_invalid_input() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let provider = crate::default_provider_spec()?;
    let provider_refresh = fs.provider_models_file_inode(provider.id, "refresh")?;
    let provider_health_check = fs.provider_health_file_inode(provider.id, "check")?;
    let provider_secret_rotate = fs.provider_secrets_file_inode(provider.id, "rotate")?;
    let user_refresh = fs.user_models_file_inode("refresh")?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;

    assert!(runtime.write(provider_refresh, 0, b"yes\n").is_err());
    assert!(runtime.write(provider_refresh, 1, b"1\n").is_err());
    assert!(runtime.write(provider_health_check, 0, b"yes\n").is_err());
    assert!(runtime.write(provider_health_check, 1, b"1\n").is_err());
    assert!(runtime.write(provider_secret_rotate, 0, b"yes\n").is_err());
    assert!(runtime.write(provider_secret_rotate, 1, b"1\n").is_err());
    assert!(runtime.write(user_refresh, 0, b"yes\n").is_err());
    assert!(runtime.write(user_refresh, 1, b"1\n").is_err());
    drop(runtime);
    Ok(())
}

#[test]
fn projection_exposes_space_model_access_view() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    for provider in crate::PROVIDER_SPECS {
        let user_model = fs.user_model_dir_inode(provider)?;
        assert_eq!(
            runtime
                .lookup_child(user_model, "allowed")
                .and_then(crate::Node::content),
            Some("1\n")
        );
        assert_eq!(
            runtime
                .lookup_child(user_model, "reason")
                .and_then(crate::Node::content),
            Some("ready\n")
        );
        assert_eq!(
            runtime
                .lookup_child(user_model, "context_window")
                .and_then(crate::Node::content),
            Some(provider.context_window)
        );
        assert_eq!(
            runtime
                .lookup_child(user_model, "max_output_tokens")
                .and_then(crate::Node::content),
            Some(provider.max_output_tokens)
        );
    }
    let models_compat = fs
        .tree
        .path_inode(&["home", "1000", "models"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert_eq!(
        runtime
            .lookup_child(models_compat, "list")
            .and_then(crate::Node::content),
        Some(crate::global_model_list().as_str()),
        "user models compatibility path must expose the same index content"
    );
    drop(runtime);
    assert!(fs.lookup_path(["home", "1000", "model", "count"]).is_none());
    assert!(fs.lookup_path(["home", "1000", "model", "list"]).is_none());
    assert_eq!(
        user_models_file_content(&fs, "count")?,
        crate::global_model_count()
    );
    assert_eq!(
        user_models_file_content(&fs, "list")?,
        crate::global_model_list()
    );
    Ok(())
}

#[test]
fn user_models_compat_refresh_uses_primary_model_control_name() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let models_compat = fs
        .tree
        .path_inode(&["home", "1000", "models"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let refresh = {
        let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime
            .lookup_child(models_compat, "refresh")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?
    };

    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        assert_eq!(runtime.write(refresh, 0, b"1\n")?, 2);
        drop(runtime);
    }

    assert_eq!(
        fs.node_content(fs.control_file_inode("last_control")?)?,
        crate::LOCAL_USER_MODELS_REFRESH_DISPLAY_TEXT
    );
    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains("\"format\":\"space.users.1000.model\""));
    assert!(audit.contains("\"name\":\"refresh\""));
    assert!(audit.contains("\"event\":\"refreshed\""));
    Ok(())
}

#[test]
fn secondary_provider_config_updates_effective_values() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let provider = crate::PROVIDER_SPECS
        .get(1)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let url = fs.provider_child_dir_inode(provider.id, "url")?;
    let enabled = fs.provider_child_dir_inode(provider.id, "enabled")?;
    let health = fs.provider_child_dir_inode(provider.id, "health")?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;

    let url_current = runtime
        .lookup_child(url, "current")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    runtime.write(url_current, 0, b"https://relay.example.test/v1\n")?;
    assert_eq!(
        runtime
            .lookup_child(url, "effective")
            .and_then(crate::Node::content),
        Some("https://relay.example.test/v1\n")
    );
    assert_eq!(
        runtime
            .lookup_child(url, "source")
            .and_then(crate::Node::content),
        Some("current\n")
    );

    let enabled_current = runtime
        .lookup_child(enabled, "current")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    runtime.write(enabled_current, 0, b"0\n")?;
    assert_eq!(
        runtime
            .lookup_child(enabled, "effective")
            .and_then(crate::Node::content),
        Some("0\n")
    );
    assert_eq!(
        runtime
            .lookup_child(health, "status")
            .and_then(crate::Node::content),
        Some("disabled\n")
    );
    drop(runtime);

    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains(&format!("\"format\":\"provider.{}.url\"", provider.id)));
    assert!(audit.contains(&format!("\"format\":\"provider.{}.enabled\"", provider.id)));
    Ok(())
}

#[test]
fn primary_provider_url_current_updates_effective_and_source() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let provider = crate::PROVIDER_SPECS
        .first()
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let url = fs.provider_child_dir_inode(provider.id, "url")?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let current = runtime
        .lookup_child(url, "current")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    runtime.write(current, 0, b"http://127.0.0.1:11435\n")?;
    assert_eq!(
        runtime
            .lookup_child(url, "effective")
            .and_then(crate::Node::content),
        Some("http://127.0.0.1:11435\n")
    );
    assert_eq!(
        runtime
            .lookup_child(url, "source")
            .and_then(crate::Node::content),
        Some("current\n")
    );
    drop(runtime);
    assert!(
        fs.node_content(fs.audit_events_inode()?)?
            .contains(&format!("\"format\":\"provider.{}.url\"", provider.id))
    );
    Ok(())
}

#[test]
fn primary_provider_enabled_current_updates_effective_and_source() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let provider = crate::PROVIDER_SPECS
        .first()
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let enabled = fs.provider_child_dir_inode(provider.id, "enabled")?;
    let health = fs.provider_child_dir_inode(provider.id, "health")?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let current = runtime
        .lookup_child(enabled, "current")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;

    runtime.write(current, 0, b"0\n")?;
    assert_eq!(
        runtime
            .lookup_child(enabled, "current")
            .and_then(crate::Node::content),
        Some("0\n")
    );
    assert_eq!(
        runtime
            .lookup_child(enabled, "effective")
            .and_then(crate::Node::content),
        Some("0\n")
    );
    assert_eq!(
        runtime
            .lookup_child(enabled, "source")
            .and_then(crate::Node::content),
        Some("current\n")
    );
    assert_eq!(
        runtime
            .lookup_child(health, "status")
            .and_then(crate::Node::content),
        Some("disabled\n")
    );

    runtime.write(current, 0, b"\n")?;
    assert_eq!(
        runtime
            .lookup_child(enabled, "current")
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        runtime
            .lookup_child(enabled, "effective")
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        runtime
            .lookup_child(enabled, "source")
            .and_then(crate::Node::content),
        Some("default\n")
    );
    assert_eq!(
        runtime
            .lookup_child(health, "status")
            .and_then(crate::Node::content),
        Some("ready\n")
    );
    assert_eq!(
        runtime.write(current, 0, b"true\n"),
        Err(fuse3::Errno::from(libc::EINVAL))
    );
    drop(runtime);

    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains(&format!("\"format\":\"provider.{}.enabled\"", provider.id)));
    assert!(!audit.contains("true"));
    Ok(())
}

#[test]
fn projection_exposes_space_routes_and_policy() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let default_provider = crate::default_provider_spec()?;

    let routes = fs
        .tree
        .path_inode(crate::USER_ROUTES_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let policy = fs
        .tree
        .path_inode(crate::USER_POLICY_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert_eq!(
        runtime
            .lookup_child(routes, "default_provider")
            .and_then(crate::Node::content),
        Some(format!("{}\n", default_provider.id).as_str())
    );
    assert_openai_chat_route(
        &runtime,
        routes,
        format!("{}\n", default_provider.id).as_str(),
        provider_model_text(default_provider.id).as_str(),
        "ready\n",
    )?;
    for format in [
        "openai.responses",
        "anthropic.messages",
        "google.generate_content",
    ] {
        let provider = crate::providers_for_format(format)
            .find(|provider| provider.account_type.trim() != "local_runtime")
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        assert_format_route(
            &runtime,
            routes,
            format,
            format!("{}\n", provider.id).as_str(),
            format!("{}\n", provider.default_model).as_str(),
            "ready\n",
        )?;
    }
    assert_eq!(
        runtime
            .lookup_child(policy, "allowed_providers")
            .and_then(crate::Node::content),
        Some(crate::default_allowed_providers_content().as_str())
    );
    drop(runtime);
    assert_eq!(
        fs.lookup_path(["spaces", "users", "1000", "context"])
            .and_then(crate::Node::content),
        Some(crate::LOCAL_USER_SPACE_CONTEXT_TEXT)
    );
    assert_eq!(
        fs.lookup_path(["spaces", "users", "1000", "uid"])
            .and_then(crate::Node::content),
        Some(crate::LOCAL_USER_UID_TEXT)
    );
    Ok(())
}

#[test]
fn unsupported_api_format_route_denies_submit_at_rename() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let anthropic = crate::providers_for_format("anthropic.messages")
        .next()
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let default = crate::default_provider_spec()?;
    let policy = fs
        .tree
        .path_inode(crate::USER_POLICY_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let routes = fs
        .tree
        .path_inode(crate::USER_ROUTES_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let allowed_providers = runtime
            .lookup_child(policy, "allowed_providers")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        runtime.write(allowed_providers, 0, format!("{}\n", default.id).as_bytes())?;
        assert_format_route(
            &runtime,
            routes,
            "anthropic.messages",
            format!("{}\n", anthropic.id).as_str(),
            "\n",
            "policy_denied\n",
        )?;
        drop(runtime);
    }

    fs.create_staged_request("anthropic.messages", "unsupported.tmp", "{}\n")?;
    assert_eq!(
        fs.submit_request(
            "anthropic.messages",
            "unsupported.tmp",
            "unsupported.req.json",
        ),
        Err(fuse3::Errno::from(libc::EACCES))
    );

    let inbox = fs
        .tree
        .path_inode(&[
            "spaces",
            "users",
            "1000",
            "api",
            "anthropic.messages",
            "inbox",
        ])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let outbox = fs
        .tree
        .path_inode(&[
            "spaces",
            "users",
            "1000",
            "api",
            "anthropic.messages",
            "outbox",
        ])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert!(runtime.lookup_child(inbox, "unsupported.tmp").is_some());
    assert!(
        runtime
            .lookup_child(outbox, "unsupported.fingerprint")
            .is_none()
    );
    drop(runtime);
    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "0\n"
    );
    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains("\"format\":\"anthropic.messages\""));
    assert!(audit.contains("\"event\":\"denied\""));
    Ok(())
}

#[test]
fn user_allowed_providers_updates_policy_and_audit() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let default_provider = crate::default_provider_spec()?;
    let policy = fs
        .tree
        .path_inode(crate::USER_POLICY_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let allowed_providers = runtime
        .lookup_child(policy, "allowed_providers")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;

    runtime.write(
        allowed_providers,
        0,
        format!("{}\n{}\n", default_provider.id, default_provider.id).as_bytes(),
    )?;
    assert_eq!(
        runtime
            .lookup_child(policy, "allowed_providers")
            .and_then(crate::Node::content),
        Some(format!("{}\n", default_provider.id).as_str())
    );

    runtime.write(allowed_providers, 0, b"\n")?;
    assert_eq!(
        runtime
            .lookup_child(policy, "allowed_providers")
            .and_then(crate::Node::content),
        Some(crate::default_allowed_providers_content().as_str())
    );
    assert_eq!(
        runtime.write(
            allowed_providers,
            0,
            format!("{}\n", crate::invalid_provider_id()).as_bytes()
        ),
        Err(fuse3::Errno::from(libc::EINVAL))
    );
    drop(runtime);

    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains("\"format\":\"space.users.1000.policy\""));
    assert!(!audit.contains(crate::invalid_provider_id()));
    Ok(())
}

#[test]
fn user_default_provider_is_constrained_by_allowed_providers() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let default = crate::default_provider_spec()?;
    let alternate = crate::alternate_provider_spec(&default)?;
    let policy = fs
        .tree
        .path_inode(crate::USER_POLICY_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let routes = fs
        .tree
        .path_inode(crate::USER_ROUTES_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let allowed_providers = runtime
        .lookup_child(policy, "allowed_providers")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let default_provider = runtime
        .lookup_child(routes, "default_provider")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;

    runtime.write(allowed_providers, 0, format!("{}\n", default.id).as_bytes())?;
    assert_eq!(
        runtime.write(
            default_provider,
            0,
            format!("{}\n", alternate.id).as_bytes()
        ),
        Err(fuse3::Errno::from(libc::EACCES))
    );
    assert_eq!(
        runtime
            .lookup_child(routes, "default_provider")
            .and_then(crate::Node::content),
        Some(format!("{}\n", default.id).as_str())
    );
    drop(runtime);
    Ok(())
}

#[test]
fn user_allowed_providers_cannot_exclude_current_default_provider() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let default = crate::default_provider_spec()?;
    let alternate = crate::alternate_provider_spec(&default)?;
    let policy = fs
        .tree
        .path_inode(crate::USER_POLICY_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let routes = fs
        .tree
        .path_inode(crate::USER_ROUTES_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let allowed_providers = runtime
        .lookup_child(policy, "allowed_providers")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let default_provider = runtime
        .lookup_child(routes, "default_provider")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;

    runtime.write(
        default_provider,
        0,
        format!("{}\n", alternate.id).as_bytes(),
    )?;
    assert_eq!(
        runtime.write(allowed_providers, 0, format!("{}\n", default.id).as_bytes()),
        Err(fuse3::Errno::from(libc::EINVAL))
    );
    assert_eq!(
        runtime
            .lookup_child(policy, "allowed_providers")
            .and_then(crate::Node::content),
        Some(crate::default_allowed_providers_content().as_str())
    );
    drop(runtime);
    Ok(())
}

#[test]
fn user_model_access_tracks_allowed_provider_policy() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let primary = crate::default_provider_spec()?;
    let secondary = crate::alternate_provider_for_format(&primary, "openai.chat")?;
    let policy = fs
        .tree
        .path_inode(crate::USER_POLICY_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let model = fs.user_model_dir_inode(&primary)?;
    let secondary_model = fs.user_model_dir_inode(&secondary)?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let allowed_providers = runtime
        .lookup_child(policy, "allowed_providers")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let routes = fs
        .tree
        .path_inode(crate::USER_ROUTES_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let default_provider = runtime
        .lookup_child(routes, "default_provider")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;

    runtime.write(
        default_provider,
        0,
        format!("{}\n", secondary.id).as_bytes(),
    )?;
    runtime.write(
        allowed_providers,
        0,
        format!("{}\n", secondary.id).as_bytes(),
    )?;
    assert_eq!(
        runtime
            .lookup_child(model, "allowed")
            .and_then(crate::Node::content),
        Some("0\n")
    );
    assert_eq!(
        runtime
            .lookup_child(model, "reason")
            .and_then(crate::Node::content),
        Some("policy_denied\n")
    );
    assert_eq!(
        runtime
            .lookup_child(secondary_model, "allowed")
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        runtime
            .lookup_child(secondary_model, "reason")
            .and_then(crate::Node::content),
        Some("ready\n")
    );
    assert_openai_chat_route(
        &runtime,
        routes,
        format!("{}\n", secondary.id).as_str(),
        provider_model_text(secondary.id).as_str(),
        "ready\n",
    )?;

    runtime.write(allowed_providers, 0, b"\n")?;
    assert_eq!(
        runtime
            .lookup_child(model, "allowed")
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        runtime
            .lookup_child(model, "reason")
            .and_then(crate::Node::content),
        Some("ready\n")
    );
    drop(runtime);
    Ok(())
}

#[test]
fn user_model_access_tracks_provider_enabled_state() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let primary = crate::default_provider_spec()?;
    let secondary = crate::alternate_provider_for_format(&primary, "openai.chat")?;
    let enabled = fs.provider_child_dir_inode(primary.id, "enabled")?;
    let model = fs.user_model_dir_inode(&primary)?;
    let secondary_model = fs.user_model_dir_inode(&secondary)?;
    let routes = fs
        .tree
        .path_inode(crate::USER_ROUTES_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let current = runtime
        .lookup_child(enabled, "current")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;

    runtime.write(current, 0, b"0\n")?;
    assert_eq!(
        runtime
            .lookup_child(model, "allowed")
            .and_then(crate::Node::content),
        Some("0\n")
    );
    assert_eq!(
        runtime
            .lookup_child(model, "reason")
            .and_then(crate::Node::content),
        Some("provider_disabled\n")
    );
    assert_eq!(
        runtime
            .lookup_child(secondary_model, "allowed")
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        runtime
            .lookup_child(secondary_model, "reason")
            .and_then(crate::Node::content),
        Some("ready\n")
    );
    assert_openai_chat_route(
        &runtime,
        routes,
        format!("{}\n", primary.id).as_str(),
        "\n",
        "provider_disabled\n",
    )?;

    runtime.write(current, 0, b"\n")?;
    assert_eq!(
        runtime
            .lookup_child(model, "allowed")
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        runtime
            .lookup_child(model, "reason")
            .and_then(crate::Node::content),
        Some("ready\n")
    );
    assert_openai_chat_route(
        &runtime,
        routes,
        format!("{}\n", primary.id).as_str(),
        provider_model_text(primary.id).as_str(),
        "ready\n",
    )?;
    drop(runtime);
    Ok(())
}

#[test]
fn user_model_access_tracks_secondary_provider_enabled_state() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let primary = crate::default_provider_spec()?;
    let provider = crate::alternate_provider_for_format(&primary, "openai.chat")?;
    let enabled = fs.provider_child_dir_inode(provider.id, "enabled")?;
    let model = fs.user_model_dir_inode(&provider)?;
    let routes = fs
        .tree
        .path_inode(crate::USER_ROUTES_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let default_provider = runtime
        .lookup_child(routes, "default_provider")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    runtime.write(default_provider, 0, format!("{}\n", provider.id).as_bytes())?;
    let current = runtime
        .lookup_child(enabled, "current")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;

    runtime.write(current, 0, b"0\n")?;
    assert_eq!(
        runtime
            .lookup_child(model, "allowed")
            .and_then(crate::Node::content),
        Some("0\n")
    );
    assert_eq!(
        runtime
            .lookup_child(model, "reason")
            .and_then(crate::Node::content),
        Some("provider_disabled\n")
    );
    assert_openai_chat_route(
        &runtime,
        routes,
        format!("{}\n", provider.id).as_str(),
        "\n",
        "provider_disabled\n",
    )?;
    drop(runtime);
    Ok(())
}

#[test]
fn user_default_provider_updates_route_and_audit() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let default = crate::default_provider_spec()?;
    let alternate = crate::alternate_provider_spec(&default)?;
    let routes = fs
        .tree
        .path_inode(crate::USER_ROUTES_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let default_provider = runtime
        .lookup_child(routes, "default_provider")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;

    runtime.write(
        default_provider,
        0,
        format!("{}\n", alternate.id).as_bytes(),
    )?;
    assert_eq!(
        runtime
            .lookup_child(routes, "default_provider")
            .and_then(crate::Node::content),
        Some(format!("{}\n", alternate.id).as_str())
    );
    assert_openai_chat_route(
        &runtime,
        routes,
        format!("{}\n", alternate.id).as_str(),
        provider_model_text(alternate.id).as_str(),
        "ready\n",
    )?;

    runtime.write(default_provider, 0, b"\n")?;
    assert_eq!(
        runtime
            .lookup_child(routes, "default_provider")
            .and_then(crate::Node::content),
        Some(format!("{}\n", default.id).as_str())
    );
    assert_openai_chat_route(
        &runtime,
        routes,
        format!("{}\n", default.id).as_str(),
        provider_model_text(default.id).as_str(),
        "ready\n",
    )?;
    assert_eq!(
        runtime.write(
            default_provider,
            0,
            format!("{}\n", crate::invalid_provider_id()).as_bytes()
        ),
        Err(fuse3::Errno::from(libc::EINVAL))
    );
    drop(runtime);

    let audit = fs.node_content(fs.audit_events_inode()?)?;
    assert!(audit.contains("\"format\":\"space.users.1000.route\""));
    assert!(!audit.contains(crate::invalid_provider_id()));
    Ok(())
}

#[test]
fn user_routes_compat_default_provider_writes_primary_route() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let default = crate::default_provider_spec()?;
    let alternate = crate::alternate_provider_spec(&default)?;
    let route = fs
        .tree
        .path_inode(crate::USER_ROUTES_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let routes_compat = fs
        .tree
        .path_inode(&["home", "1000", "routes"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let default_provider = runtime
        .lookup_child(routes_compat, "default_provider")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;

    runtime.write(
        default_provider,
        0,
        format!("{}\n", alternate.id).as_bytes(),
    )?;

    assert_eq!(
        runtime
            .lookup_child(route, "default_provider")
            .and_then(crate::Node::content),
        Some(format!("{}\n", alternate.id).as_str())
    );
    assert_eq!(
        runtime
            .lookup_child(routes_compat, "default_provider")
            .and_then(crate::Node::content),
        Some(format!("{}\n", alternate.id).as_str())
    );
    assert_openai_chat_route(
        &runtime,
        route,
        format!("{}\n", alternate.id).as_str(),
        provider_model_text(alternate.id).as_str(),
        "ready\n",
    )?;
    assert_openai_chat_route(
        &runtime,
        routes_compat,
        format!("{}\n", alternate.id).as_str(),
        provider_model_text(alternate.id).as_str(),
        "ready\n",
    )?;
    drop(runtime);
    Ok(())
}
