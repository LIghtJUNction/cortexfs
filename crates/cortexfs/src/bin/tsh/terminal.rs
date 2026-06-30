struct RawTerminal<'fd> {
    fd: BorrowedFd<'fd>,
    original: termios::Termios,
}

impl<'fd> RawTerminal<'fd> {
    fn enable(stdin: &'fd io::Stdin) -> Result<Self, TshError> {
        let fd = stdin.as_fd();
        let original = termios::tcgetattr(fd).map_err(|error| {
            TshError::unavailable(format!("cannot read terminal mode: {error}"))
        })?;
        let mut raw = original.clone();
        raw.input_flags.remove(
            InputFlags::BRKINT
                | InputFlags::ICRNL
                | InputFlags::INPCK
                | InputFlags::ISTRIP
                | InputFlags::IXON,
        );
        raw.output_flags.remove(OutputFlags::OPOST);
        raw.control_flags.insert(ControlFlags::CS8);
        raw.local_flags
            .remove(LocalFlags::ECHO | LocalFlags::ICANON | LocalFlags::IEXTEN | LocalFlags::ISIG);
        if let Some(value) = raw.control_chars.get_mut(libc::VMIN) {
            *value = 1;
        }
        if let Some(value) = raw.control_chars.get_mut(libc::VTIME) {
            *value = 0;
        }
        termios::tcsetattr(fd, SetArg::TCSAFLUSH, &raw).map_err(|error| {
            TshError::unavailable(format!("cannot switch terminal to raw mode: {error}"))
        })?;
        Ok(Self { fd, original })
    }
}

impl Drop for RawTerminal<'_> {
    fn drop(&mut self) {
        let _restored = termios::tcsetattr(self.fd, SetArg::TCSAFLUSH, &self.original);
    }
}

#[derive(Clone, Copy)]
enum ReplKey {
    Byte(u8),
    Up,
    Down,
    Right,
    Left,
    Home,
    End,
}

fn read_repl_line(prompt: &str, history: &[String]) -> Result<Option<String>, TshError> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    if !stdin.is_terminal() || !stdout.is_terminal() {
        return read_repl_line_canonical(prompt);
    }

    let _raw = RawTerminal::enable(&stdin)?;
    write_stdout(prompt)?;

    let mut input = stdin.lock();
    let mut buffer = Vec::new();
    let mut cursor = 0usize;
    let mut history_cursor: Option<usize> = None;

    loop {
        match read_repl_key(&mut input)? {
            ReplKey::Byte(b'\r' | b'\n') => {
                write_stdout("\r\n")?;
                return Ok(Some(buffer.into_iter().collect()));
            }
            ReplKey::Byte(4) if buffer.is_empty() => {
                write_stdout("\r\n")?;
                return Ok(None);
            }
            ReplKey::Byte(3) => {
                write_stdout("^C\r\n")?;
                return Ok(Some(String::new()));
            }
            ReplKey::Byte(8 | 127) => {
                if cursor > 0 {
                    cursor -= 1;
                    buffer.remove(cursor);
                    redraw_repl_line(prompt, &buffer, cursor)?;
                }
            }
            ReplKey::Byte(byte) if byte.is_ascii_graphic() || byte == b' ' => {
                if buffer.len() >= MAX_TSH_REPL_LINE_BYTES {
                    return Err(TshError::usage("tsh input line exceeds limit"));
                }
                buffer.insert(cursor, char::from(byte));
                cursor += 1;
                history_cursor = None;
                redraw_repl_line(prompt, &buffer, cursor)?;
            }
            ReplKey::Up => {
                if history.is_empty() {
                    continue;
                }
                let next =
                    history_cursor.map_or(history.len() - 1, |index| index.saturating_sub(1));
                history_cursor = Some(next);
                if let Some(entry) = history.get(next) {
                    buffer = entry.chars().collect();
                    cursor = buffer.len();
                    redraw_repl_line(prompt, &buffer, cursor)?;
                }
            }
            ReplKey::Down => {
                let Some(index) = history_cursor else {
                    continue;
                };
                if index + 1 < history.len() {
                    let next = index + 1;
                    history_cursor = Some(next);
                    if let Some(entry) = history.get(next) {
                        buffer = entry.chars().collect();
                        cursor = buffer.len();
                    }
                } else {
                    history_cursor = None;
                    buffer.clear();
                    cursor = 0;
                }
                redraw_repl_line(prompt, &buffer, cursor)?;
            }
            ReplKey::Left => {
                if cursor > 0 {
                    cursor -= 1;
                    redraw_repl_line(prompt, &buffer, cursor)?;
                }
            }
            ReplKey::Right => {
                if cursor < buffer.len() {
                    cursor += 1;
                    redraw_repl_line(prompt, &buffer, cursor)?;
                }
            }
            ReplKey::Home => {
                cursor = 0;
                redraw_repl_line(prompt, &buffer, cursor)?;
            }
            ReplKey::End => {
                cursor = buffer.len();
                redraw_repl_line(prompt, &buffer, cursor)?;
            }
            ReplKey::Byte(_) => {}
        }
    }
}

