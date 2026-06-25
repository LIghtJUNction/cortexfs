use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt::Write as FmtWrite;

use serde_json::json;

#[derive(Clone, Debug, Deserialize)]
struct RunnerProviderConfig {
    base_url: String,
    api_key_env: Option<String>,
    oauth: Option<cortexfs::OAuthProviderConfig>,
    #[serde(default)]
    transports: BTreeMap<String, RunnerTransportConfig>,
    #[serde(default)]
    route: Vec<RunnerModelRoute>,
    default_transport: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct RunnerTransportConfig {
    kind: String,
    url: Option<String>,
    path: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct RunnerModelRoute {
    model: String,
    transport: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ResolvedTransport {
    Direct {
        base_url: String,
    },
    Http {
        base_url: String,
    },
    Unix {
        base_url: String,
        socket_path: String,
    },
}

struct CurlJsonTarget {
    url: String,
    unix_socket: Option<String>,
}

fn provider_chat_completion(
    name: &str,
    input: &str,
    run: &str,
    stdout: &mut impl Write,
) -> Result<(), String> {
    let (provider, model) = name
        .split_once('/')
        .ok_or_else(|| format!("invalid provider model: {name}"))?;
    let config =
        provider_config(provider).ok_or_else(|| format!("missing provider: {provider}"))?;
    let key = provider_bearer_token(&config)?
        .ok_or_else(|| format!("missing provider credential: {provider}"))?;
    let transport = provider_transport(&config, model)?;
    match call_openai_chat_streaming(&transport, model, input, &key, run, stdout) {
        Ok(()) => Ok(()),
        Err(error) if error.can_fallback => {
            let content = call_openai_chat(&transport, model, input, &key)?;
            write_model_delta(stdout, run, &content)
                .and_then(|()| stdout.flush())
                .map_err(|error| format!("cannot write output: {error}"))
        }
        Err(error) => Err(error.message),
    }
}

fn provider_config(provider: &str) -> Option<RunnerProviderConfig> {
    let entries = fs::read_dir("/etc/cortexfs/providers.d").ok()?;
    for entry in entries.flatten() {
        let content = fs::read_to_string(entry.path()).ok()?;
        let value = serde_json::from_str::<Value>(&content).ok()?;
        let base_url = value.get("base_url")?.as_str()?.to_owned();
        if provider_name_from_base_url(&base_url).as_deref() != Some(provider) {
            continue;
        }
        let mut config = serde_json::from_value::<RunnerProviderConfig>(value).ok()?;
        config.base_url = base_url;
        return Some(config);
    }
    None
}

fn provider_transport(
    config: &RunnerProviderConfig,
    model: &str,
) -> Result<ResolvedTransport, String> {
    let selected = config
        .route
        .iter()
        .find(|route| model_route_matches(&route.model, model))
        .map(|route| route.transport.as_str())
        .or(config.default_transport.as_deref());
    let Some(name) = selected else {
        return Ok(ResolvedTransport::Direct {
            base_url: config.base_url.clone(),
        });
    };
    let transport = config
        .transports
        .get(name)
        .ok_or_else(|| format!("missing provider transport: {name}"))?;
    match transport.kind.as_str() {
        "direct" => Ok(ResolvedTransport::Direct {
            base_url: transport
                .url
                .clone()
                .unwrap_or_else(|| config.base_url.clone()),
        }),
        "http" => {
            let base_url = transport
                .url
                .clone()
                .ok_or_else(|| format!("provider transport {name} missing url"))?;
            Ok(ResolvedTransport::Http { base_url })
        }
        "unix" => {
            let socket_path = transport
                .path
                .clone()
                .ok_or_else(|| format!("provider transport {name} missing path"))?;
            let base_url = transport
                .url
                .clone()
                .unwrap_or_else(|| "http://localhost/v1".to_owned());
            Ok(ResolvedTransport::Unix {
                base_url,
                socket_path,
            })
        }
        kind => Err(format!("unsupported provider transport kind: {kind}")),
    }
}

fn model_route_matches(pattern: &str, model: &str) -> bool {
    if matches!(pattern, "*" | "") {
        return true;
    }
    pattern == model
        || pattern
            .strip_suffix('*')
            .is_some_and(|prefix| model.starts_with(prefix))
}

fn provider_bearer_token(config: &RunnerProviderConfig) -> Result<Option<String>, String> {
    let Some(provider) = provider_name_from_base_url(&config.base_url) else {
        return Ok(None);
    };
    let api_key = resolve_api_key_from_env_names(
        &provider_key_names(config),
        &provider_keychain_service(&provider),
        "default",
    )
    .map_err(|_error| format!("keychain unavailable: {provider}"))?;
    if api_key.is_some() {
        return Ok(api_key);
    }
    let Some(oauth) = config.oauth.as_ref() else {
        return Ok(None);
    };
    cortexfs::resolve_oauth_access_token(&provider, oauth)
        .map_err(|_error| format!("oauth credential unavailable: {provider}"))
}

fn provider_key_names(config: &RunnerProviderConfig) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(name) = config.api_key_env.as_deref() {
        append_provider_key_name(name, &mut names);
    }
    if let Some(host) = provider_name_from_base_url(&config.base_url) {
        append_provider_key_name_for_host(&host, true, &mut names);
        append_provider_key_name_for_host(&host, false, &mut names);
    }
    names
}

fn append_provider_key_name_for_host(host: &str, drop_api_prefix: bool, names: &mut Vec<String>) {
    let labels = host
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let labels = if drop_api_prefix && labels.first() == Some(&"api") {
        labels.get(1..).unwrap_or_default()
    } else {
        labels.as_slice()
    };
    if labels.is_empty() {
        return;
    }
    append_provider_key_name(
        &(labels
            .iter()
            .map(|part| part.to_ascii_uppercase())
            .collect::<Vec<_>>()
            .join("_")
            + "_API_KEY"),
        names,
    );
}

fn append_provider_key_name(name: &str, names: &mut Vec<String>) {
    if is_provider_key_name(name) && !names.contains(&name.to_owned()) {
        names.push(name.to_owned());
    }
}

fn is_provider_key_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte == b'_' || byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

fn provider_keychain_service(provider: &str) -> String {
    format!("cortexfs:{provider}")
}

fn provider_name_from_base_url(base_url: &str) -> Option<String> {
    let host = base_url
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split(['/', ':'])
        .next()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

fn call_openai_chat(
    transport: &ResolvedTransport,
    model: &str,
    input: &str,
    api_key: &str,
) -> Result<String, String> {
    let target = chat_completions_target(transport);
    let body = json!({
        "model": model,
        "messages": provider_messages(input),
        "stream": false
    })
    .to_string();
    let output = run_curl_json(&target, api_key, &body)?;
    parse_openai_chat_content(&output)
}

struct StreamFailure {
    message: String,
    can_fallback: bool,
}

fn call_openai_chat_streaming(
    transport: &ResolvedTransport,
    model: &str,
    input: &str,
    api_key: &str,
    run: &str,
    stdout: &mut impl Write,
) -> Result<(), StreamFailure> {
    let target = chat_completions_target(transport);
    let body = json!({
        "model": model,
        "messages": provider_messages(input),
        "stream": true
    })
    .to_string();
    let mut child = start_curl_json(&target, api_key, &body).map_err(|message| StreamFailure {
        message,
        can_fallback: true,
    })?;
    let Some(child_stdout) = child.stdout.take() else {
        cleanup_curl_child(&mut child);
        return Err(StreamFailure {
            message: "cannot read provider stream".to_owned(),
            can_fallback: true,
        });
    };
    let mut emitted = false;
    let mut done = false;
    for line in BufReader::new(child_stdout).lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                cleanup_curl_child(&mut child);
                return Err(StreamFailure {
                    message: format!("cannot read provider stream: {error}"),
                    can_fallback: !emitted,
                });
            }
        };
        match openai_stream_event(&line) {
            Ok(OpenAiStreamEvent::Delta(text)) if !text.is_empty() => {
                if let Err(error) = write_model_delta(stdout, run, &text)
                    .and_then(|()| stdout.flush())
                {
                    cleanup_curl_child(&mut child);
                    return Err(StreamFailure {
                        message: format!("cannot write output: {error}"),
                        can_fallback: false,
                    });
                }
                emitted = true;
            }
            Ok(OpenAiStreamEvent::Delta(_empty)) => {}
            Ok(OpenAiStreamEvent::Done) => done = true,
            Ok(OpenAiStreamEvent::Ignore) => {}
            Err(message) => {
                cleanup_curl_child(&mut child);
                return Err(StreamFailure {
                    message,
                    can_fallback: !emitted,
                });
            }
        }
    }
    let status = child.wait().map_err(|error| StreamFailure {
        message: format!("cannot run curl: {error}"),
        can_fallback: !emitted,
    })?;
    if !status.success() {
        return Err(StreamFailure {
            message: "provider stream request failed".to_owned(),
            can_fallback: !emitted,
        });
    }
    if emitted {
        Ok(())
    } else {
        Err(StreamFailure {
            message: if done {
                "provider stream produced no answer text".to_owned()
            } else {
                "provider stream produced no content".to_owned()
            },
            can_fallback: true,
        })
    }
}

