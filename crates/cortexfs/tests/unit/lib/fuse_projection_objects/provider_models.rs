#[test]
fn fuse_v1_projection_projects_configured_provider_models() {
    let root = reference_tree("fuse-v1-provider-model");
    let providers = root.join("providers.d");
    let cache = root.join("provider-models");
    write_text_file(
        &providers.join("api.lmm.best.json"),
        r#"{
  "base_url": "https://api.lmm.best:9000/",
  "default_model": "gpt-5.4-mini",
  "enabled": true,
  "formats": ["openai.chat", "openai.responses"]
}
"#,
    );
    write_text_file(
        &cache.join("api.lmm.best.models.json"),
        r#"{"models":["gpt-5.4","gpt-5.4-mini","gpt-5.5","bad/name"]}"#,
    );
    let projection = FuseV1Projection::new(&root)
        .with_provider_config_dir(&providers)
        .with_provider_model_cache_dir(&cache);

    let model_entries = projection.readdir("model");
    assert!(model_entries.is_ok());
    let model_names = model_entries
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.name().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        model_names,
        ["api.lmm.best", "debug", "helper", "main", "route"]
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

    let provider_entries = projection.readdir("model/api.lmm.best");
    assert!(provider_entries.is_ok());
    let provider_names = provider_entries
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.name().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        provider_names,
        [
            "gpt-5.4",
            "gpt-5.4-mini",
            "gpt-5.4-mini.d",
            "gpt-5.4.d",
            "gpt-5.5",
            "gpt-5.5.d"
        ]
    );

    let metadata = projection.read_to_string("model/api.lmm.best/gpt-5.4-mini");
    assert!(matches!(
        metadata,
        Ok(ref content)
            if content.starts_with(&format!("#!{CORTEXFS_OBJECT_RUNNER}\n"))
                && content.contains("# cortexfs.name=api.lmm.best/gpt-5.4-mini\n")
                && content.contains("# cortexfs.driver.default=openai-chat\n")
                && content.contains("# cortexfs.driver.agent=openai-responses,openai-chat\n")
    ));
    assert_eq!(
        projection.read_to_string("model/api.lmm.best/gpt-5.4-mini.d/driver"),
        Ok("default=openai-chat\nexec=openai-chat\nagent=openai-responses,openai-chat\n".to_owned())
    );
    assert_eq!(
        projection.read_to_string("model/api.lmm.best/gpt-5.4-mini.d/default"),
        Ok("base_url=https://api.lmm.best:9000/\n".to_owned())
    );
    assert!(fs::create_dir_all(root.join("model/api.lmm.best/gpt-5.4-mini.d/hooks/pre.d")).is_ok());
    assert!(fs::create_dir_all(root.join("model/api.lmm.best/gpt-5.4-mini.d/hooks/post.d")).is_ok());
    let hooks_attr = projection.getattr("model/api.lmm.best/gpt-5.4-mini.d/hooks");
    assert!(matches!(
        hooks_attr,
        Ok(ref attr) if attr.file_type() == FuseV1FileType::Directory
    ));
    let hook_entries = projection.readdir("model/api.lmm.best/gpt-5.4-mini.d/hooks");
    assert_eq!(
        hook_entries.map(|entries| entries
            .into_iter()
            .map(|entry| entry.name().to_owned())
            .collect::<Vec<_>>()),
        Ok(vec!["post.d".to_owned(), "pre.d".to_owned()])
    );
    assert_eq!(
        projection.read_to_string("model/api.lmm.best/gpt-5.4-mini.d/effort"),
        Ok("auto\n".to_owned())
    );
    assert!(matches!(
        projection.read_to_string("model/api.lmm.best/gpt-5.4-mini.d/fallback"),
        Ok(ref content) if content.contains("api.lmm.best/gpt-5.5\n")
    ));
    assert!(projection
        .write_control_file("model/api.lmm.best/gpt-5.4-mini.d/effort", "high\n")
        .is_ok());
    assert_eq!(
        projection.read_to_string("model/api.lmm.best/gpt-5.4-mini.d/effort"),
        Ok("high\n".to_owned())
    );
    let attr = projection.getattr("model/api.lmm.best/gpt-5.4-mini");
    assert!(matches!(attr, Ok(ref attr) if attr.mode() & 0o777 == 0o555));
}

