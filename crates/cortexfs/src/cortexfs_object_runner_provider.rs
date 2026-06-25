use serde_json::json;

struct RunnerProviderConfig {
    base_url: String,
    api_key_env: Option<String>,
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
    let key = provider_key(&config)?.ok_or_else(|| format!("missing api key: {provider}"))?;
    match call_openai_chat_streaming(&config.base_url, model, input, &key, run, stdout) {
        Ok(()) => Ok(()),
        Err(error) if error.can_fallback => {
            let content = call_openai_chat(&config.base_url, model, input, &key)?;
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
        return Some(RunnerProviderConfig {
            base_url,
            api_key_env: value
                .get("api_key_env")
                .and_then(Value::as_str)
                .map(str::to_owned),
        });
    }
    None
}

fn provider_key(config: &RunnerProviderConfig) -> Result<Option<String>, String> {
    let Some(provider) = provider_name_from_base_url(&config.base_url) else {
        return Ok(None);
    };
    resolve_api_key_from_env_names(
        &provider_key_names(config),
        &provider_keychain_service(&provider),
        "default",
    )
    .map_err(|_error| format!("keychain unavailable: {provider}"))
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
    base_url: &str,
    model: &str,
    input: &str,
    api_key: &str,
) -> Result<String, String> {
    let url = chat_completions_url(base_url);
    let body = json!({
        "model": model,
        "messages": provider_messages(input),
        "stream": false
    })
    .to_string();
    let output = run_curl_json(&url, api_key, &body)?;
    parse_openai_chat_content(&output)
}

struct StreamFailure {
    message: String,
    can_fallback: bool,
}

fn call_openai_chat_streaming(
    base_url: &str,
    model: &str,
    input: &str,
    api_key: &str,
    run: &str,
    stdout: &mut impl Write,
) -> Result<(), StreamFailure> {
    let url = chat_completions_url(base_url);
    let body = json!({
        "model": model,
        "messages": provider_messages(input),
        "stream": true
    })
    .to_string();
    let mut child = start_curl_json(&url, api_key, &body).map_err(|message| StreamFailure {
        message,
        can_fallback: true,
    })?;
    let child_stdout = child.stdout.take().ok_or_else(|| StreamFailure {
        message: "cannot read provider stream".to_owned(),
        can_fallback: true,
    })?;
    let mut emitted = false;
    let mut done = false;
    for line in BufReader::new(child_stdout).lines() {
        let line = line.map_err(|error| StreamFailure {
            message: format!("cannot read provider stream: {error}"),
            can_fallback: !emitted,
        })?;
        match openai_stream_event(&line) {
            Ok(OpenAiStreamEvent::Delta(text)) if !text.is_empty() => {
                write_model_delta(stdout, run, &text)
                    .and_then(|()| stdout.flush())
                    .map_err(|error| StreamFailure {
                        message: format!("cannot write output: {error}"),
                        can_fallback: false,
                })?;
                emitted = true;
            }
            Ok(OpenAiStreamEvent::Delta(_empty)) => {}
            Ok(OpenAiStreamEvent::Done) => done = true,
            Ok(OpenAiStreamEvent::Ignore) => {}
            Err(message) => {
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
    provider_messages_for_agent(input, agent.as_deref(), &agent_system)
}

fn provider_messages_for_agent(input: &str, agent: Option<&str>, agent_system: &str) -> Value {
    agent.map_or_else(
        || json!([{"role": "user", "content": input}]),
        |agent| json!([
            {"role": "system", "content": agent_system_prompt(agent, agent_system)},
            {"role": "user", "content": input}
        ]),
    )
}

fn agent_system_prompt(agent: &str, agent_system: &str) -> String {
    let custom = agent_system.trim();
    let custom_section = if custom.is_empty() {
        String::new()
    } else {
        format!(
            "\
Agent instructions from agent/{agent}.d/system.md:
---
{custom}
---

"
        )
    };
    format!(
        "\
{custom_section}CortexFS immutable runtime contract:
You are CortexFS agent `{agent}`.
Your only native callable tool is `tsh`, the CortexFS tool shell.
Do not claim direct access to provider, host, or assistant-platform tools.
If asked what tools you can call, answer that you can call `tsh` only.
Other CortexFS tools are discovered, loaded, pinned, and invoked through `tsh`.
Use `tsh tools` to discover tools, `tsh load TOOL` to load a tool description into context, \
`tsh pin TOOL` to keep it resident, and `tsh TOOL ARG...` to invoke it.
Interactive shells and multiplexers such as bash, tmux, and zellij are ordinary CortexFS tools \
that must be invoked through `tsh` when visible."
    )
}

fn run_curl_json(url: &str, api_key: &str, body: &str) -> Result<Vec<u8>, String> {
    let output = start_curl_json(url, api_key, body)?
        .wait_with_output()
        .map_err(|error| format!("cannot run curl: {error}"))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err("provider request failed".to_owned())
    }
}

fn start_curl_json(url: &str, api_key: &str, body: &str) -> Result<std::process::Child, String> {
    let mut child = Command::new("curl")
        .arg("--config")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("cannot start curl: {error}"))?;
    let Some(mut stdin) = child.stdin.take() else {
        return Err("cannot write curl config".to_owned());
    };
    let config = format!(
        "fail\nsilent\nshow-error\nno-buffer\nmax-time = 60\nrequest = POST\nurl = {}\nheader = {}\nheader = {}\ndata = {}\n",
        curl_config_quote(url),
        curl_config_quote(&format!("Authorization: Bearer {api_key}")),
        curl_config_quote("Content-Type: application/json"),
        curl_config_quote(body),
    );
    stdin
        .write_all(config.as_bytes())
        .map_err(|error| format!("cannot write curl config: {error}"))?;
    drop(stdin);
    Ok(child)
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
