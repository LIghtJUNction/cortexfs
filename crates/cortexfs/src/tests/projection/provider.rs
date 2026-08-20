fn assert_model_entries(projection: &FuseProjection, path: &str, expected: &[&str]) {
    let entries = projection.readdir(path);
    assert!(entries.is_ok());
    let names = entries
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.name().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(names, expected);
}

#[test]
fn fuse_projection_ignores_provider_backing_control_content() -> std::io::Result<()> {
    let root = clean_test_dir("fuse-provider-backing-control");
    let providers = root.join("providers.d");
    write_text_file(
        &providers.join("local.json"),
        r#"{"name":"local","base_url":"http://127.0.0.1/v1","models":["chat"]}"#,
    );
    write_text_file(&root.join("model/local/chat.d/effort"), "high\n");
    let projection = FuseProjection::new(&root).with_provider_config_dir(&providers);

    assert_eq!(
        projection.read_to_string("model/local/chat.d/effort"),
        Ok("auto\n".to_owned())
    );
    assert!(matches!(
        projection.getattr("model/local/chat.d/effort"),
        Ok(ref attr) if attr.size() == 5 && attr.mode() == 0o444
    ));
    assert_eq!(
        projection.write_control_file("model/local/chat.d/effort", "high\n"),
        Err(FuseError::ReadOnly)
    );
    assert_eq!(
        fs::read_to_string(root.join("model/local/chat.d/effort"))?,
        "high\n"
    );
    Ok(())
}

#[test]
fn fuse_projection_hides_reserved_and_inactive_wrong_kind_residue() -> std::io::Result<()> {
    let root = clean_test_dir("fuse-provider-reserved-residue");
    let providers = root.join("providers.d");
    let cache = root.join("provider-models");
    fs::create_dir_all(&providers)?;
    fs::create_dir_all(&cache)?;
    fs::create_dir_all(root.join("model/debug"))?;
    write_text_file(&root.join("model/debug/foreign"), "hidden\n");
    fs::DirBuilder::new().create(root.join("model/route"))?;
    fs::DirBuilder::new().create(root.join("model/main"))?;
    symlink("debug", root.join("model/inactive"))?;
    let projection = FuseProjection::new(&root)
        .with_provider_config_dir(&providers)
        .with_provider_model_cache_dir(&cache);

    assert_model_entries(&projection, "model/debug", &["echo", "echo.d"]);
    for path in ["model/debug/foreign", "model/inactive"] {
        assert_eq!(projection.getattr(path), Err(FuseError::NotFound));
        assert_eq!(projection.read_to_string(path), Err(FuseError::NotFound));
        assert_eq!(projection.readdir(path), Err(FuseError::NotFound));
    }
    assert!(matches!(
        projection.getattr("model/route"),
        Ok(ref attr) if attr.file_type() == FuseFileType::Regular
    ));
    assert_eq!(
        projection.read_to_string("model/route"),
        Ok(crate::DEFAULT_MODEL_ROUTE.to_owned())
    );
    assert!(matches!(
        projection.getattr("model/main"),
        Ok(ref attr) if attr.file_type() == FuseFileType::Symlink
    ));
    assert_eq!(
        projection.readlink("model/main"),
        Ok(PathBuf::from("/ctx/model/debug/echo"))
    );
    Ok(())
}

#[test]
fn fuse_provider_readdir_uses_one_config_snapshot() {
    let root = clean_test_dir("fuse-provider-one-snapshot");
    let providers = root.join("providers.d");
    let config = providers.join("local.json");
    write_text_file(
        &config,
        r#"{"name":"local","base_url":"http://127.0.0.1/v1","models":["old"]}"#,
    );
    let replacement = config;
    let previous = crate::provider::set_load_hook(Some(Box::new(move || {
        write_text_file(
            &replacement,
            r#"{"name":"local","base_url":"http://127.0.0.1/v1","models":["new"]}"#,
        );
    })));
    let projection = FuseProjection::new(&root).with_provider_config_dir(&providers);

    assert_model_entries(&projection, "model/local", &["old", "old.d"]);
    let _previous = crate::provider::set_load_hook(previous);
    assert_model_entries(&projection, "model/local", &["new", "new.d"]);
}

