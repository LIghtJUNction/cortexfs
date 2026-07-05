const ID_PROGRAM: &str = "/usr/bin/id";

fn get_id_program() -> &'static str {
    ID_PROGRAM
}

fn id_command() -> std::process::Command {
    let mut command = std::process::Command::new(get_id_program());
    command
        .arg("-u")
        .env_clear()
        .env("PATH", "/usr/bin:/bin");
    command
}

fn current_uid_text() -> Result<String, String> {
    let output = id_command()
        .output()
        .map_err(|error| format!("cannot run id -u: {error}"))?;
    if !output.status.success() {
        return Err("id -u failed".to_owned());
    }
    let uid = String::from_utf8(output.stdout)
        .map_err(|_error| "id -u returned non-UTF-8 output".to_owned())?;
    parse_current_uid_text(&uid).map_err(str::to_owned)
}

fn parse_current_uid_text(output: &str) -> Result<String, &'static str> {
    let uid = output.trim();
    if uid.is_empty() {
        return Err("id -u returned empty output");
    }
    if !uid.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("id -u returned invalid uid");
    }
    Ok(uid.to_owned())
}