#[test]
fn fuse_v1_projection_rejects_symlink_provider_model_control_dir() {
    let root = reference_tree("fuse-v1-provider-model-control-symlink");
    let providers = root.join("providers.d");
    let cache = root.join("provider-models");
    let outside = clean_test_dir("fuse-v1-provider-model-control-symlink-outside");
    write_text_file(
        &providers.join("api.lmm.best.json"),
        r#"{
  "base_url": "https://api.lmm.best:9000/",
  "enabled": true,
  "formats": ["openai.chat"]
}
"#,
    );
    write_text_file(
        &cache.join("api.lmm.best.models.json"),
        r#"{"models":["gpt-5.4-mini"]}"#,
    );
    assert!(fs::create_dir_all(root.join("model").join("api.lmm.best")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(
        symlink(
            &outside,
            root.join("model")
                .join("api.lmm.best")
                .join("gpt-5.4-mini.d")
        )
        .is_ok()
    );
    let projection = FuseV1Projection::new(&root)
        .with_provider_config_dir(&providers)
        .with_provider_model_cache_dir(&cache);

    assert_eq!(
        projection.write_control_file("model/api.lmm.best/gpt-5.4-mini.d/effort", "high\n"),
        Err(FuseV1Error::Io)
    );
    assert!(!outside.join("effort").exists());
    assert!(
        root.join("model")
            .join("api.lmm.best")
            .join("gpt-5.4-mini.d")
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
    );
}

#[test]
fn fuse_v1_projection_requires_name_for_address_provider() {
    let root = reference_tree("fuse-v1-address-provider-requires-name");
    let providers = root.join("providers.d");
    write_text_file(
        &providers.join("local.json"),
        r#"{
  "base_url": "http://127.0.0.1:8317/v1",
  "default_model": "gpt-5.4-mini",
  "enabled": true,
  "formats": ["openai.chat"]
}
"#,
    );
    let projection = FuseV1Projection::new(&root).with_provider_config_dir(&providers);

    assert_eq!(projection.readdir("model"), Err(FuseV1Error::InvalidContent));
}

#[test]
fn fuse_v1_projection_uses_configured_provider_name_for_address_provider() {
    let root = reference_tree("fuse-v1-address-provider-named");
    let providers = root.join("providers.d");
    write_text_file(
        &providers.join("local.json"),
        r#"{
  "name": "local",
  "base_url": "http://127.0.0.1:8317/v1",
  "default_model": "gpt-5.4-mini",
  "enabled": true,
  "formats": ["openai.chat"]
}
"#,
    );
    let projection = FuseV1Projection::new(&root).with_provider_config_dir(&providers);

    let model_entries = projection.readdir("model");
    assert!(model_entries.is_ok());
    let model_names = model_entries
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.name().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(model_names, ["debug", "helper", "local", "main", "route"]);
    assert_eq!(
        projection.read_to_string("model/local/gpt-5.4-mini.d/default"),
        Ok("base_url=http://127.0.0.1:8317/v1\n".to_owned())
    );
}

#[test]
fn fuse_v1_projection_ignores_symlink_provider_configs() {
    let root = reference_tree("fuse-v1-symlink-provider-config");
    let providers = root.join("providers.d");
    let outside = root.join("outside-provider.json");
    write_text_file(
        &outside,
        r#"{
  "name": "local",
  "base_url": "http://127.0.0.1:8317/v1",
  "default_model": "gpt-5.4-mini",
  "enabled": true,
  "formats": ["openai.chat"]
}
"#,
    );
    assert!(fs::create_dir_all(&providers).is_ok());
    assert!(symlink(&outside, providers.join("local.json")).is_ok());
    let projection = FuseV1Projection::new(&root).with_provider_config_dir(&providers);

    let model_entries = projection.readdir("model");
    assert!(model_entries.is_ok());
    let model_names = model_entries
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.name().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(model_names, ["debug", "helper", "main", "route"]);
    assert_eq!(projection.getattr("model/local"), Err(FuseV1Error::NotFound));
}

