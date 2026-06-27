use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt::Write as FmtWrite;
use std::net::IpAddr;

use serde_json::json;

#[derive(Clone, Debug, Deserialize)]
struct RunnerProviderConfig {
    base_url: String,
    oauth: Option<cortexfs::OAuthProviderConfig>,
    #[serde(default)]
    formats: Vec<String>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProviderRuntimeDriver {
    OpenAiChat,
    OpenAiResponses,
    AnthropicMessages,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ProviderCredential {
    Bearer(String),
    AnthropicApiKey(String),
}

impl ProviderCredential {
    fn secret(&self) -> &str {
        match self {
            &Self::Bearer(ref secret) | &Self::AnthropicApiKey(ref secret) => secret,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TokenUsage {
    input_tokens: u64,
    output_tokens: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProviderTextCompletion {
    content: String,
    usage: Option<TokenUsage>,
}

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
    let config =
        provider_config(provider).ok_or_else(|| ProviderCompletionError::fallback(format!("missing provider: {provider}")))?;
    let ctx_root =
        env::var_os("CTX_ROOT").map_or_else(|| PathBuf::from(DEFAULT_CTX_ROOT), PathBuf::from);
    let route = fs::read_to_string(ctx_root.join("model").join("route")).ok();
    let route = provider_route(&config, provider, model, route.as_deref())
        .map_err(ProviderCompletionError::fallback)?;
    let effort = model_effort(&ctx_root, provider, model);
    let driver = provider_runtime_driver(&config, env::var_os("CTX_AGENT").is_some());
    let credential = provider_credential(provider, &config, route.key_slot.as_deref(), driver)
        .map_err(ProviderCompletionError::fallback)?;
    match driver {
        ProviderRuntimeDriver::OpenAiChat => {
            let key = credential.as_ref().map(ProviderCredential::secret);
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
            let key = credential.as_ref().map(ProviderCredential::secret);
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

fn provider_config(provider: &str) -> Option<RunnerProviderConfig> {
    let entries = fs::read_dir("/etc/cortexfs/providers.d").ok()?;
    for entry in entries.flatten() {
        let content = fs::read_to_string(entry.path()).ok()?;
        let value = serde_json::from_str::<Value>(&content).ok()?;
        let base_url = value.get("base_url")?.as_str()?.to_owned();
        if provider_name_from_config(&base_url, value.get("name").and_then(Value::as_str))
            .as_deref()
            != Ok(provider)
        {
            continue;
        }
        let mut config = serde_json::from_value::<RunnerProviderConfig>(value).ok()?;
        config.base_url = base_url;
        return Some(config);
    }
    None
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProviderRoute {
    transport: ResolvedTransport,
    key_slot: Option<String>,
}

fn provider_route(
    config: &RunnerProviderConfig,
    provider: &str,
    model: &str,
    route_text: Option<&str>,
) -> Result<ProviderRoute, String> {
    let Some(route_text) = route_text else {
        return Ok(ProviderRoute {
            transport: ResolvedTransport::Direct {
                base_url: config.base_url.clone(),
            },
            key_slot: None,
        });
    };
    let table = parse_model_transport_route_table(route_text)?;
    let target = ProviderRouteTarget::from_provider_model(provider, model, &config.base_url)?;
    let group = table
        .rules
        .iter()
        .find(|rule| rule.matches(&target))
        .map(|rule| rule.group.as_str())
        .or(table.fallback.as_deref());
    let Some(group) = group else {
        return Ok(ProviderRoute {
            transport: ResolvedTransport::Direct {
                base_url: config.base_url.clone(),
            },
            key_slot: None,
        });
    };
    let action = table
        .groups
        .get(group)
        .cloned()
        .unwrap_or_else(|| RouteGroupAction::named_default(group));
    Ok(ProviderRoute {
        transport: action.transport.into_transport(&config.base_url, group)?,
        key_slot: action.key_slot,
    })
}

#[cfg(test)]
fn provider_transport(
    config: &RunnerProviderConfig,
    route_text: Option<&str>,
) -> Result<ResolvedTransport, String> {
    provider_route(config, "", "", route_text).map(|route| route.transport)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ModelTransportRouteTable {
    groups: BTreeMap<String, RouteGroupAction>,
    rules: Vec<RouteRule>,
    fallback: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RouteGroupAction {
    transport: RouteAction,
    key_slot: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RouteAction {
    Direct,
    Http { base_url: String },
    Unix { socket_path: String, base_url: String },
}

impl RouteAction {
    fn named_default(group: &str) -> Self {
        if group == "direct" || group == "must_direct" {
            Self::Direct
        } else {
            Self::Http {
                base_url: group.to_owned(),
            }
        }
    }

    fn into_transport(self, provider_base_url: &str, group: &str) -> Result<ResolvedTransport, String> {
        match self {
            Self::Direct => Ok(ResolvedTransport::Direct {
                base_url: provider_base_url.to_owned(),
            }),
            Self::Http { base_url } if is_url(&base_url) => Ok(ResolvedTransport::Http { base_url }),
            Self::Http { .. } => Err(format!("route group {group} is not defined")),
            Self::Unix {
                socket_path,
                base_url,
            } => Ok(ResolvedTransport::Unix {
                base_url,
                socket_path,
            }),
        }
    }
}

impl RouteGroupAction {
    fn named_default(group: &str) -> Self {
        Self {
            transport: RouteAction::named_default(group),
            key_slot: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RouteRule {
    matcher: RouteMatcher,
    group: String,
}

impl RouteRule {
    fn matches(&self, target: &ProviderRouteTarget) -> bool {
        match self.matcher {
            RouteMatcher::Domain(ref patterns) => patterns
                .iter()
                .any(|pattern| domain_matches(pattern, &target.host)),
            RouteMatcher::DestinationIp(ref patterns) => target
                .ip
                .as_ref()
                .is_some_and(|ip| patterns.iter().any(|pattern| ip_matches(pattern, ip))),
            RouteMatcher::ProcessName(ref names) => env::args()
                .next()
                .and_then(|path| {
                    PathBuf::from(path)
                        .file_name()
                        .map(ToOwned::to_owned)
                })
                .and_then(|value| value.to_str().map(str::to_owned))
                .is_some_and(|name| names.iter().any(|pattern| pattern == &name)),
            RouteMatcher::Provider(ref patterns) => patterns
                .iter()
                .any(|pattern| route_pattern_matches(pattern, &target.provider)),
            RouteMatcher::Model(ref patterns) => patterns
                .iter()
                .any(|pattern| route_pattern_matches(pattern, &target.model)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RouteMatcher {
    Domain(Vec<String>),
    DestinationIp(Vec<String>),
    ProcessName(Vec<String>),
    Provider(Vec<String>),
    Model(Vec<String>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProviderRouteTarget {
    provider: String,
    model: String,
    host: String,
    ip: Option<IpAddr>,
}

impl ProviderRouteTarget {
    fn from_provider_model(provider: &str, model: &str, base_url: &str) -> Result<Self, String> {
        let host = cortexfs::provider_host_from_base_url(base_url)
            .ok_or_else(|| "invalid provider base_url".to_owned())?;
        let ip = host.parse::<IpAddr>().ok();
        Ok(Self {
            provider: provider.to_owned(),
            model: model.to_owned(),
            host,
            ip,
        })
    }
}

fn parse_model_transport_route_table(content: &str) -> Result<ModelTransportRouteTable, String> {
    let mut table = ModelTransportRouteTable {
        groups: BTreeMap::from([
            ("direct".to_owned(), RouteGroupAction::named_default("direct")),
            (
                "must_direct".to_owned(),
                RouteGroupAction::named_default("must_direct"),
            ),
        ]),
        rules: Vec::new(),
        fallback: None,
    };
    for (index, raw_line) in content.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.split_once('#').map_or(raw_line, |(value, _comment)| value).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(value) = line.strip_prefix("fallback:") {
            table.fallback = Some(valid_route_name(value.trim(), line_number)?);
            continue;
        }
        let Some((left, right)) = line.split_once("->") else {
            return Err(format!("invalid route line {line_number}: missing ->"));
        };
        let left = left.trim();
        let right = right.trim();
        if let Some(name) = call_arg(left, "group") {
            table.groups.insert(
                valid_route_name(name.trim(), line_number)?,
                parse_route_action(right, line_number)?,
            );
            continue;
        }
        table.rules.push(RouteRule {
            matcher: parse_route_matcher(left, line_number)?,
            group: valid_route_name(right, line_number)?,
        });
    }
    Ok(table)
}

fn parse_route_matcher(value: &str, line: usize) -> Result<RouteMatcher, String> {
    if let Some(args) = call_arg(value, "domain") {
        return Ok(RouteMatcher::Domain(parse_route_list(args, line)?));
    }
    if let Some(args) = call_arg(value, "dip") {
        return Ok(RouteMatcher::DestinationIp(parse_route_list(args, line)?));
    }
    if let Some(args) = call_arg(value, "pname") {
        return Ok(RouteMatcher::ProcessName(parse_route_list(args, line)?));
    }
    if let Some(args) = call_arg(value, "provider") {
        return Ok(RouteMatcher::Provider(parse_route_list(args, line)?));
    }
    if let Some(args) = call_arg(value, "model") {
        return Ok(RouteMatcher::Model(parse_route_list(args, line)?));
    }
    Err(format!("invalid route matcher on line {line}"))
}

fn parse_route_action(value: &str, line: usize) -> Result<RouteGroupAction, String> {
    let mut transport = None;
    let mut key_slot = None;
    for part in split_route_action_parts(value) {
        if part == "direct" || part == "must_direct" {
            transport = Some(RouteAction::Direct);
            continue;
        }
        if let Some(url) = call_arg(part, "http") {
            let url = url.trim();
            if is_url(url) {
                transport = Some(RouteAction::Http {
                    base_url: url.to_owned(),
                });
                continue;
            }
            return Err(format!("invalid http group on line {line}"));
        }
        if let Some(args) = call_arg(part, "unix") {
            let values = parse_route_list(args, line)?;
            let Some(socket_path) = values.first() else {
                return Err(format!("invalid unix group on line {line}"));
            };
            let base_url = values
                .get(1)
                .cloned()
                .unwrap_or_else(|| "http://localhost/v1".to_owned());
            transport = Some(RouteAction::Unix {
                socket_path: socket_path.to_owned(),
                base_url,
            });
            continue;
        }
        if let Some(slot) = call_arg(part, "key") {
            key_slot = Some(valid_route_name(slot.trim(), line)?);
            continue;
        }
        return Err(format!("invalid group action on line {line}"));
    }
    let Some(transport) = transport else {
        return Err(format!("route group missing transport on line {line}"));
    };
    Ok(RouteGroupAction {
        transport,
        key_slot,
    })
}

fn split_route_action_parts(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0;
    for (index, character) in value.char_indices() {
        match character {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            ',' if depth == 0 => {
                let part = value.get(start..index).unwrap_or_default().trim();
                if !part.is_empty() {
                    parts.push(part);
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    let tail = value.get(start..).unwrap_or_default().trim();
    if !tail.is_empty() {
        parts.push(tail);
    }
    parts
}

fn parse_route_list(value: &str, line: usize) -> Result<Vec<String>, String> {
    let values = value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if values.is_empty() {
        Err(format!("empty route list on line {line}"))
    } else {
        Ok(values)
    }
}

fn call_arg<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    value.strip_prefix(name)?.strip_prefix('(')?.strip_suffix(')')
}

fn valid_route_name(value: &str, line: usize) -> Result<String, String> {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        Ok(value.to_owned())
    } else {
        Err(format!("invalid route group on line {line}"))
    }
}

fn domain_matches(pattern: &str, host: &str) -> bool {
    if let Some(geosite) = pattern.strip_prefix("geosite:") {
        return geosite == "cn"
            && host
                .rsplit('.')
                .next()
                .is_some_and(|suffix| suffix.eq_ignore_ascii_case("cn"));
    }
    host == pattern || host.ends_with(&format!(".{pattern}"))
}

fn route_pattern_matches(pattern: &str, value: &str) -> bool {
    pattern == "*" || pattern == value || pattern.strip_suffix('*').is_some_and(|prefix| value.starts_with(prefix))
}

fn ip_matches(pattern: &str, ip: &IpAddr) -> bool {
    match pattern {
        "geoip:private" => match *ip {
            IpAddr::V4(ip) => ip.is_private() || ip.is_loopback() || ip.is_link_local(),
            IpAddr::V6(ip) => ip.is_loopback() || ip.is_unique_local(),
        },
        "geoip:cn" => false,
        value => value.parse::<IpAddr>().is_ok_and(|target| &target == ip),
    }
}

fn is_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

fn provider_credential(
    provider: &str,
    config: &RunnerProviderConfig,
    key_slot: Option<&str>,
    driver: ProviderRuntimeDriver,
) -> Result<Option<ProviderCredential>, String> {
    let account = key_slot.unwrap_or("default");
    if let Some(api_key) = provider_secret_from_runtime_file(provider, account)
        .map_err(|_error| format!("runtime provider secret unavailable: {provider}"))?
    {
        return Ok(Some(match driver {
            ProviderRuntimeDriver::AnthropicMessages => ProviderCredential::AnthropicApiKey(api_key),
            ProviderRuntimeDriver::OpenAiChat | ProviderRuntimeDriver::OpenAiResponses => {
                ProviderCredential::Bearer(api_key)
            }
        }));
    }
    if let Some(api_key) = provider_secret_from_inherited_fd(provider, account)
        .map_err(|_error| format!("inherited provider secret unavailable: {provider}"))?
    {
        return Ok(Some(match driver {
            ProviderRuntimeDriver::AnthropicMessages => ProviderCredential::AnthropicApiKey(api_key),
            ProviderRuntimeDriver::OpenAiChat | ProviderRuntimeDriver::OpenAiResponses => {
                ProviderCredential::Bearer(api_key)
            }
        }));
    }
    if let Some(api_key) = cortexfs::read_provider_system_secret(provider, account)
        .map_err(|_error| format!("system provider secret unavailable: {provider}"))?
    {
        return Ok(Some(match driver {
            ProviderRuntimeDriver::AnthropicMessages => ProviderCredential::AnthropicApiKey(api_key),
            ProviderRuntimeDriver::OpenAiChat | ProviderRuntimeDriver::OpenAiResponses => {
                ProviderCredential::Bearer(api_key)
            }
        }));
    }
    let Some(oauth) = config.oauth.as_ref() else {
        return Ok(None);
    };
    if key_slot.is_none() {
        return cortexfs::resolve_oauth_access_token(provider, oauth)
            .map(|token| token.map(ProviderCredential::Bearer))
            .map_err(|_error| format!("oauth credential unavailable: {provider}"));
    }
    Ok(None)
}

fn provider_secret_from_runtime_file(
    provider: &str,
    account: &str,
) -> Result<Option<String>, io::Error> {
    if env::var("CTX_PROVIDER_SECRET_PROVIDER").as_deref() != Ok(provider)
        || env::var("CTX_PROVIDER_SECRET_SLOT").as_deref() != Ok(account)
    {
        return Ok(None);
    }
    let Ok(path) = env::var("CTX_PROVIDER_SECRET_PATH") else {
        return Ok(None);
    };
    if path.is_empty() {
        return Ok(None);
    }
    let secret = fs::read_to_string(path)?;
    let secret = secret.trim_end_matches(['\r', '\n']);
    if secret.is_empty() {
        Ok(None)
    } else {
        Ok(Some(secret.to_owned()))
    }
}

fn provider_secret_from_inherited_fd(
    provider: &str,
    account: &str,
) -> Result<Option<String>, io::Error> {
    if env::var("CTX_PROVIDER_SECRET_PROVIDER").as_deref() != Ok(provider)
        || env::var("CTX_PROVIDER_SECRET_SLOT").as_deref() != Ok(account)
    {
        return Ok(None);
    }
    let Ok(fd) = env::var("CTX_PROVIDER_SECRET_FD") else {
        return Ok(None);
    };
    if fd.is_empty() || !fd.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(None);
    }
    let secret = fs::read_to_string(format!("/proc/self/fd/{fd}"))?;
    let secret = secret.trim_end_matches(['\r', '\n']);
    if secret.is_empty() {
        Ok(None)
    } else {
        Ok(Some(secret.to_owned()))
    }
}

fn provider_name_from_config(
    base_url: &str,
    name: Option<&str>,
) -> Result<String, cortexfs::ProviderNameError> {
    cortexfs::provider_name_from_config(base_url, name)
}

fn model_effort(ctx_root: &Path, provider: &str, model: &str) -> cortexfs::ModelEffort {
    let path = ctx_root
        .join("model")
        .join(provider)
        .join(format!("{model}.d"))
        .join("effort");
    fs::read_to_string(path)
        .ok()
        .and_then(|content| cortexfs::ModelEffort::parse(&content))
        .unwrap_or(cortexfs::ModelEffort::Auto)
}

struct OpenAiProviderRequest<'a> {
    model: &'a str,
    input: &'a str,
    api_key: Option<&'a str>,
    effort: cortexfs::ModelEffort,
}

fn call_openai_chat(
    transport: &ResolvedTransport,
    request: &OpenAiProviderRequest<'_>,
) -> Result<ProviderTextCompletion, String> {
    let target = chat_completions_target(transport);
    let body = openai_chat_body(request.model, request.input, false, request.effort);
    let output = run_curl_json(&target, request.api_key, &body)?;
    Ok(ProviderTextCompletion {
        content: parse_openai_chat_content(&output)?,
        usage: parse_provider_usage(&output)?,
    })
}

fn call_openai_responses(
    transport: &ResolvedTransport,
    request: &OpenAiProviderRequest<'_>,
) -> Result<ProviderTextCompletion, String> {
    let target = responses_target(transport);
    let body = openai_responses_body(request.model, request.input, false, request.effort);
    let output = run_curl_json(&target, request.api_key, &body)?;
    Ok(ProviderTextCompletion {
        content: parse_openai_response_content(&output)?,
        usage: parse_provider_usage(&output)?,
    })
}

fn call_anthropic_messages(
    transport: &ResolvedTransport,
    model: &str,
    input: &str,
    credential: &ProviderCredential,
) -> Result<ProviderTextCompletion, String> {
    let target = anthropic_messages_target(transport);
    let body = json!({
        "model": model,
        "max_tokens": 4096,
        "messages": [{"role": "user", "content": input}]
    })
    .to_string();
    let headers = anthropic_headers(credential);
    let output = run_curl_json_with_headers(&target, &headers, &body)?;
    Ok(ProviderTextCompletion {
        content: parse_anthropic_message_content(&output)?,
        usage: parse_provider_usage(&output)?,
    })
}

struct StreamFailure {
    message: String,
    can_fallback: bool,
}

fn call_openai_chat_streaming(
    transport: &ResolvedTransport,
    request: &OpenAiProviderRequest<'_>,
    run: &str,
    stdout: &mut impl Write,
) -> Result<(), StreamFailure> {
    let target = chat_completions_target(transport);
    let body = openai_chat_body(request.model, request.input, true, request.effort);
    call_openai_sse_streaming(&target, request.api_key, &body, run, stdout)
}

fn call_openai_responses_streaming(
    transport: &ResolvedTransport,
    request: &OpenAiProviderRequest<'_>,
    run: &str,
    stdout: &mut impl Write,
) -> Result<(), StreamFailure> {
    let target = responses_target(transport);
    let body = openai_responses_body(request.model, request.input, true, request.effort);
    call_openai_sse_streaming(&target, request.api_key, &body, run, stdout)
}

fn openai_chat_body(
    model: &str,
    input: &str,
    stream: bool,
    effort: cortexfs::ModelEffort,
) -> String {
    let mut body = json!({
        "model": model,
        "messages": provider_messages(input),
        "stream": stream
    });
    if stream
        && let Some(object) = body.as_object_mut()
    {
        object.insert(
            "stream_options".to_owned(),
            json!({
                "include_usage": true
            }),
        );
    }
    apply_openai_effort(&mut body, effort);
    body.to_string()
}

fn openai_responses_body(
    model: &str,
    input: &str,
    stream: bool,
    effort: cortexfs::ModelEffort,
) -> String {
    let mut body = json!({
        "model": model,
        "input": provider_messages(input),
        "stream": stream
    });
    apply_openai_effort(&mut body, effort);
    body.to_string()
}

fn apply_openai_effort(body: &mut Value, effort: cortexfs::ModelEffort) {
    if effort == cortexfs::ModelEffort::Auto {
        return;
    }
    if let Some(object) = body.as_object_mut() {
        object.insert(
            "reasoning".to_owned(),
            json!({
                "effort": effort.to_string()
            }),
        );
    }
}

fn call_openai_sse_streaming(
    target: &CurlJsonTarget,
    api_key: Option<&str>,
    body: &str,
    run: &str,
    stdout: &mut impl Write,
) -> Result<(), StreamFailure> {
    let mut child = start_curl_json(target, api_key, body).map_err(|message| StreamFailure {
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
    let mut text_emitter = OpenAiStreamTextEmitter::new(run);
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
                if let Err(error) = text_emitter
                    .push(stdout, &text)
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
            Ok(OpenAiStreamEvent::Usage(usage)) => {
                if let Err(error) = write_model_usage(stdout, run, usage)
                    .and_then(|()| stdout.flush())
                {
                    cleanup_curl_child(&mut child);
                    return Err(StreamFailure {
                        message: format!("cannot write output: {error}"),
                        can_fallback: false,
                    });
                }
            }
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
        let stderr = child_stderr_text(&mut child);
        if emitted {
            return text_emitter
                .finish(stdout)
                .and_then(|()| stdout.flush())
                .map_err(|error| StreamFailure {
                    message: format!("cannot write output: {error}"),
                    can_fallback: false,
                });
        }
        return Err(StreamFailure {
            message: if stderr.trim().is_empty() {
                format!("provider stream request failed with {status}")
            } else {
                format!("provider stream request failed with {status}: {}", stderr.trim())
            },
            can_fallback: !emitted && !stderr.contains("Operation timed out"),
        });
    }
    if let Err(error) = text_emitter.finish(stdout).and_then(|()| stdout.flush()) {
        return Err(StreamFailure {
            message: format!("cannot write output: {error}"),
            can_fallback: false,
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

enum StreamTextMode {
    Undecided,
    BufferToolCall,
    Plain,
}

struct OpenAiStreamTextEmitter<'a> {
    run: &'a str,
    mode: StreamTextMode,
    buffer: String,
}

impl<'a> OpenAiStreamTextEmitter<'a> {
    fn new(run: &'a str) -> Self {
        Self {
            run,
            mode: StreamTextMode::Undecided,
            buffer: String::new(),
        }
    }

    fn push(&mut self, stdout: &mut impl Write, text: &str) -> io::Result<()> {
        match self.mode {
            StreamTextMode::Plain => write_model_delta(stdout, self.run, text),
            StreamTextMode::BufferToolCall => {
                self.buffer.push_str(text);
                Ok(())
            }
            StreamTextMode::Undecided => {
                self.buffer.push_str(text);
                let trimmed = self.buffer.trim_start();
                if trimmed.is_empty() {
                    return Ok(());
                }
                if trimmed.starts_with('{') {
                    self.mode = StreamTextMode::BufferToolCall;
                    return Ok(());
                }
                self.mode = StreamTextMode::Plain;
                let buffered = std::mem::take(&mut self.buffer);
                write_model_delta(stdout, self.run, &buffered)
            }
        }
    }

    fn finish(&mut self, stdout: &mut impl Write) -> io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let buffered = std::mem::take(&mut self.buffer);
        write_model_text_or_tool_call(stdout, self.run, &buffered)
    }
}

fn provider_messages(input: &str) -> Value {
    let agent = env::var("CTX_AGENT")
        .ok()
        .filter(|value| is_object_name(value));
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

fn run_curl_json(
    target: &CurlJsonTarget,
    api_key: Option<&str>,
    body: &str,
) -> Result<Vec<u8>, String> {
    let headers = openai_headers(api_key);
    run_curl_json_with_headers(target, &headers, body)
}

fn run_curl_json_with_headers(
    target: &CurlJsonTarget,
    headers: &[String],
    body: &str,
) -> Result<Vec<u8>, String> {
    let output = start_curl_json_with_headers(target, headers, body)?
        .wait_with_output()
        .map_err(|error| format!("cannot run curl: {error}"))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(provider_request_failure_message(&output))
    }
}

fn provider_request_failure_message(output: &std::process::Output) -> String {
    let body = String::from_utf8_lossy(&output.stdout);
    let body = body.trim();
    if !body.is_empty() {
        return format!("provider request failed with {}: {body}", output.status);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    if !stderr.is_empty() {
        return format!("provider request failed with {}: {stderr}", output.status);
    }
    format!("provider request failed with {}", output.status)
}

fn start_curl_json(
    target: &CurlJsonTarget,
    api_key: Option<&str>,
    body: &str,
) -> Result<std::process::Child, String> {
    let headers = openai_headers(api_key);
    start_curl_json_with_headers(target, &headers, body)
}

fn openai_headers(api_key: Option<&str>) -> Vec<String> {
    api_key.map_or_else(Vec::new, |api_key| {
        vec![format!("Authorization: Bearer {api_key}")]
    })
}

fn start_curl_json_with_headers(
    target: &CurlJsonTarget,
    headers: &[String],
    body: &str,
) -> Result<std::process::Child, String> {
    let mut child = Command::new("curl")
        .arg("--config")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot start curl: {error}"))?;
    let Some(mut stdin) = child.stdin.take() else {
        cleanup_curl_child(&mut child);
        return Err("cannot write curl config".to_owned());
    };
    let mut config = format!(
        "fail\nsilent\nshow-error\nno-buffer\nconnect-timeout = 5\nmax-time = {}\nrequest = POST\nurl = {}\n",
        provider_max_time_seconds(),
        curl_config_quote(&target.url),
    );
    if let Some(socket_path) = target.unix_socket.as_deref() {
        let _ignored = writeln!(config, "unix-socket = {}", curl_config_quote(socket_path));
    }
    for header in headers {
        let _ignored = writeln!(config, "header = {}", curl_config_quote(header));
    }
    let _ignored = write!(
        config,
        "header = {}\ndata = {}\n",
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

fn provider_max_time_seconds() -> u64 {
    env::var("CTX_PROVIDER_MAX_TIME_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| (1..=600).contains(value))
        .unwrap_or(20)
}

fn cleanup_curl_child(child: &mut std::process::Child) {
    let _ignored = child.kill();
    let _ignored = child.wait();
}

fn child_stderr_text(child: &mut std::process::Child) -> String {
    let Some(mut stderr) = child.stderr.take() else {
        return String::new();
    };
    let mut output = String::new();
    let _ignored = stderr.read_to_string(&mut output);
    output
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

fn parse_openai_response_content(output: &[u8]) -> Result<String, String> {
    let value = serde_json::from_slice::<Value>(output)
        .map_err(|error| format!("invalid provider json: {error}"))?;
    if let Some(text) = value.get("output_text").and_then(Value::as_str)
        && !text.is_empty()
    {
        return Ok(text.to_owned());
    }
    let mut content = String::new();
    if let Some(items) = value.get("output").and_then(Value::as_array) {
        for item in items {
            let Some(parts) = item.get("content").and_then(Value::as_array) else {
                continue;
            };
            for part in parts {
                if matches!(
                    part.get("type").and_then(Value::as_str),
                    Some("output_text" | "text")
                ) && let Some(text) = part.get("text").and_then(Value::as_str)
                {
                    content.push_str(text);
                }
            }
        }
    }
    if content.is_empty() {
        Err("provider response missing content".to_owned())
    } else {
        Ok(content)
    }
}

fn parse_anthropic_message_content(output: &[u8]) -> Result<String, String> {
    let value = serde_json::from_slice::<Value>(output)
        .map_err(|error| format!("invalid provider json: {error}"))?;
    let parts = value
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| "provider response missing content".to_owned())?;
    let mut output = String::new();
    for part in parts {
        if part.get("type").and_then(Value::as_str) == Some("text")
            && let Some(text) = part.get("text").and_then(Value::as_str)
        {
            output.push_str(text);
        }
    }
    if output.is_empty() {
        Err("provider response missing text content".to_owned())
    } else {
        Ok(output)
    }
}

enum OpenAiStreamEvent {
    Delta(String),
    Usage(TokenUsage),
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
    match value.get("type").and_then(Value::as_str) {
        Some("response.output_text.delta" | "response.output_text.done") => {
            return Ok(OpenAiStreamEvent::Delta(
                value
                    .get("delta")
                    .or_else(|| value.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            ));
        }
        Some("response.content_part.done") => {
            return Ok(OpenAiStreamEvent::Delta(
                value
                    .pointer("/part/text")
                    .or_else(|| value.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            ));
        }
        Some("response.output_item.done") => {
            return Ok(OpenAiStreamEvent::Delta(response_output_item_text(
                value.get("item"),
            )));
        }
        Some("response.completed" | "response.done") => return Ok(OpenAiStreamEvent::Done),
        Some("response.failed" | "response.incomplete" | "error") => {
            let message = value
                .pointer("/error/message")
                .or_else(|| value.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("provider stream failed");
            return Err(message.to_owned());
        }
        _ => {}
    }
    if let Some(usage) = token_usage_from_value(&value) {
        return Ok(OpenAiStreamEvent::Usage(usage));
    }
    let text = value
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
        .or_else(|| value.get("delta").and_then(Value::as_str))
        .or_else(|| value.get("output_text").and_then(Value::as_str))
        .unwrap_or_default();
    Ok(OpenAiStreamEvent::Delta(text.to_owned()))
}

fn response_output_item_text(item: Option<&Value>) -> String {
    let Some(item) = item else {
        return String::new();
    };
    if let Some(text) = item.get("output_text").and_then(Value::as_str) {
        return text.to_owned();
    }
    let Some(parts) = item.get("content").and_then(Value::as_array) else {
        return String::new();
    };
    let mut output = String::new();
    for part in parts {
        if matches!(
            part.get("type").and_then(Value::as_str),
            Some("output_text" | "text")
        ) && let Some(text) = part.get("text").and_then(Value::as_str)
        {
            output.push_str(text);
        }
    }
    output
}

fn parse_provider_usage(output: &[u8]) -> Result<Option<TokenUsage>, String> {
    let value = serde_json::from_slice::<Value>(output)
        .map_err(|error| format!("invalid provider json: {error}"))?;
    Ok(token_usage_from_value(&value))
}

fn token_usage_from_value(value: &Value) -> Option<TokenUsage> {
    usage_value_candidates(value)
        .into_iter()
        .find_map(token_usage_from_usage_value)
}

fn usage_value_candidates(value: &Value) -> Vec<&Value> {
    [
        value.get("usage"),
        value.pointer("/response/usage"),
        value.pointer("/message/usage"),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn token_usage_from_usage_value(value: &Value) -> Option<TokenUsage> {
    let input_tokens = value
        .get("input_tokens")
        .or_else(|| value.get("prompt_tokens"))
        .and_then(Value::as_u64)?;
    let output_tokens = value
        .get("output_tokens")
        .or_else(|| value.get("completion_tokens"))
        .and_then(Value::as_u64)?;
    Some(TokenUsage {
        input_tokens,
        output_tokens,
    })
}

fn chat_completions_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.rsplit('/').next() == Some("v1") {
        format!("{base}/chat/completions")
    } else {
        format!("{base}/v1/chat/completions")
    }
}

fn responses_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.rsplit('/').next() == Some("v1") {
        format!("{base}/responses")
    } else {
        format!("{base}/v1/responses")
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

fn responses_target(transport: &ResolvedTransport) -> CurlJsonTarget {
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
        url: responses_url(base_url),
        unix_socket,
    }
}

fn anthropic_messages_target(transport: &ResolvedTransport) -> CurlJsonTarget {
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
        url: anthropic_messages_url(base_url),
        unix_socket,
    }
}

fn anthropic_messages_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.rsplit('/').next() == Some("v1") {
        format!("{base}/messages")
    } else {
        format!("{base}/v1/messages")
    }
}

fn anthropic_headers(credential: &ProviderCredential) -> Vec<String> {
    let auth = match *credential {
        ProviderCredential::Bearer(ref token) => format!("Authorization: Bearer {token}"),
        ProviderCredential::AnthropicApiKey(ref key) => format!("x-api-key: {key}"),
    };
    vec![auth, "anthropic-version: 2023-06-01".to_owned()]
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