#[test]
fn fuse_provider_fallback_uses_the_virtual_lookup_snapshot() {
    let root = clean_test_dir("fuse-provider-fallback-snapshot");
    let providers = root.join("providers.d");
    let cache = root.join("provider-models");
    let config = providers.join("local.json");
    write_text_file(
        &config,
        r#"{"name":"local","base_url":"http://127.0.0.1/v1","models":["chat"]}"#,
    );
    write_text_file(&root.join("model/offline"), "#!/bin/sh\n");
    assert!(fs::create_dir_all(&cache).is_ok());
    let replacement = config;
    let previous = crate::provider::set_load_hook(Some(Box::new(move || {
        write_text_file(&replacement, "{");
    })));
    let projection = FuseProjection::new(&root)
        .with_provider_config_dir(&providers)
        .with_provider_model_cache_dir(&cache);

    let attr = projection.getattr("model/offline");
    let _previous = crate::provider::set_load_hook(previous);
    assert!(matches!(
        attr,
        Ok(ref attr) if attr.file_type() == FuseFileType::Regular
    ));
}

#[test]
fn fuse_projection_projects_configured_provider_models() -> std::io::Result<()> {
    let root = reference_tree("fuse-provider-model");
    let providers = root.join("providers.d");
    let cache = root.join("provider-models");
    write_text_file(
        &providers.join("api.test.json"),
        r#"{
  "base_url": "https://api.test:9000/",
  "default_model": "gpt-5.6-terra",
  "enabled": true,
  "formats": ["openai.chat", "openai.responses"]
}
"#,
    );
    write_text_file(
        &cache.join("api.test.models.json"),
        r#"{"models":["gpt-5.6","gpt-5.6-terra","gpt-5.6-sol","bad/name"]}"#,
    );
    let projection = FuseProjection::new(&root)
        .with_provider_config_dir(&providers)
        .with_provider_model_cache_dir(&cache);
    let virtual_main = projection.readlink("model/main");
    crate::ensure_runtime_models_from(&root, &providers, &cache)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    assert_eq!(virtual_main, Ok(fs::read_link(root.join("model/main"))?));

    assert_model_entries(
        &projection,
        "model",
        &[
            "api.test", "code", "debug", "fast", "helper", "main", "reason", "route", "vision",
        ],
    );
    let route = projection.read_to_string("model/route");
    assert!(matches!(route, Ok(ref content) if content.contains("fallback: direct")));
    assert_eq!(
        projection.write_control_file(
            "model/route",
            "group(proxy) -> http(http://127.0.0.1:8080/v1), key(office)\nmodel(gpt-*) -> proxy\nfallback: direct\n"
        ),
        Ok(())
    );
    assert_eq!(
        projection.read_to_string("model/route"),
        Ok("group(proxy) -> http(http://127.0.0.1:8080/v1), key(office)\nmodel(gpt-*) -> proxy\nfallback: direct\n".to_owned())
    );

    assert_model_entries(
        &projection,
        "model/api.test",
        &[
            "gpt-5.6",
            "gpt-5.6-sol",
            "gpt-5.6-sol.d",
            "gpt-5.6-terra",
            "gpt-5.6-terra.d",
            "gpt-5.6.d",
        ],
    );

    let metadata = projection.read_to_string("model/api.test/gpt-5.6-terra");
    assert!(matches!(
        metadata,
        Ok(ref content)
            if content.starts_with(&format!("#!{CORTEXFS_OBJECT_RUNNER}\n"))
                && content.contains("# cortexfs.name=api.test/gpt-5.6-terra\n")
                && content.contains("# cortexfs.driver.default=openai-chat\n")
                && content.contains("# cortexfs.driver.agent=openai-responses,openai-chat\n")
    ));
    assert_eq!(
        projection.read_to_string("model/api.test/gpt-5.6-terra.d/driver"),
        Ok(
            "default=openai-chat\nexec=openai-chat\nagent=openai-responses,openai-chat\n"
                .to_owned()
        )
    );
    assert_eq!(
        projection.read_to_string("model/api.test/gpt-5.6-terra.d/default"),
        Ok("base_url=https://api.test:9000/\n".to_owned())
    );
    assert!(fs::create_dir_all(root.join("model/api.test/gpt-5.6-terra.d/hooks/pre.d")).is_ok());
    assert!(fs::create_dir_all(root.join("model/api.test/gpt-5.6-terra.d/hooks/post.d")).is_ok());
    let hooks_attr = projection.getattr("model/api.test/gpt-5.6-terra.d/hooks");
    assert!(matches!(
        hooks_attr,
        Ok(ref attr) if attr.file_type() == FuseFileType::Directory
    ));
    let hook_entries = projection.readdir("model/api.test/gpt-5.6-terra.d/hooks");
    assert_eq!(
        hook_entries.map(|entries| entries
            .into_iter()
            .map(|entry| entry.name().to_owned())
            .collect::<Vec<_>>()),
        Ok(vec!["post.d".to_owned(), "pre.d".to_owned()])
    );
    assert_eq!(
        projection.read_to_string("model/api.test/gpt-5.6-terra.d/effort"),
        Ok("auto\n".to_owned())
    );
    assert_eq!(
        projection.read_to_string("model/api.test/gpt-5.6-terra.d/recommended"),
        Ok("unknown\n".to_owned())
    );
    assert_eq!(
        projection.read_to_string("model/api.test/gpt-5.6-terra.d/compact"),
        Ok("unknown\n".to_owned())
    );
    assert_eq!(
        projection.write_control_file("model/api.test/gpt-5.6-terra.d/effort", "high\n"),
        Err(FuseError::ReadOnly)
    );
    assert_eq!(
        projection.read_to_string("model/api.test/gpt-5.6-terra.d/effort"),
        Ok("auto\n".to_owned())
    );
    let attr = projection.getattr("model/api.test/gpt-5.6-terra");
    assert!(matches!(attr, Ok(ref attr) if attr.mode() & 0o777 == 0o555));
    Ok(())
}