#[test]
fn fuse_v1_projection_rejects_symlink_provider_config_dir() {
    let root = reference_tree("fuse-v1-symlink-provider-config-dir");
    let providers = root.join("providers.d");
    let outside = clean_test_dir("fuse-v1-symlink-provider-config-dir-outside");
    write_text_file(
        &outside.join("local.json"),
        r#"{
  "name": "local",
  "base_url": "http://127.0.0.1:8317/v1",
  "default_model": "gpt-5.4-mini",
  "enabled": true,
  "formats": ["openai.chat"]
}
"#,
    );
    assert!(symlink(&outside, &providers).is_ok());
    let projection = FuseV1Projection::new(&root).with_provider_config_dir(&providers);

    assert_eq!(projection.readdir("model"), Err(FuseV1Error::Io));
    assert_eq!(projection.getattr("model/local"), Err(FuseV1Error::Io));
}

#[test]
fn fuse_v1_projection_ignores_symlink_provider_model_cache() {
    let root = reference_tree("fuse-v1-symlink-provider-model-cache");
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
    let projection = FuseV1Projection::new(&root)
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
        Err(FuseV1Error::NotFound)
    );
}

#[test]
fn fuse_v1_projection_ignores_symlink_provider_model_cache_dir() {
    let root = reference_tree("fuse-v1-symlink-provider-model-cache-dir");
    let providers = root.join("providers.d");
    let cache = root.join("provider-models");
    let outside = clean_test_dir("fuse-v1-symlink-provider-model-cache-dir-outside");
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
    write_text_file(&outside.join("local.models.json"), r#"{"models":["leaked"]}"#);
    assert!(symlink(&outside, &cache).is_ok());
    let projection = FuseV1Projection::new(&root)
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
    assert_eq!(projection.getattr("model/local/leaked"), Err(FuseV1Error::NotFound));
}

#[test]
fn fuse_v1_projection_ignores_oversized_provider_model_cache() {
    let root = reference_tree("fuse-v1-oversized-provider-model-cache");
    let providers = root.join("providers.d");
    let cache = root.join("provider-models");
    write_text_file(
        &providers.join("api.lmm.best.json"),
        r#"{
  "base_url": "https://api.lmm.best:9000/",
  "default_model": "gpt-5.4-mini",
  "enabled": true,
  "formats": ["openai.chat"]
}
"#,
    );
    let oversized_padding = "x".repeat(1024 * 1024);
    write_text_file(
        &cache.join("api.lmm.best.models.json"),
        &format!(r#"{{"models":["gpt-cache"],"padding":"{oversized_padding}"}}"#),
    );
    let projection = FuseV1Projection::new(&root)
        .with_provider_config_dir(&providers)
        .with_provider_model_cache_dir(&cache);

    let provider_entries = projection.readdir("model/api.lmm.best");
    assert!(provider_entries.is_ok());
    let provider_names = provider_entries
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.name().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(provider_names, ["gpt-5.4-mini", "gpt-5.4-mini.d"]);
}

#[test]
fn fuse_v1_projection_skips_disabled_provider_models() {
    let root = reference_tree("fuse-v1-disabled-provider-model");
    let providers = root.join("providers.d");
    write_text_file(
        &providers.join("api.lmm.best.json"),
        r#"{
  "base_url": "https://api.lmm.best:9000/",
  "default_model": "gpt-5.4-mini",
  "enabled": false,
  "formats": ["openai.chat"]
}
"#,
    );
    let projection = FuseV1Projection::new(&root).with_provider_config_dir(&providers);

    let model_entries = projection.readdir("model");
    assert!(model_entries.is_ok());
    let model_names = model_entries
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.name().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(model_names, ["debug", "helper", "main", "route"]);
    assert_eq!(
        projection.getattr("model/api.lmm.best"),
        Err(FuseV1Error::NotFound)
    );
}
