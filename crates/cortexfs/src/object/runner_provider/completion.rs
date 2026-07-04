fn provider_runtime_driver(config: &RunnerProviderConfig, agent_call: bool) -> ProviderRuntimeDriver {
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
                .any(|format| matches!(format.trim(), "openai.chat" | "openai-compatible")))
    {
        ProviderRuntimeDriver::OpenAiResponses
    } else {
        ProviderRuntimeDriver::OpenAiChat
    }
}

fn provider_chat_completion(
    name: &str,
    input: &str,
    run: &str,
    stdout: &mut impl Write,
) -> Result<(), ProviderCompletionError> {
    let (provider, model) = name
        .split_once('/')
        .ok_or_else(|| ProviderCompletionError::fallback(format!("invalid provider model: {name}")))?;
    let ctx_root =
        env::var_os("CTX_ROOT").map_or_else(|| PathBuf::from(DEFAULT_CTX_ROOT), PathBuf::from);
    let config = provider_config(provider)
        .or_else(|| provider_config_from_model_control(&ctx_root, provider, model))
        .ok_or_else(|| ProviderCompletionError::fallback(format!("missing provider: {provider}")))?;
    let route = read_small_plain_text_file(&ctx_root.join("model").join("route")).ok();
    let route = provider_route(&config, provider, model, route.as_deref())
        .map_err(ProviderCompletionError::fallback)?;
    let effort = model_effort(&ctx_root, provider, model);
    let driver = provider_runtime_driver(&config, env::var_os("CTX_AGENT").is_some());
    let credential = provider_credential(provider, &config, route.key_slot.as_deref(), driver)
        .map_err(ProviderCompletionError::fallback)?;
    match driver {
        ProviderRuntimeDriver::OpenAiChat => {
            let key = openai_api_key(provider, &route.transport, credential.as_ref())
                .map_err(ProviderCompletionError::fallback)?;
            let request = OpenAiProviderRequest {
                model,
                input,
                api_key: key,
                effort,
            };
            match call_openai_chat_streaming(&route.transport, &request, run, stdout) {
                Ok(()) => Ok(()),
                Err(error) if error.can_fallback => {
                    let completion = call_openai_chat(&route.transport, &request)
                        .map_err(ProviderCompletionError::fallback)?;
                    write_text_completion(stdout, run, &completion).map_err(|error| {
                        ProviderCompletionError::no_fallback(format!(
                            "cannot write output: {error}"
                        ))
                    })
                }
                Err(error) => Err(ProviderCompletionError {
                    message: error.message,
                    can_fallback: error.can_fallback,
                }),
            }
        }
        ProviderRuntimeDriver::OpenAiResponses => {
            let key = openai_api_key(provider, &route.transport, credential.as_ref())
                .map_err(ProviderCompletionError::fallback)?;
            let request = OpenAiProviderRequest {
                model,
                input,
                api_key: key,
                effort,
            };
            match call_openai_responses_streaming(&route.transport, &request, run, stdout) {
                Ok(()) => Ok(()),
                Err(error) if error.can_fallback => {
                    let completion = call_openai_responses(&route.transport, &request)
                        .map_err(ProviderCompletionError::fallback)?;
                    write_text_completion(stdout, run, &completion).map_err(|error| {
                        ProviderCompletionError::no_fallback(format!(
                            "cannot write output: {error}"
                        ))
                    })
                }
                Err(error) => Err(ProviderCompletionError {
                    message: error.message,
                    can_fallback: error.can_fallback,
                }),
            }
        }
        ProviderRuntimeDriver::AnthropicMessages => {
            let credential = credential
                .ok_or_else(|| ProviderCompletionError::fallback(format!("missing provider credential: {provider}")))?;
            let completion = call_anthropic_messages(&route.transport, model, input, &credential)
                .map_err(ProviderCompletionError::fallback)?;
            write_text_completion(stdout, run, &completion)
                .map_err(|error| {
                    ProviderCompletionError::no_fallback(format!("cannot write output: {error}"))
                })
        }
    }
}

fn openai_api_key<'a>(
    provider: &str,
    transport: &ResolvedTransport,
    credential: Option<&'a ProviderCredential>,
) -> Result<Option<&'a str>, String> {
    if let Some(credential) = credential {
        return Ok(Some(credential.secret()));
    }
    if transport_allows_unauthenticated(transport) {
        return Ok(None);
    }
    Err(format!("missing provider credential: {provider}"))
}

fn transport_allows_unauthenticated(transport: &ResolvedTransport) -> bool {
    match *transport {
        ResolvedTransport::Unix { .. } => true,
        ResolvedTransport::Direct { ref base_url } | ResolvedTransport::Http { ref base_url } => {
            base_url_has_local_host(base_url)
        }
    }
}

fn base_url_has_local_host(base_url: &str) -> bool {
    let Some(host) = cortexfs::provider_host_from_base_url(base_url) else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<IpAddr>()
        .is_ok_and(|ip| ip_matches("geoip:private", &ip))
}

fn write_text_completion(
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

struct ProviderCompletionError {
    message: String,
    can_fallback: bool,
}

impl ProviderCompletionError {
    fn fallback(message: String) -> Self {
        Self {
            message,
            can_fallback: true,
        }
    }

    fn no_fallback(message: String) -> Self {
        Self {
            message,
            can_fallback: false,
        }
    }
}