fn provider_messages(input: &str) -> Value {
    let agent = env::var("CTX_AGENT")
        .ok()
        .filter(|value| cortexfs::is_object_name(value));
    let agent_system = env::var("CTX_AGENT_SYSTEM").unwrap_or_default();
    let prompt_context = cortexfs::AgentPromptContext::from_env();
    provider_messages_for_agent(input, agent.as_deref(), &agent_system, &prompt_context)
}

fn provider_messages_for_agent(
    input: &str,
    agent: Option<&str>,
    agent_system: &str,
    prompt_context: &cortexfs::AgentPromptContext,
) -> Value {
    agent.map_or_else(
        || json!([{"role": "user", "content": input}]),
        |agent| json!([
            {
                "role": "system",
                "content": cortexfs::render_agent_system_prompt(agent, agent_system, prompt_context)
            },
            {"role": "user", "content": input}
        ]),
    )
}

fn run_curl_json(target: &CurlJsonTarget, api_key: &str, body: &str) -> Result<Vec<u8>, String> {
    let output = start_curl_json(target, api_key, body)?
        .wait_with_output()
        .map_err(|error| format!("cannot run curl: {error}"))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err("provider request failed".to_owned())
    }
}

fn start_curl_json(
    target: &CurlJsonTarget,
    api_key: &str,
    body: &str,
) -> Result<std::process::Child, String> {
    let mut child = Command::new("curl")
        .arg("--config")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("cannot start curl: {error}"))?;
    let Some(mut stdin) = child.stdin.take() else {
        cleanup_curl_child(&mut child);
        return Err("cannot write curl config".to_owned());
    };
    let mut config = format!(
        "fail\nsilent\nshow-error\nno-buffer\nmax-time = 60\nrequest = POST\nurl = {}\n",
        curl_config_quote(&target.url),
    );
    if let Some(socket_path) = target.unix_socket.as_deref() {
        let _ignored = writeln!(config, "unix-socket = {}", curl_config_quote(socket_path));
    }
    let _ignored = write!(
        config,
        "header = {}\nheader = {}\ndata = {}\n",
        curl_config_quote(&format!("Authorization: Bearer {api_key}")),
        curl_config_quote("Content-Type: application/json"),
        curl_config_quote(body),
    );
    if let Err(error) = stdin.write_all(config.as_bytes()) {
        cleanup_curl_child(&mut child);
        return Err(format!("cannot write curl config: {error}"));
    }
    drop(stdin);
    Ok(child)
}

