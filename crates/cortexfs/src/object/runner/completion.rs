use super::*;
use std::io;
use std::net::IpAddr;
use std::path::PathBuf;

pub(crate) fn provider_runtime_driver(
    config: &RunnerProviderConfig,
    agent_call: bool,
) -> ProviderRuntimeDriver {
    if config
        .formats
        .iter()
        .any(|format| format.trim() == "anthropic.messages")
    {
        ProviderRuntimeDriver::AnthropicMessages
    } else if config
        .formats
        .iter()
        .any(|format| format.trim() == "openai.responses")
        && (agent_call
            || !config
                .formats
                .iter()
                .any(|format| format.trim() == "openai.chat"))
    {
        ProviderRuntimeDriver::OpenAiResponses
    } else {
        ProviderRuntimeDriver::OpenAiChat
    }
}
pub(crate) fn provider_chat_completion(
    name: &str,
    input: &str,
    run: &str,
    stdout: &mut impl Write,
) -> Result<(), ProviderCompletionError> {
    let (provider, model) = name.split_once('/').ok_or_else(|| {
        ProviderCompletionError::fallback(format!("invalid provider model: {name}"))
    })?;
    let ctx_root =
        env::var_os("CTX_ROOT").map_or_else(|| PathBuf::from(DEFAULT_CTX_ROOT), PathBuf::from);
    let config = provider_config(provider)
        .or_else(|| provider_config_from_model_control(&ctx_root, provider, model))
        .ok_or_else(|| {
            ProviderCompletionError::fallback(format!("missing provider: {provider}"))
        })?;
    let route = read_small_plain_text_file(
        &ctx_root.join("model").join("route"),
        MAX_RUNNER_CONTROL_BYTES,
        "runner",
    )
    .ok();
    let mut route = provider_route(&config, provider, model, route.as_deref())
        .map_err(ProviderCompletionError::fallback)?;
    let allow_unauthenticated = transport_allows_unauthenticated(&route.transport);
    route.transport = provider_egress_transport(
        provider,
        route.transport,
        env::var_os(cortexfs::runtime::egress::PROVIDER_EGRESS_DIR_ENV).as_deref(),
    )
    .map_err(ProviderCompletionError::fallback)?;
    let effort = model_effort(&ctx_root, provider, model);
    let agent_call = env::var_os("CTX_AGENT").is_some();
    let drivers = model_runtime_drivers(&ctx_root, provider, model, agent_call)
        .map_err(ProviderCompletionError::fallback)?
        .unwrap_or_else(|| vec![provider_runtime_driver(&config, agent_call)]);
    let mut last_error = None;
    for driver in drivers {
        match call_provider_driver(
            driver,
            provider,
            model,
            input,
            run,
            stdout,
            &config,
            &route,
            effort,
            allow_unauthenticated,
        ) {
            Ok(()) => return Ok(()),
            Err(error) if error.can_fallback => last_error = Some(error),
            Err(error) => return Err(error),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        ProviderCompletionError::fallback(format!("no supported model driver: {name}"))
    }))
}

pub(crate) fn provider_egress_transport(
    provider: &str,
    transport: ResolvedTransport,
    egress_dir: Option<&std::ffi::OsStr>,
) -> Result<ResolvedTransport, String> {
    let Some(egress_dir) = egress_dir else {
        return Ok(transport);
    };
    if egress_dir != std::ffi::OsStr::new(cortexfs::runtime::egress::PROVIDER_EGRESS_SANDBOX_PATH) {
        return Err("invalid provider egress directory".to_owned());
    }
    match transport {
        ResolvedTransport::Direct { base_url } => {
            if !cortexfs::is_object_name(provider) {
                return Err("invalid provider egress name".to_owned());
            }
            let trusted = reqwest::Url::parse(&base_url)
                .map_err(|_error| "invalid provider egress base URL".to_owned())?;
            let local_base_url = format!("http://localhost{}", trusted.path());
            Ok(ResolvedTransport::Unix {
                base_url: local_base_url,
                socket_path: format!(
                    "{}/{}.sock",
                    cortexfs::runtime::egress::PROVIDER_EGRESS_SANDBOX_PATH,
                    provider
                ),
            })
        }
        ResolvedTransport::Http { .. } | ResolvedTransport::Unix { .. } => Ok(transport),
    }
}