#[test]
fn fuse_projection_exposes_complete_models_dev_record() -> std::io::Result<()> {
    let root = reference_tree("fuse-provider-model-metadata");
    let providers = root.join("providers.d");
    let cache = root.join("provider-models");
    write_text_file(
        &providers.join("local.json"),
        r#"{"name":"local","base_url":"https://api.test/v1","models":["known"]}"#,
    );
    let raw = serde_json::json!({
        "id": "known",
        "name": "Known",
        "attachment": false,
        "reasoning": true,
        "tool_call": true,
        "modalities": {"input": ["text"], "output": ["text"]},
        "open_weights": false,
        "limit": {"context": 1_000_000, "output": 32_768},
        "future_field": "retained-through-file-abi"
    });
    let cache_document = serde_json::json!({
        "schema": cortexfs_metadatas::MODEL_METADATA_SCHEMA,
        "catalog": {
            "providers": {"local": {
                "id": "local", "name": "Local", "doc": "https://example.invalid",
                "models": {"known": raw}
            }},
            "models": {"local/known": {
                "id": "local/known", "name": "Known",
                "benchmarks": [{"name": "Example", "score": 90}]
            }}
        }
    });
    write_text_file(
        &cache.join("model-metadata.json"),
        &serde_json::to_string(&cache_document)?,
    );
    let projection = FuseProjection::new(&root)
        .with_provider_config_dir(&providers)
        .with_provider_model_cache_dir(&cache);

    let content = projection
        .read_to_string("model/local/known.d/metadata.json")
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let document: serde_json::Value = serde_json::from_str(&content)?;
    assert_eq!(document.pointer("/metadata/models_dev"), Some(&raw));
    assert_eq!(
        document.pointer("/metadata/models_dev/future_field"),
        Some(&serde_json::Value::from("retained-through-file-abi")),
    );
    assert_eq!(
        document.pointer("/effective/limit_tokens"),
        Some(&serde_json::Value::from(1_000_000)),
    );
    assert!(matches!(
        projection.getattr("model/local/known.d/metadata.json"),
        Ok(ref attr) if attr.mode() & 0o777 == 0o444
    ));
    assert_eq!(
        projection
            .read_at("model/local/known.d/metadata.json", 0, content.len())
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?,
        content.as_bytes(),
    );
    Ok(())
}

