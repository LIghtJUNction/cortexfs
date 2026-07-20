#[test]
fn provider_transport_defaults_to_direct_base_url() {
    assert_eq!(
        provider_transport(&test_provider_config("https://api.example.test/v1"), None),
        Ok(ResolvedTransport::Direct {
            base_url: "https://api.example.test/v1".to_owned()
        })
    );
}

#[test]
fn provider_config_from_dir_ignores_symlink_configs() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-provider-config-symlink")?;
    let providers = root.join("providers.d");
    let outside = root.join("outside.json");
    fs::create_dir_all(&providers)?;
    fs::write(
        &outside,
        r#"{
  "name": "fixture",
  "base_url": "http://203.0.113.10:8317/v1",
  "formats": ["openai.chat"]
}
"#,
    )?;
    symlink(&outside, providers.join("fixture.json"))?;

    assert!(provider_config_from_dir(&providers, "fixture").is_none());

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn provider_config_file_reader_refuses_symlink_leaf() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-provider-config-file-symlink")?;
    let providers = root.join("providers.d");
    let outside = root.join("outside.json");
    fs::create_dir_all(&providers)?;
    fs::write(&outside, "{}\n")?;
    symlink(&outside, providers.join("fixture.json"))?;
    let directory = open_plain_directory(&providers)?;

    assert!(
        cortexfs::support::plain::read_small_text_file_at(
            &directory,
            "fixture.json",
            MAX_RUNNER_PROVIDER_CONFIG_BYTES,
            "provider config file is invalid"
        )
        .is_err()
    );

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn provider_config_from_dir_rejects_symlink_config_directory()
-> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-provider-config-dir-symlink")?;
    let providers = root.join("providers.d");
    let outside = root.join("outside-providers.d");
    fs::create_dir_all(&outside)?;
    fs::write(
        outside.join("fixture.json"),
        r#"{
  "name": "fixture",
  "base_url": "http://203.0.113.10:8317/v1",
  "formats": ["openai.chat"]
}
"#,
    )?;
    symlink(&outside, &providers)?;

    assert!(provider_config_from_dir(&providers, "fixture").is_none());

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn provider_config_from_dir_reads_plain_json_configs() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-provider-config-plain")?;
    let providers = root.join("providers.d");
    fs::create_dir_all(&providers)?;
    fs::write(
        providers.join("fixture.json"),
        r#"{
  "name": "fixture",
  "base_url": "http://203.0.113.10:8317/v1",
  "formats": ["openai.responses"]
}
"#,
    )?;

    let config = provider_config_from_dir(&providers, "fixture");

    assert!(matches!(
        config,
        Some(ref config)
            if config.base_url == "http://203.0.113.10:8317/v1"
                && config.formats == ["openai.responses"]
    ));
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn provider_config_from_dir_rejects_duplicate_known_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-provider-config-duplicate")?;
    let providers = root.join("providers.d");
    fs::create_dir_all(&providers)?;
    fs::write(
        providers.join("fixture.json"),
        "{\"name\":\"fixture\",\"base_url\":\"http://203.0.113.10:8317/v1\",\"base_url\":\"http://evil/v1\"}\n",
    )?;

    assert!(provider_config_from_dir(&providers, "fixture").is_none());
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn provider_runner_uses_absolute_curl_path() {
    let command = provider_curl_command();
    assert_eq!(command.get_program(), PROVIDER_CURL_BIN);
    assert_eq!(
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        ["-q", "--config", "-"]
    );
    assert!(command.get_envs().next().is_none());
}