fn cleanup_curl_child(child: &mut std::process::Child) {
    let _ignored = child.kill();
    let _ignored = child.wait();
}

fn parse_openai_chat_content(output: &[u8]) -> Result<String, String> {
    let value = serde_json::from_slice::<Value>(output)
        .map_err(|error| format!("invalid provider json: {error}"))?;
    value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .or_else(|| value.get("output_text").and_then(Value::as_str))
        .map(str::to_owned)
        .ok_or_else(|| "provider response missing content".to_owned())
}

enum OpenAiStreamEvent {
    Delta(String),
    Done,
    Ignore,
}

fn openai_stream_event(line: &str) -> Result<OpenAiStreamEvent, String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with(':') || !line.starts_with("data:") {
        return Ok(OpenAiStreamEvent::Ignore);
    }
    let data = line.trim_start_matches("data:").trim();
    if data == "[DONE]" {
        return Ok(OpenAiStreamEvent::Done);
    }
    let value = serde_json::from_str::<Value>(data)
        .map_err(|error| format!("invalid provider stream json: {error}"))?;
    let text = value
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
        .or_else(|| value.get("output_text").and_then(Value::as_str))
        .unwrap_or_default();
    Ok(OpenAiStreamEvent::Delta(text.to_owned()))
}

fn chat_completions_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.rsplit('/').next() == Some("v1") {
        format!("{base}/chat/completions")
    } else {
        format!("{base}/v1/chat/completions")
    }
}

fn chat_completions_target(transport: &ResolvedTransport) -> CurlJsonTarget {
    let (base_url, unix_socket) = match *transport {
        ResolvedTransport::Direct { ref base_url } | ResolvedTransport::Http { ref base_url } => {
            (base_url, None)
        }
        ResolvedTransport::Unix {
            ref base_url,
            ref socket_path,
        } => (base_url, Some(socket_path.clone())),
    };
    CurlJsonTarget {
        url: chat_completions_url(base_url),
        unix_socket,
    }
}

fn curl_config_quote(value: &str) -> String {
    let mut quoted = String::from("\"");
    for character in value.chars() {
        if matches!(character, '"' | '\\') {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');
    quoted
}