#[test]
fn fuse_projection_keeps_unmapped_aggregator_metadata_unverified() -> std::io::Result<()> {
    let root = reference_tree("fuse-provider-model-deepseek-alias");
    let providers = root.join("providers.d");
    write_text_file(
        &providers.join("lmm.json"),
        r#"{"name":"lmm","base_url":"https://api.lmm.best/v1","models":["deepseek-v4-flash-0731"]}"#,
    );
    let projection = FuseProjection::new(&root).with_provider_config_dir(&providers);

    let content = projection
        .read_to_string("model/lmm/deepseek-v4-flash-0731.d/metadata.json")
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let document: serde_json::Value = serde_json::from_str(&content)?;
    assert_eq!(
        document.pointer("/metadata/provider"),
        Some(&serde_json::Value::from("lmm"))
    );
    assert_eq!(
        document.pointer("/metadata/id"),
        Some(&serde_json::Value::from("deepseek-v4-flash-0731"))
    );
    assert_eq!(
        document.pointer("/resolution"),
        Some(&serde_json::Value::from("unverified"))
    );
    assert_eq!(
        document.pointer("/canonical_id"),
        Some(&serde_json::Value::from("lmm/deepseek-v4-flash-0731"))
    );
    assert_eq!(document.pointer("/metadata/models_dev"), None);
    assert_eq!(
        projection.read_to_string("model/lmm/deepseek-v4-flash-0731.d/limit"),
        Ok("unknown\n".to_owned())
    );
    assert_eq!(
        projection.read_to_string("model/lmm/deepseek-v4-flash-0731.d/cap"),
        Ok("chat\nstream\n".to_owned())
    );
    Ok(())
}

#[test]
fn fuse_projection_uses_the_explicit_custom_model_context_limit() {
    let root = reference_tree("fuse-provider-model-limit");
    let providers = root.join("providers.d");
    write_text_file(
        &providers.join("local.json"),
        r#"{
  "name": "local",
  "base_url": "http://127.0.0.1:8317/v1",
  "models": ["custom-model"],
  "model_limits": {"custom-model": 32768},
  "enabled": true,
  "formats": ["openai.chat"]
}
"#,
    );
    let projection = FuseProjection::new(&root).with_provider_config_dir(&providers);

    assert_eq!(
        projection.read_to_string("model/local/custom-model.d/limit"),
        Ok("32768\n".to_owned())
    );
    assert!(matches!(
        projection.read_to_string("model/local/custom-model"),
        Ok(ref metadata) if metadata.contains("# cortexfs.context_length=32768\n")
    ));
}

#[test]
fn fuse_projection_uses_per_model_capability_overrides() {
    let root = reference_tree("fuse-provider-model-capabilities");
    let providers = root.join("providers.d");
    write_text_file(
        &providers.join("local.json"),
        r#"{
  "name": "local",
  "base_url": "http://127.0.0.1:8317/v1",
  "models": ["text", "vision"],
  "model_capabilities": {
    "text": ["chat", "stream"],
    "vision": ["vision", "chat", "stream"]
  },
  "formats": ["openai.chat", "openai.responses"]
}"#,
    );
    let projection = FuseProjection::new(&root).with_provider_config_dir(&providers);

    assert_eq!(
        projection.read_to_string("model/local/text.d/cap"),
        Ok("chat\nstream\n".to_owned())
    );
    assert_eq!(
        projection.read_to_string("model/local/vision.d/cap"),
        Ok("chat\nstream\nvision\n".to_owned())
    );
}

#[test]
fn fuse_projection_prefers_local_limit_over_catalog_cache() {
    let root = reference_tree("fuse-provider-local-limit-priority");
    let providers = root.join("providers.d");
    let cache = root.join("provider-models");
    write_text_file(
        &providers.join("local.json"),
        r#"{"name":"local","base_url":"http://127.0.0.1/v1","models":["chat"],"model_limits":{"chat":32768}}"#,
    );
    write_text_file(
        &cache.join("model-limits.json"),
        r#"{"schema":"cortexfs.model-limits/v1","models":{"local/chat":65536}}"#,
    );
    let projection = FuseProjection::new(&root)
        .with_provider_config_dir(&providers)
        .with_provider_model_cache_dir(&cache);

    assert_eq!(
        projection.read_to_string("model/local/chat.d/limit"),
        Ok("32768\n".to_owned())
    );
}

#[test]
fn fuse_projection_uses_catalog_limit_without_local_override() {
    let root = reference_tree("fuse-provider-catalog-limit");
    let providers = root.join("providers.d");
    let cache = root.join("provider-models");
    write_text_file(
        &providers.join("local.json"),
        r#"{"name":"local","base_url":"http://127.0.0.1/v1","models":["chat"]}"#,
    );
    write_text_file(
        &cache.join("model-limits.json"),
        r#"{"schema":"cortexfs.model-limits/v1","models":{"local/chat":65536}}"#,
    );
    let projection = FuseProjection::new(&root)
        .with_provider_config_dir(&providers)
        .with_provider_model_cache_dir(&cache);

    assert_eq!(
        projection.read_to_string("model/local/chat.d/limit"),
        Ok("65536\n".to_owned())
    );
}

