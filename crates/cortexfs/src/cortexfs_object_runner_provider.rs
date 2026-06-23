use serde_json::json;

struct RunnerProviderConfig {
    base_url: String,
    api_key_env: Option<String>,
}

fn provider_chat_completion(name: &str, input: &str) -> Result<String, String> {
    let (provider, model) = name
        .split_once('/')
        .ok_or_else(|| format!("invalid provider model: {name}"))?;
    let config =
        provider_config(provider).ok_or_else(|| format!("missing provider: {provider}"))?;
    let key = provider_key(&config).ok_or_else(|| format!("missing api key: {provider}"))?;
    call_openai_chat(&config.base_url, model, input, &key)
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

fn provider_key(config: &RunnerProviderConfig) -> Option<String> {
    for name in provider_key_names(config) {
        if let Ok(value) = env::var(name)
            && !value.trim().is_empty()
        {
            return Some(value);
        }
    }
    None
}

fn provider_key_names(config: &RunnerProviderConfig) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(name) = config.api_key_env.as_deref() {
        names.push(name.to_owned());
    }
    if let Some(host) = provider_name_from_base_url(&config.base_url) {
        names.push(format!(
            "{}_API_KEY",
            host.replace('.', "_").to_ascii_uppercase()
        ));
    }
    names
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
        "messages": [{"role": "user", "content": input}],
        "stream": false
    })
    .to_string();
    let output = run_curl_json(&url, api_key, &body)?;
    parse_openai_chat_content(&output)
}

fn run_curl_json(url: &str, api_key: &str, body: &str) -> Result<Vec<u8>, String> {
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
        "fail\nsilent\nshow-error\nmax-time = 60\nrequest = POST\nurl = {}\nheader = {}\nheader = {}\ndata = {}\n",
        curl_config_quote(url),
        curl_config_quote(&format!("Authorization: Bearer {api_key}")),
        curl_config_quote("Content-Type: application/json"),
        curl_config_quote(body),
    );
    stdin
        .write_all(config.as_bytes())
        .map_err(|error| format!("cannot write curl config: {error}"))?;
    drop(stdin);
    let output = child
        .wait_with_output()
        .map_err(|error| format!("cannot run curl: {error}"))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err("provider request failed".to_owned())
    }
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