fn read_repl_line_canonical(prompt: &str) -> Result<Option<String>, TshError> {
    write_stdout(prompt)?;
    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    read_repl_line_canonical_from(&mut stdin)
}

fn read_repl_line_canonical_from(reader: &mut impl BufRead) -> Result<Option<String>, TshError> {
    let mut line = String::new();
    let limit = u64::try_from(MAX_TSH_REPL_LINE_BYTES.saturating_add(2))
        .map_err(|error| TshError::unavailable(format!("input limit is invalid: {error}")))?;
    let bytes = reader
        .take(limit)
        .read_line(&mut line)
        .map_err(|error| TshError::unavailable(format!("cannot read input: {error}")))?;
    if bytes == 0 {
        return Ok(None);
    }
    while line.ends_with(['\n', '\r']) {
        line.pop();
    }
    if line.len() > MAX_TSH_REPL_LINE_BYTES {
        return Err(TshError::usage("tsh input line exceeds limit"));
    }
    Ok(Some(line))
}

fn read_repl_key(input: &mut impl Read) -> Result<ReplKey, TshError> {
    let byte = read_byte(input)?;
    if byte != b'\x1b' {
        return Ok(ReplKey::Byte(byte));
    }

    let introducer = read_byte(input)?;
    if introducer != b'[' {
        return Ok(ReplKey::Byte(byte));
    }

    match read_byte(input)? {
        b'A' => Ok(ReplKey::Up),
        b'B' => Ok(ReplKey::Down),
        b'C' => Ok(ReplKey::Right),
        b'D' => Ok(ReplKey::Left),
        b'H' => Ok(ReplKey::Home),
        b'F' => Ok(ReplKey::End),
        b'1' | b'7' => {
            let _tilde = read_byte(input)?;
            Ok(ReplKey::Home)
        }
        b'4' | b'8' => {
            let _tilde = read_byte(input)?;
            Ok(ReplKey::End)
        }
        _ => Ok(ReplKey::Byte(byte)),
    }
}

fn read_byte(input: &mut impl Read) -> Result<u8, TshError> {
    let mut byte = [0u8; 1];
    input
        .read_exact(&mut byte)
        .map_err(|error| TshError::unavailable(format!("cannot read terminal input: {error}")))?;
    Ok(byte[0])
}

fn redraw_repl_line(prompt: &str, buffer: &[char], cursor: usize) -> Result<(), TshError> {
    let text: String = buffer.iter().collect();
    write_stdout(&format!("\r{prompt}{text}\x1b[K"))?;
    let right = buffer.len().saturating_sub(cursor);
    if right > 0 {
        write_stdout(&format!("\x1b[{right}D"))?;
    }
    Ok(())
}

fn push_history(history: &mut Vec<String>, line: &str) {
    if history.last().is_some_and(|entry| entry == line) {
        return;
    }
    history.push(line.to_owned());
}