#[test]
fn fuse_projection_uses_unknown_limit_without_catalog_cache() {
    let root = reference_tree("fuse-provider-unknown-limit");
    let providers = root.join("providers.d");
    write_text_file(
        &providers.join("local.json"),
        r#"{"name":"local","base_url":"http://127.0.0.1/v1","models":["chat"]}"#,
    );
    let projection = FuseProjection::new(&root).with_provider_config_dir(&providers);

    assert_eq!(
        projection.read_to_string("model/local/chat.d/limit"),
        Ok("unknown\n".to_owned())
    );
}

#[test]
fn fuse_projection_rejects_provider_with_invalid_local_limits() {
    for (case, limits) in [("zero", r#"{"chat":0}"#), ("foreign", r#"{"other":32768}"#)] {
        let root = reference_tree(&format!("fuse-provider-invalid-limit-{case}"));
        let providers = root.join("providers.d");
        write_text_file(
            &providers.join("local.json"),
            &format!(
                r#"{{"name":"local","base_url":"http://127.0.0.1/v1","models":["chat"],"model_limits":{limits}}}"#
            ),
        );
        let projection = FuseProjection::new(&root).with_provider_config_dir(&providers);

        assert_eq!(
            projection.getattr("model/local"),
            Err(FuseError::InvalidContent)
        );
    }
}

#[test]
fn fuse_projection_rejects_invalid_model_capability_overrides() {
    for (case, capabilities) in [
        ("foreign-model", r#"{"other":["chat"]}"#),
        ("private-word", r#"{"chat":["openai_responses"]}"#),
        ("duplicate", r#"{"chat":["stream","stream"]}"#),
    ] {
        let root = reference_tree(&format!("fuse-provider-invalid-capability-{case}"));
        let providers = root.join("providers.d");
        write_text_file(
            &providers.join("local.json"),
            &format!(
                r#"{{"name":"local","base_url":"http://127.0.0.1/v1","models":["chat"],"model_capabilities":{capabilities}}}"#
            ),
        );
        let projection = FuseProjection::new(&root).with_provider_config_dir(&providers);

        assert_eq!(
            projection.getattr("model/local"),
            Err(FuseError::InvalidContent)
        );
    }
}

#[test]
fn fuse_projection_hides_noncanonical_debug_hook_dirs() {
    let root = clean_test_dir("fuse-debug-model-hooks");
    assert!(fs::create_dir_all(root.join("model/debug/echo.d/hooks/pre.d")).is_ok());
    assert!(fs::create_dir_all(root.join("model/debug/echo.d/hooks/post.d")).is_ok());
    let projection = FuseProjection::new(&root);

    for path in ["model/debug", "model/debug/echo.d"] {
        let attr = projection.getattr(path);
        assert!(matches!(
            attr,
            Ok(ref attr) if attr.file_type() == FuseFileType::Directory
        ));
    }
    for path in [
        "model/debug/echo.d/hooks",
        "model/debug/echo.d/hooks/pre.d",
        "model/debug/echo.d/hooks/post.d",
    ] {
        assert_eq!(projection.getattr(path), Err(FuseError::NotFound));
    }
    assert_eq!(
        projection.readdir("model/debug/echo.d/hooks"),
        Err(FuseError::NotFound)
    );
}

#[test]
fn fuse_projection_does_not_touch_symlink_provider_model_control_dir() {
    let root = reference_tree("fuse-provider-model-control-symlink");
    let providers = root.join("providers.d");
    let cache = root.join("provider-models");
    let outside = clean_test_dir("fuse-provider-model-control-symlink-outside");
    write_text_file(
        &providers.join("api.test.json"),
        r#"{
  "base_url": "https://api.test:9000/",
  "enabled": true,
  "formats": ["openai.chat"]
}
"#,
    );
    write_text_file(
        &cache.join("api.test.models.json"),
        r#"{"models":["gpt-5.6-terra"]}"#,
    );
    assert!(fs::create_dir_all(root.join("model").join("api.test")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(
        symlink(
            &outside,
            root.join("model").join("api.test").join("gpt-5.6-terra.d")
        )
        .is_ok()
    );
    let projection = FuseProjection::new(&root)
        .with_provider_config_dir(&providers)
        .with_provider_model_cache_dir(&cache);

    assert_eq!(
        projection.write_control_file("model/api.test/gpt-5.6-terra.d/effort", "high\n"),
        Err(FuseError::ReadOnly)
    );
    assert!(!outside.join("effort").exists());
    assert!(
        root.join("model")
            .join("api.test")
            .join("gpt-5.6-terra.d")
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
    );
}

#[test]
fn fuse_projection_requires_name_for_address_provider() {
    let root = reference_tree("fuse-address-provider-requires-name");
    let providers = root.join("providers.d");
    write_text_file(
        &providers.join("local.json"),
        r#"{
  "base_url": "http://127.0.0.1:8317/v1",
  "default_model": "gpt-5.6-terra",
  "enabled": true,
  "formats": ["openai.chat"]
}
"#,
    );
    let projection = FuseProjection::new(&root).with_provider_config_dir(&providers);

    assert_eq!(projection.readdir("model"), Err(FuseError::InvalidContent));
}

#[test]
fn fuse_projection_uses_configured_provider_name_for_address_provider() {
    let root = reference_tree("fuse-address-provider-named");
    let providers = root.join("providers.d");
    write_text_file(
        &providers.join("local.json"),
        r#"{
  "name": "local",
  "base_url": "http://127.0.0.1:8317/v1",
  "default_model": "gpt-5.6-terra",
  "enabled": true,
  "formats": ["openai.chat"]
}
"#,
    );
    let projection = FuseProjection::new(&root).with_provider_config_dir(&providers);

    let model_entries = projection.readdir("model");
    assert!(model_entries.is_ok());
    let model_names = model_entries
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.name().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        model_names,
        [
            "code", "debug", "fast", "helper", "local", "main", "reason", "route", "vision",
        ]
    );
    assert_eq!(
        projection.read_to_string("model/local/gpt-5.6-terra.d/default"),
        Ok("base_url=http://127.0.0.1:8317/v1\n".to_owned())
    );
}

#[test]
fn fuse_projection_rejects_symlink_provider_configs() {
    let root = reference_tree("fuse-symlink-provider-config");
    let providers = root.join("providers.d");
    let outside = root.join("outside-provider.json");
    write_text_file(
        &outside,
        r#"{
  "name": "local",
  "base_url": "http://127.0.0.1:8317/v1",
  "default_model": "gpt-5.6-terra",
  "enabled": true,
  "formats": ["openai.chat"]
}
"#,
    );
    assert!(fs::create_dir_all(&providers).is_ok());
    assert!(symlink(&outside, providers.join("local.json")).is_ok());
    let projection = FuseProjection::new(&root).with_provider_config_dir(&providers);

    assert_eq!(projection.readdir("model"), Err(FuseError::InvalidContent));
    assert_eq!(
        projection.getattr("model/local"),
        Err(FuseError::InvalidContent)
    );
}

#[test]
fn fuse_projection_rejects_symlink_provider_config_dir() {
    let root = reference_tree("fuse-symlink-provider-config-dir");
    let providers = root.join("providers.d");
    let outside = clean_test_dir("fuse-symlink-provider-config-dir-outside");
    write_text_file(
        &outside.join("local.json"),
        r#"{
  "name": "local",
  "base_url": "http://127.0.0.1:8317/v1",
  "default_model": "gpt-5.6-terra",
  "enabled": true,
  "formats": ["openai.chat"]
}
"#,
    );
    assert!(symlink(&outside, &providers).is_ok());
    let projection = FuseProjection::new(&root).with_provider_config_dir(&providers);

    assert_eq!(projection.readdir("model"), Err(FuseError::Io));
    assert_eq!(projection.getattr("model/local"), Err(FuseError::Io));
}

