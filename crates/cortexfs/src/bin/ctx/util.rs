fn read_file_to_string(path: &Path) -> Result<String, CliError> {
    fs::read_to_string(path)
        .map_err(|error| CliError::unavailable(format!("cannot read {}: {error}", path.display())))
}

fn classify_input_path(root: &Path, path: &str) -> Result<String, CliError> {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        let relative = candidate.strip_prefix(root).map_err(|error| {
            CliError::usage(format!(
                "absolute file path must be under CTX_ROOT: {error}"
            ))
        })?;
        validate_relative_abi_path(relative)?;
        return Ok(relative.display().to_string());
    }
    Ok(path.to_owned())
}

fn resolve_abi_path(root: &Path, path: &str) -> Result<PathBuf, CliError> {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        let relative = candidate.strip_prefix(root).map_err(|error| {
            CliError::usage(format!(
                "absolute file path must be under CTX_ROOT: {error}"
            ))
        })?;
        validate_relative_abi_path(relative)?;
        return Ok(root.join(relative));
    }

    if path
        .split('/')
        .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(CliError::usage("file path must be a relative ABI path"));
    }

    Ok(root.join(path))
}

fn validate_relative_abi_path(path: &Path) -> Result<(), CliError> {
    if path
        .components()
        .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(CliError::usage("file path must be a relative ABI path"));
    }

    Ok(())
}

fn newline_terminated(value: &str) -> String {
    if value.ends_with('\n') {
        value.to_owned()
    } else {
        format!("{value}\n")
    }
}

fn json_string(value: &str) -> String {
    let mut escaped = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                push_json_unicode_escape(&mut escaped, character);
            }
            character => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

fn push_json_unicode_escape(output: &mut String, character: char) {
    let value = u32::from(character);
    output.push_str("\\u");
    output.push(hex_digit((value >> 12) & 0x0f));
    output.push(hex_digit((value >> 8) & 0x0f));
    output.push(hex_digit((value >> 4) & 0x0f));
    output.push(hex_digit(value & 0x0f));
}

fn hex_digit(value: u32) -> char {
    match value {
        0 => '0',
        1 => '1',
        2 => '2',
        3 => '3',
        4 => '4',
        5 => '5',
        6 => '6',
        7 => '7',
        8 => '8',
        9 => '9',
        10 => 'a',
        11 => 'b',
        12 => 'c',
        13 => 'd',
        14 => 'e',
        15 => 'f',
        _ => '?',
    }
}

fn temp_file_name() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!(".ctx.tmp.{}.{}", std::process::id(), nanos)
}

fn validate_name(name: &str) -> Result<(), CliError> {
    if is_object_name(name) {
        print_line("ok")
    } else {
        Err(CliError::usage(format!("invalid name: {name}")))
    }
}

fn ctx_tool_path(root: &Path) -> Result<ToolPath, CliError> {
    match env::var("CTX_PATH") {
        Ok(value) => Ok(ToolPath::parse(&value)),
        Err(env::VarError::NotPresent) => Ok(ToolPath::default(root, &ctx_home(root)?)),
        Err(env::VarError::NotUnicode(_value)) => {
            Err(CliError::usage("CTX_PATH must be valid UTF-8"))
        }
    }
}

fn tool_path_error(error: cortexfs::ToolPathError) -> CliError {
    match error {
        cortexfs::ToolPathError::InvalidName => CliError::usage("invalid tool name"),
        cortexfs::ToolPathError::CannotReadDirectory => {
            CliError::unavailable("cannot read CTX_PATH directory")
        }
    }
}

fn is_mount_point(root: &Path) -> io::Result<bool> {
    let mountinfo = fs::read_to_string("/proc/self/mountinfo")?;
    let root = root.display().to_string();
    Ok(mountinfo
        .lines()
        .any(|line| mount_point(line).is_some_and(|point| point == root)))
}

fn mount_point(line: &str) -> Option<String> {
    let mut fields = line.split(' ');
    let _id = fields.next()?;
    let _parent = fields.next()?;
    let _major_minor = fields.next()?;
    let _root = fields.next()?;
    fields.next().map(unescape_mountinfo)
}

fn unescape_mountinfo(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

fn shell_quote(value: &str) -> String {
    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-' | b':')
    }) {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn print_lines(lines: &[&str]) -> Result<(), CliError> {
    for line in lines {
        print_line(line)?;
    }
    Ok(())
}

fn print_line(line: &str) -> Result<(), CliError> {
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(line.as_bytes())
        .and_then(|()| stdout.write_all(b"\n"))
        .map_err(|error| CliError::unavailable(format!("stdout write failed: {error}")))
}

fn write_error(line: &str) -> io::Result<()> {
    let mut stderr = io::stderr().lock();
    stderr
        .write_all(line.as_bytes())
        .and_then(|()| stderr.write_all(b"\n"))
}
