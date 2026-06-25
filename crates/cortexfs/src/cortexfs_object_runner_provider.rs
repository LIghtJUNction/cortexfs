use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt::Write as FmtWrite;
use std::net::IpAddr;

use serde_json::json;

#[derive(Clone, Debug, Deserialize)]
struct RunnerProviderConfig {
    base_url: String,
    api_key_env: Option<String>,
    oauth: Option<cortexfs::OAuthProviderConfig>,
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
    let ctx_root =
        env::var_os("CTX_ROOT").map_or_else(|| PathBuf::from(DEFAULT_CTX_ROOT), PathBuf::from);
    let route = fs::read_to_string(ctx_root.join("model").join("route")).ok();
    let route = provider_route(&config, provider, model, route.as_deref())?;
    let key = provider_bearer_token(&config, route.key_slot.as_deref())?
        .ok_or_else(|| format!("missing provider credential: {provider}"))?;
    match call_openai_chat_streaming(&route.transport, model, input, &key, run, stdout) {
        Ok(()) => Ok(()),
        Err(error) if error.can_fallback => {
            let content = call_openai_chat(&route.transport, model, input, &key)?;
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
        let host = provider_host(base_url).ok_or_else(|| "invalid provider base_url".to_owned())?;
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

fn provider_bearer_token(
    config: &RunnerProviderConfig,
    key_slot: Option<&str>,
) -> Result<Option<String>, String> {
    let Some(provider) = provider_name_from_base_url(&config.base_url) else {
        return Ok(None);
    };
    let account = key_slot.unwrap_or("default");
    let api_key = resolve_api_key_from_env_names(
        &provider_key_names(config, key_slot),
        &provider_keychain_service(&provider),
        account,
    )
    .map_err(|_error| format!("keychain unavailable: {provider}"))?;
    if api_key.is_some() {
        return Ok(api_key);
    }
    let Some(oauth) = config.oauth.as_ref() else {
        return Ok(None);
    };
    if key_slot.is_none() {
        return cortexfs::resolve_oauth_access_token(&provider, oauth)
            .map_err(|_error| format!("oauth credential unavailable: {provider}"));
    }
    Ok(None)
}

fn provider_key_names(config: &RunnerProviderConfig, key_slot: Option<&str>) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(name) = config.api_key_env.as_deref() {
        append_provider_key_name_with_slot(name, key_slot, &mut names);
    }
    if let Some(host) = provider_name_from_base_url(&config.base_url) {
        append_provider_key_name_for_host(&host, true, key_slot, &mut names);
        append_provider_key_name_for_host(&host, false, key_slot, &mut names);
    }
    names
}

fn append_provider_key_name_for_host(
    host: &str,
    drop_api_prefix: bool,
    key_slot: Option<&str>,
    names: &mut Vec<String>,
) {
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
    let name = labels
        .iter()
        .map(|part| part.to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join("_")
        + "_API_KEY";
    append_provider_key_name_with_slot(&name, key_slot, names);
}

fn append_provider_key_name_with_slot(name: &str, key_slot: Option<&str>, names: &mut Vec<String>) {
    if let Some(slot) = key_slot.and_then(env_slot_suffix) {
        append_provider_key_name(&format!("{name}_{slot}"), names);
        if let Some(prefix) = name.strip_suffix("_API_KEY") {
            append_provider_key_name(&format!("{prefix}_{slot}_API_KEY"), names);
        }
    }
    append_provider_key_name(name, names);
}

fn env_slot_suffix(slot: &str) -> Option<String> {
    let mut value = String::new();
    for byte in slot.bytes() {
        match byte {
            b'a'..=b'z' => value.push(char::from(byte.to_ascii_uppercase())),
            b'A'..=b'Z' | b'0'..=b'9' => value.push(char::from(byte)),
            b'.' | b'-' | b'_' => value.push('_'),
            _ => return None,
        }
    }
    (!value.is_empty()).then_some(value)
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
    let host = provider_host(base_url)?;
    (!host.is_empty()).then_some(host)
}

fn provider_host(base_url: &str) -> Option<String> {
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