#[test]
fn fuse_projection_ignores_symlink_provider_model_cache() {
    let root = reference_tree("fuse-symlink-provider-model-cache");
    let providers = root.join("providers.d");
    let cache = root.join("provider-models");
    let outside = root.join("outside-provider-cache.json");
    write_text_file(
        &providers.join("local.json"),
        r#"{
  "name": "local",
  "base_url": "http://127.0.0.1:8317/v1",
  "default_model": "base",
  "enabled": true,
  "formats": ["openai.chat"]
}
"#,
    );
    write_text_file(&outside, r#"{"models":["leaked"]}"#);
    assert!(fs::create_dir_all(&cache).is_ok());
    assert!(symlink(&outside, cache.join("local.models.json")).is_ok());
    let projection = FuseProjection::new(&root)
        .with_provider_config_dir(&providers)
        .with_provider_model_cache_dir(&cache);

    let provider_entries = projection.readdir("model/local");
    assert!(provider_entries.is_ok());
    let provider_names = provider_entries
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.name().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(provider_names, ["base", "base.d"]);
    assert_eq!(
        projection.getattr("model/local/leaked"),
        Err(FuseError::NotFound)
    );
}

#[test]
fn fuse_projection_ignores_symlink_provider_model_cache_dir() {
    let root = reference_tree("fuse-symlink-provider-model-cache-dir");
    let providers = root.join("providers.d");
    let cache = root.join("provider-models");
    let outside = clean_test_dir("fuse-symlink-provider-model-cache-dir-outside");
    write_text_file(
        &providers.join("local.json"),
        r#"{
  "name": "local",
  "base_url": "http://127.0.0.1:8317/v1",
  "default_model": "base",
  "enabled": true,
  "formats": ["openai.chat"]
}
"#,
    );
    write_text_file(
        &outside.join("local.models.json"),
        r#"{"models":["leaked"]}"#,
    );
    assert!(symlink(&outside, &cache).is_ok());
    let projection = FuseProjection::new(&root)
        .with_provider_config_dir(&providers)
        .with_provider_model_cache_dir(&cache);

    let provider_entries = projection.readdir("model/local");
    assert!(provider_entries.is_ok());
    let provider_names = provider_entries
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.name().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(provider_names, ["base", "base.d"]);
    assert_eq!(
        projection.getattr("model/local/leaked"),
        Err(FuseError::NotFound)
    );
}