fn model_runtime_drivers(
    ctx_root: &Path,
    provider: &str,
    model: &str,
    agent_call: bool,
) -> Result<Option<Vec<ProviderRuntimeDriver>>, String> {
    let control = ctx_root
        .join("model")
        .join(provider)
        .join(format!("{model}.d"))
        .join("driver");
    let content = match read_small_plain_text_file(&control, MAX_RUNNER_CONTROL_BYTES, "runner") {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_error) => return Err("cannot read model driver control".to_owned()),
    };
    let routes = cortexfs::parse_model_driver_routes(&content).map_err(|error| {
        format!(
            "invalid model driver control: {}",
            crate::object::layout::model_driver_route_error_value(&error)
        )
    })?;
    let use_case = if agent_call {
        cortexfs::ModelDriverUseCase::Agent
    } else {
        cortexfs::ModelDriverUseCase::Exec
    };
    let drivers = routes.drivers_for(use_case).ok_or_else(|| {
        format!(
            "model driver control has no {} or default route",
            use_case.as_str()
        )
    })?;
    drivers
        .iter()
        .map(|driver| match driver.as_str() {
            "openai-chat" => Ok(ProviderRuntimeDriver::OpenAiChat),
            "openai-responses" => Ok(ProviderRuntimeDriver::OpenAiResponses),
            "anthropic-messages" => Ok(ProviderRuntimeDriver::AnthropicMessages),
            _ => Err(format!("unsupported model driver adapter: {driver}")),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

#[expect(
    clippy::too_many_arguments,
    reason = "narrow provider driver dispatch boundary"
)]
fn call_provider_driver(
    driver: ProviderRuntimeDriver,
    provider: &str,
    model: &str,
    input: &str,
    run: &str,
    stdout: &mut impl Write,
    config: &RunnerProviderConfig,
    route: &ProviderRoute,
    effort: cortexfs::ModelEffort,
    allow_unauthenticated: bool,
) -> Result<(), ProviderCompletionError> {
    let credential = provider_credential(provider, config, route.key_slot.as_deref(), driver)
        .map_err(ProviderCompletionError::fallback)?;
    match driver {
        ProviderRuntimeDriver::OpenAiChat => {
            let key = openai_api_key(provider, allow_unauthenticated, credential.as_ref())
                .map_err(ProviderCompletionError::fallback)?;
            let request = OpenAiProviderRequest {
                model,
                input,
                api_key: key,
                effort,
            };
            stream_or_complete(
                call_openai_chat_streaming(&route.transport, &request, run, stdout),
                || call_openai_chat(&route.transport, &request),
                run,
                stdout,
            )
        }
        ProviderRuntimeDriver::OpenAiResponses => {
            let key = openai_api_key(provider, allow_unauthenticated, credential.as_ref())
                .map_err(ProviderCompletionError::fallback)?;
            let request = OpenAiProviderRequest {
                model,
                input,
                api_key: key,
                effort,
            };
            stream_or_complete(
                call_openai_responses_streaming(&route.transport, &request, run, stdout),
                || call_openai_responses(&route.transport, &request),
                run,
                stdout,
            )
        }
        ProviderRuntimeDriver::AnthropicMessages => {
            let credential = credential.ok_or_else(|| {
                ProviderCompletionError::fallback(format!(
                    "missing provider credential: {provider}"
                ))
            })?;
            let completion = call_anthropic_messages(&route.transport, model, input, &credential)
                .map_err(ProviderCompletionError::fallback)?;
            write_text_completion(stdout, run, &completion).map_err(|error| {
                ProviderCompletionError::no_fallback(format!("cannot write output: {error}"))
            })
        }
    }
}

