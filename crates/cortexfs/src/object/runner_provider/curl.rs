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
    let output = wait_for_curl_json_output(start_curl_json_with_headers(target, headers, body)?)?;
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
) -> Result<Child, String> {
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
) -> Result<Child, String> {
    let mut child = provider_curl_command()
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
        curl_config_quote(&target.url)?,
    );
    if let Some(socket_path) = target.unix_socket.as_deref() {
        let _ignored = writeln!(config, "unix-socket = {}", curl_config_quote(socket_path)?);
    }
    for header in headers {
        let _ignored = writeln!(config, "header = {}", curl_config_quote(header)?);
    }
    let _ignored = write!(
        config,
        "header = {}\ndata = {}\n",
        curl_config_quote("Content-Type: application/json")?,
        curl_config_quote(body)?,
    );
    if let Err(error) = stdin.write_all(config.as_bytes()) {
        cleanup_curl_child(&mut child);
        return Err(format!("cannot write curl config: {error}"));
    }
    drop(stdin);
    Ok(child)
}

fn provider_curl_command() -> Command {
    let mut command = Command::new(PROVIDER_CURL_BIN);
    command.env_clear().arg("-q").arg("--config").arg("-");
    command
}

fn wait_for_curl_json_output(mut child: Child) -> Result<std::process::Output, String> {
    let Some(stdout) = child.stdout.take() else {
        cleanup_curl_child(&mut child);
        return Err("cannot read curl response".to_owned());
    };
    let Some(stderr) = child.stderr.take() else {
        cleanup_curl_child(&mut child);
        return Err("cannot read curl diagnostics".to_owned());
    };
    let stdout_reader =
        thread::spawn(move || {
        read_limited_bytes(stdout, MAX_PROVIDER_RESPONSE_BYTES + 1)
    });
    let stderr_reader = thread::spawn(move || {
        read_limited_bytes(stderr, MAX_CHILD_STDERR_BYTES + 1)
    });
    let (status, mut stdout, stderr) =
        wait_for_limited_curl_output(child, stdout_reader, stderr_reader)?;
    Ok(std::process::Output {
        status,
        stdout: std::mem::take(&mut stdout),
        stderr,
    })
}

fn wait_for_limited_curl_output(
    mut child: Child,
    stdout_reader: thread::JoinHandle<Vec<u8>>,
    stderr_reader: thread::JoinHandle<Vec<u8>>,
) -> Result<CurlJsonOutputParts, String> {
    let mut stdout_reader = Some(stdout_reader);
    let mut stderr_reader = Some(stderr_reader);
    let mut stdout = None;
    let mut stderr = None;
    let status = loop {
        if stdout.is_none()
            && stdout_reader
                .as_ref()
                .is_some_and(thread::JoinHandle::is_finished)
        {
            let output = stdout_reader
                .take()
                .and_then(|reader| reader.join().ok())
                .unwrap_or_default();
            if output.len() > MAX_PROVIDER_RESPONSE_BYTES {
                cleanup_curl_child(&mut child);
                if let Some(reader) = stderr_reader.take() {
                    let _ignored = reader.join();
                }
                return Err(format!(
                    "provider response exceeds {MAX_PROVIDER_RESPONSE_BYTES} bytes"
                ));
            }
            stdout = Some(output);
        }
        if stderr.is_none()
            && stderr_reader
                .as_ref()
                .is_some_and(thread::JoinHandle::is_finished)
        {
            let output = stderr_reader
                .take()
                .and_then(|reader| reader.join().ok())
                .unwrap_or_default();
            if output.len() > MAX_CHILD_STDERR_BYTES {
                cleanup_curl_child(&mut child);
                if let Some(reader) = stdout_reader.take() {
                    let _ignored = reader.join();
                }
                return Err(format!(
                    "provider diagnostics exceeds {MAX_CHILD_STDERR_BYTES} bytes"
                ));
            }
            stderr = Some(output);
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("cannot run curl: {error}"))?
        {
            break status;
        }
        thread::sleep(Duration::from_millis(10));
    };
    let stdout = stdout.unwrap_or_else(|| {
        stdout_reader
            .take()
            .and_then(|reader| reader.join().ok())
            .unwrap_or_default()
    });
    let mut stderr = stderr.unwrap_or_else(|| {
        stderr_reader
            .take()
            .and_then(|reader| reader.join().ok())
            .unwrap_or_default()
    });
    if stdout.len() > MAX_PROVIDER_RESPONSE_BYTES {
        return Err(format!(
            "provider response exceeds {MAX_PROVIDER_RESPONSE_BYTES} bytes"
        ));
    }
    if stderr.len() > MAX_CHILD_STDERR_BYTES {
        stderr.truncate(MAX_CHILD_STDERR_BYTES);
    }
    Ok((status, stdout, stderr))
}

fn provider_max_time_seconds() -> u64 {
    env::var("CTX_PROVIDER_MAX_TIME_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| (1..=600).contains(value))
        .unwrap_or(60)
}
fn cleanup_curl_child(child: &mut Child) {
    let _ignored = child.kill();
    let _ignored = child.wait();
}
fn curl_config_quote(value: &str) -> Result<String, String> {
    let mut quoted = String::from("\"");
    for character in value.chars() {
        if character.is_ascii_control() {
            return Err("curl config value contains a forbidden control character".to_owned());
        }
        if matches!(character, '"' | '\\') {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');
    Ok(quoted)
}