#[test]
fn fuse_projection_ignores_oversized_provider_model_cache() {
    let root = reference_tree("fuse-oversized-provider-model-cache");
    let providers = root.join("providers.d");
    let cache = root.join("provider-models");
    write_text_file(
        &providers.join("api.test.json"),
        r#"{
  "base_url": "https://api.test:9000/",
  "default_model": "gpt-5.6-terra",
  "enabled": true,
  "formats": ["openai.chat"]
}
"#,
    );
    let oversized_padding = "x".repeat(1024 * 1024);
    write_text_file(
        &cache.join("api.test.models.json"),
        &format!(r#"{{"models":["gpt-cache"],"padding":"{oversized_padding}"}}"#),
    );
    let projection = FuseProjection::new(&root)
        .with_provider_config_dir(&providers)
        .with_provider_model_cache_dir(&cache);

    let provider_entries = projection.readdir("model/api.test");
    assert!(provider_entries.is_ok());
    let provider_names = provider_entries
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.name().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(provider_names, ["gpt-5.6-terra", "gpt-5.6-terra.d"]);
}

#[test]
fn fuse_projection_skips_disabled_provider_models() {
    let root = reference_tree("fuse-disabled-provider-model");
    let providers = root.join("providers.d");
    write_text_file(
        &providers.join("api.test.json"),
        r#"{
  "base_url": "https://api.test:9000/",
  "default_model": "gpt-5.6-terra",
  "enabled": false,
  "formats": ["openai.chat"]
}
"#,
    );
    write_text_file(&root.join("model/api.test/gpt-5.6-terra"), "stale\n");
    write_text_file(&root.join("model/offline"), "flat\n");
    let projection = FuseProjection::new(&root).with_provider_config_dir(&providers);

    let model_entries = projection.readdir("model");
    assert!(model_entries.is_ok());
    let model_names = model_entries
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.name().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        model_names,
        [
            "code", "debug", "fast", "helper", "main", "offline", "reason", "route", "vision"
        ]
    );
    assert!(matches!(
        projection.getattr("model/api.test"),
        Err(FuseError::NotFound)
    ));
    assert!(matches!(
        projection.read_to_string("model/api.test/gpt-5.6-terra"),
        Err(FuseError::NotFound)
    ));
    assert_eq!(
        projection.read_to_string("model/offline").as_deref(),
        Ok("flat\n")
    );
}
use super::*;