fn stream_or_complete(
    stream_result: Result<(), StreamFailure>,
    complete: impl FnOnce() -> Result<ProviderTextCompletion, String>,
    run: &str,
    stdout: &mut impl Write,
) -> Result<(), ProviderCompletionError> {
    match stream_result {
        Ok(()) => Ok(()),
        Err(error) if error.can_fallback => {
            let completion = complete().map_err(ProviderCompletionError::fallback)?;
            write_text_completion(stdout, run, &completion).map_err(|error| {
                ProviderCompletionError::no_fallback(format!("cannot write output: {error}"))
            })
        }
        Err(error) => Err(ProviderCompletionError {
            message: error.message,
            can_fallback: error.can_fallback,
        }),
    }
}
pub(crate) fn openai_api_key<'a>(
    provider: &str,
    allow_unauthenticated: bool,
    credential: Option<&'a ProviderCredential>,
) -> Result<Option<&'a str>, String> {
    if let Some(credential) = credential {
        return Ok(Some(credential.secret()));
    }
    if allow_unauthenticated {
        return Ok(None);
    }
    Err(format!("missing provider credential: {provider}"))
}
pub(crate) fn transport_allows_unauthenticated(transport: &ResolvedTransport) -> bool {
    match *transport {
        ResolvedTransport::Unix { .. } => true,
        ResolvedTransport::Direct { ref base_url } | ResolvedTransport::Http { ref base_url } => {
            base_url_has_local_host(base_url)
        }
    }
}
pub(crate) fn base_url_has_local_host(base_url: &str) -> bool {
    let Some(host) = cortexfs::provider_host_from_base_url(base_url) else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<IpAddr>()
        .is_ok_and(|ip| ip_matches("geoip:private", &ip))
}
pub(crate) fn write_text_completion(
    stdout: &mut impl Write,
    run: &str,
    completion: &ProviderTextCompletion,
) -> io::Result<()> {
    write_model_text_or_tool_call(stdout, run, &completion.content)?;
    if let Some(usage) = completion.usage {
        write_model_usage(stdout, run, usage)?;
    }
    stdout.flush()
}
pub(crate) struct ProviderCompletionError {
    pub(crate) message: String,
    pub(crate) can_fallback: bool,
}
impl ProviderCompletionError {
    pub(crate) fn fallback(message: String) -> Self {
        Self {
            message,
            can_fallback: true,
        }
    }
    pub(crate) fn no_fallback(message: String) -> Self {
        Self {
            message,
            can_fallback: false,
        }
    }
}

#[cfg(test)]
mod driver_route_tests {
    use super::*;
    use std::fs;

    #[test]
    fn direct_exec_uses_exec_driver_route() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let control = root.path().join("model/fixture/model.d");
        fs::create_dir_all(&control)?;
        fs::write(
            control.join("driver"),
            "default=openai-responses\nexec=openai-chat\nagent=openai-responses,openai-chat\n",
        )?;

        assert_eq!(
            model_runtime_drivers(root.path(), "fixture", "model", false),
            Ok(Some(vec![ProviderRuntimeDriver::OpenAiChat]))
        );
        Ok(())
    }

    #[test]
    fn agent_route_uses_default_when_agent_route_is_missing()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let control = root.path().join("model/fixture/model.d");
        fs::create_dir_all(&control)?;
        fs::write(control.join("driver"), "default=openai-chat\n")?;

        assert_eq!(
            model_runtime_drivers(root.path(), "fixture", "model", true),
            Ok(Some(vec![ProviderRuntimeDriver::OpenAiChat]))
        );
        Ok(())
    }

    #[test]
    fn unknown_adapter_returns_stable_error_before_transport()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let control = root.path().join("model/fixture/model.d");
        fs::create_dir_all(&control)?;
        fs::write(control.join("driver"), "agent=vendor-magic\n")?;

        assert_eq!(
            model_runtime_drivers(root.path(), "fixture", "model", true),
            Err("unsupported model driver adapter: vendor-magic".to_owned())
        );
        Ok(())
    }

    #[test]
    fn format_names_are_not_driver_aliases() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let control = root.path().join("model/fixture/model.d");
        fs::create_dir_all(&control)?;
        fs::write(control.join("driver"), "agent=openai.responses\n")?;

        assert_eq!(
            model_runtime_drivers(root.path(), "fixture", "model", true),
            Err("unsupported model driver adapter: openai.responses".to_owned())
        );
        Ok(())
    }

    #[test]
    fn malformed_control_returns_stable_error() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let control = root.path().join("model/fixture/model.d");
        fs::create_dir_all(&control)?;
        fs::write(control.join("driver"), "agent=\n")?;

        assert_eq!(
            model_runtime_drivers(root.path(), "fixture", "model", true),
            Err("invalid model driver control: line 1 empty driver".to_owned())
        );
        Ok(())
    }

    #[test]
    fn unreadable_control_returns_stable_error_without_os_detail()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let control = root.path().join("model/fixture/model.d/driver");
        fs::create_dir_all(&control)?;

        assert_eq!(
            model_runtime_drivers(root.path(), "fixture", "model", true),
            Err("cannot read model driver control".to_owned())
        );
        Ok(())
    }
}