#[test]
fn runtime_provider_secret_reader_refuses_symlink_targets() -> Result<(), Box<dyn std::error::Error>>
{
    let root = unique_temp_dir("runner-runtime-secret-symlink")?;
    fs::create_dir_all(&root)?;
    let target = root.join("target");
    let link = root.join("secret");
    fs::write(&target, "runtime-secret\n")?;
    symlink(&target, &link)?;

    assert!(read_runtime_provider_secret_file(&link).is_err());

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn runtime_provider_secret_reader_refuses_symlink_intermediate_dir()
-> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-runtime-secret-symlink-intermediate")?;
    let outside = root.join("outside");
    fs::create_dir_all(&outside)?;
    fs::write(outside.join("secret"), "runtime-secret\n")?;
    symlink(&outside, root.join("runtime"))?;

    assert!(read_runtime_provider_secret_file(&root.join("runtime/secret")).is_err());

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn provider_secret_from_runtime_file_reads_only_matching_plain_file()
-> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-runtime-secret-plain")?;
    fs::create_dir_all(&root)?;
    let path = root.join("secret");
    fs::write(&path, "runtime-secret\n")?;
    let path_text = path.display().to_string();
    let env = |name: &str| match name {
        "CTX_PROVIDER_SECRET_PROVIDER" => Ok("fixture".to_owned()),
        "CTX_PROVIDER_SECRET_SLOT" => Ok("default".to_owned()),
        "CTX_PROVIDER_SECRET_PATH" => Ok(path_text.clone()),
        _ => Err(std::env::VarError::NotPresent),
    };

    assert_eq!(
        provider_secret_from_runtime_file_with_env("fixture", "default", env)?,
        Some("runtime-secret".to_owned())
    );
    assert_eq!(
        provider_secret_from_runtime_file_with_env("other", "default", env)?,
        None
    );

    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn provider_secret_from_runtime_value_reads_only_matching_slot() {
    let env = |name: &str| match name {
        "CTX_PROVIDER_SECRET_PROVIDER" => Ok("fixture".to_owned()),
        "CTX_PROVIDER_SECRET_SLOT" => Ok("default".to_owned()),
        "CTX_PROVIDER_SECRET_VALUE" => Ok("runtime-secret\n".to_owned()),
        _ => Err(std::env::VarError::NotPresent),
    };

    assert_eq!(
        provider_secret_from_runtime_value_with_env("fixture", "default", env),
        Some("runtime-secret".to_owned())
    );
    assert_eq!(
        provider_secret_from_runtime_value_with_env("other", "default", env),
        None
    );
}

#[test]
fn provider_secret_from_runtime_file_ignores_relative_path()
-> Result<(), Box<dyn std::error::Error>> {
    let env = |name: &str| match name {
        "CTX_PROVIDER_SECRET_PROVIDER" => Ok("fixture".to_owned()),
        "CTX_PROVIDER_SECRET_SLOT" => Ok("default".to_owned()),
        "CTX_PROVIDER_SECRET_PATH" => Ok("relative-secret".to_owned()),
        _ => Err(std::env::VarError::NotPresent),
    };

    assert_eq!(
        provider_secret_from_runtime_file_with_env("fixture", "default", env)?,
        None
    );
    Ok(())
}

#[test]
fn provider_secret_from_inherited_fd_reads_regular_secret_file()
-> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("runner-inherited-secret-fd")?;
    let path = root.join("secret");
    fs::write(&path, "fd-secret\n")?;
    let file = fs::File::open(&path)?;
    let fd = file.as_raw_fd().to_string();
    let env = |name: &str| match name {
        "CTX_PROVIDER_SECRET_PROVIDER" => Ok("fixture".to_owned()),
        "CTX_PROVIDER_SECRET_SLOT" => Ok("default".to_owned()),
        "CTX_PROVIDER_SECRET_FD" => Ok(fd.clone()),
        _ => Err(std::env::VarError::NotPresent),
    };

    let secret = provider_secret_from_inherited_fd_with_env("fixture", "default", env)?;

    assert_eq!(secret, Some("fd-secret".to_owned()));
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn provider_secret_from_inherited_fd_ignores_standard_fds() -> Result<(), Box<dyn std::error::Error>>
{
    for fd in ["0", "1", "2"] {
        let env = |name: &str| match name {
            "CTX_PROVIDER_SECRET_PROVIDER" => Ok("fixture".to_owned()),
            "CTX_PROVIDER_SECRET_SLOT" => Ok("default".to_owned()),
            "CTX_PROVIDER_SECRET_FD" => Ok(fd.to_owned()),
            _ => Err(std::env::VarError::NotPresent),
        };

        assert_eq!(
            provider_secret_from_inherited_fd_with_env("fixture", "default", env)?,
            None
        );
    }
    Ok(())
}

#[test]
fn provider_secret_from_inherited_fd_rejects_non_regular_fd()
-> Result<(), Box<dyn std::error::Error>> {
    let (read_fd, write_fd) = nix::unistd::pipe()?;
    let fd = read_fd.as_raw_fd().to_string();
    let env = |name: &str| match name {
        "CTX_PROVIDER_SECRET_PROVIDER" => Ok("fixture".to_owned()),
        "CTX_PROVIDER_SECRET_SLOT" => Ok("default".to_owned()),
        "CTX_PROVIDER_SECRET_FD" => Ok(fd.clone()),
        _ => Err(std::env::VarError::NotPresent),
    };

    let result = provider_secret_from_inherited_fd_with_env("fixture", "default", env);

    assert!(matches!(result, Err(ref error) if error.kind() == std::io::ErrorKind::InvalidData));
    drop(read_fd);
    drop(write_fd);
    Ok(())
}

#[test]
fn provider_transport_uses_exact_http_route() {
    let config = test_provider_config("https://api.example.test/v1");

    assert_eq!(
        provider_transport(
            &config,
            Some(
                "group(office) -> http(http://127.0.0.1:8080/v1)\ndomain(example.test) -> office\n"
            )
        ),
        Ok(ResolvedTransport::Http {
            base_url: "http://127.0.0.1:8080/v1".to_owned()
        })
    );
}

#[test]
fn provider_egress_converts_only_direct_transport() {
    let transport = ResolvedTransport::Direct {
        base_url: "https://api.example.test/v1".to_owned(),
    };

    assert_eq!(
        provider_egress_transport(
            "fixture",
            transport,
            Some(OsStr::new(
                cortexfs::runtime::egress::PROVIDER_EGRESS_SANDBOX_PATH
            ))
        ),
        Ok(ResolvedTransport::Unix {
            base_url: "http://localhost/v1".to_owned(),
            socket_path: "/run/cortexfs/provider-egress/fixture.sock".to_owned(),
        })
    );
}

#[test]
fn provider_egress_preserves_explicit_http_transport() {
    let transport = ResolvedTransport::Http {
        base_url: "http://127.0.0.1:8080/v1".to_owned(),
    };

    assert_eq!(
        provider_egress_transport(
            "fixture",
            transport.clone(),
            Some(OsStr::new(
                cortexfs::runtime::egress::PROVIDER_EGRESS_SANDBOX_PATH
            ))
        ),
        Ok(transport)
    );
}

#[test]
fn provider_egress_preserves_explicit_unix_transport() {
    let transport = ResolvedTransport::Unix {
        base_url: "http://localhost/v1".to_owned(),
        socket_path: "/run/fixture.sock".to_owned(),
    };

    assert_eq!(
        provider_egress_transport(
            "fixture",
            transport.clone(),
            Some(OsStr::new(
                cortexfs::runtime::egress::PROVIDER_EGRESS_SANDBOX_PATH
            ))
        ),
        Ok(transport)
    );
}

#[test]
fn provider_egress_rejects_environment_drift() {
    let transport = ResolvedTransport::Direct {
        base_url: "https://api.example.test/v1".to_owned(),
    };

    assert_eq!(
        provider_egress_transport(
            "fixture",
            transport,
            Some(OsStr::new("/run/attacker-controlled"))
        ),
        Err("invalid provider egress directory".to_owned())
    );
}

#[test]
fn provider_transport_uses_wildcard_unix_route() {
    let config = test_provider_config("https://api.example.test/v1");

    assert_eq!(
        provider_transport(
            &config,
            Some(
                "group(unix-egress) -> unix(/run/user/1000/cortexfs/proxy/fixture.sock)\ndomain(example.test) -> unix-egress\n"
            )
        ),
        Ok(ResolvedTransport::Unix {
            base_url: "http://localhost/v1".to_owned(),
            socket_path: "/run/user/1000/cortexfs/proxy/fixture.sock".to_owned()
        })
    );
}

#[test]
fn provider_transport_rejects_relative_unix_socket_route() {
    let config = test_provider_config("https://api.example.test/v1");

    assert!(
        provider_transport(
            &config,
            Some(
                "group(unix-egress) -> unix(relative.sock)\ndomain(example.test) -> unix-egress\n"
            )
        )
        .is_err()
    );
}

#[test]
fn provider_transport_rejects_control_character_unix_socket_route() {
    let config = test_provider_config("https://api.example.test/v1");

    assert!(provider_transport(
        &config,
        Some("group(unix-egress) -> unix(/run/cortexfs/\u{1b}proxy.sock)\ndomain(example.test) -> unix-egress\n")
    )
    .is_err());
}

#[test]
fn provider_transport_rejects_non_url_unix_base_url() {
    let config = test_provider_config("https://api.example.test/v1");

    assert!(provider_transport(
        &config,
        Some("group(unix-egress) -> unix(/run/cortexfs/proxy.sock, not-a-url)\ndomain(example.test) -> unix-egress\n")
    )
    .is_err());
}

#[test]
fn curl_config_quote_rejects_line_break_injection() {
    assert!(curl_config_quote("https://api.example.test/v1").is_ok());
    assert!(curl_config_quote("https://api.example.test/v1\noutput = /tmp/leak").is_err());
    assert!(curl_config_quote("Authorization: Bearer bad\rheader = injected").is_err());
    assert!(curl_config_quote("Authorization: Bearer \u{1b}]52;c;payload").is_err());
    assert!(curl_config_quote("abc\0def").is_err());
}

#[test]
fn provider_json_body_escapes_prompt_newlines_before_curl_config() {
    let body = openai_chat_body(
        "gpt-5.4",
        "first line\nsecond line",
        false,
        cortexfs::ModelEffort::Auto,
    );

    assert!(!body.contains('\n'));
    assert!(body.contains("\\n"));
    assert!(curl_config_quote(&body).is_ok());
}

#[test]
fn provider_route_selects_key_slot_by_model() {
    let config = test_provider_config("https://api.example.test/v1");

    assert_eq!(
        provider_route(
            &config,
            "fixture",
            "gpt-5.4",
            Some("group(paid) -> direct, key(office)\nmodel(gpt-*) -> paid\nfallback: direct\n")
        ),
        Ok(ProviderRoute {
            transport: ResolvedTransport::Direct {
                base_url: "https://api.example.test/v1".to_owned()
            },
            key_slot: Some("office".to_owned())
        })
    );
}

#[test]
fn anthropic_message_content_parses_text_parts() {
    assert_eq!(
        parse_anthropic_message_content(
            br#"{"content":[{"type":"text","text":"hello "},{"type":"text","text":"claude"}]}"#
        ),
        Ok("hello claude".to_owned())
    );
}

fn test_provider_config(base_url: &str) -> RunnerProviderConfig {
    test_provider_config_with_formats(base_url, &[])
}

pub(super) fn test_provider_config_with_formats(
    base_url: &str,
    formats: &[&str],
) -> RunnerProviderConfig {
    RunnerProviderConfig {
        name: None,
        base_url: base_url.to_owned(),
        oauth: None,
        formats: formats.iter().map(|format| (*format).to_owned()).collect(),
    }
}

#[test]
fn provider_targets_share_effective_v1_base_normalization() {
    for (base, expected) in [
        ("http://localhost", "http://localhost/v1/responses"),
        ("http://localhost/v1", "http://localhost/v1/responses"),
        (
            "http://localhost/custom",
            "http://localhost/custom/v1/responses",
        ),
        (
            "http://localhost/custom/v1",
            "http://localhost/custom/v1/responses",
        ),
    ] {
        assert_eq!(
            provider_target(
                &ResolvedTransport::Direct {
                    base_url: base.into()
                },
                "responses",
            )
            .url,
            expected
        );
    }
}

#[test]
fn responses_function_call_becomes_canonical_tool_call() {
    let output = serde_json::json!({"output":[{"type":"function_call","call_id":"call_123","name":"tsh","arguments":"{\"args\":[\"tools\"]}"}]}).to_string();
    let value: serde_json::Value =
        serde_json::from_str(&parse_openai_response_content(output.as_bytes()).unwrap_or_default())
            .unwrap_or_default();
    assert_eq!(
        value.pointer("/arguments/args/0"),
        Some(&serde_json::json!("tools"))
    );
}

#[test]
fn provider_tool_call_parser_rejects_undeclarable_function_names() {
    let chat = serde_json::json!({"choices":[{"message":{"tool_calls":[{"id":"call_1","function":{"name":"shell.exec","arguments":"{\"args\":[\"date\"]}"}}]}}]}).to_string();
    let responses = serde_json::json!({"output":[{"type":"function_call","call_id":"call_1","name":"shell.exec","arguments":"{\"args\":[\"date\"]}"}]}).to_string();
    for result in [
        parse_openai_chat_content(chat.as_bytes()),
        parse_openai_response_content(responses.as_bytes()),
    ] {
        assert_eq!(result, Err("provider response missing content".to_owned()));
    }
}
use super::*;
