fn is_tsh_builtin(name: &str) -> bool {
    matches!(
        name,
        "exit"
            | "quit"
            | "help"
            | "tools"
            | "which"
            | "type"
            | "command"
            | "load"
            | "unload"
            | "loads"
            | "pin"
            | "unpin"
            | "pins"
    )
}

fn requires_explicit_repl_input(name: &str) -> bool {
    matches!(name, "fs.read" | "fs.write" | "shell.exec")
}

fn parse_exit_code(words: &[String]) -> Result<ExitCode, TshError> {
    match *words {
        [_] => Ok(ExitCode::SUCCESS),
        [_, ref code] => {
            let code = code
                .parse::<u8>()
                .map_err(|_error| TshError::usage("exit code must be 0..255"))?;
            Ok(ExitCode::from(code))
        }
        _ => Err(TshError::usage("exit accepts at most one code")),
    }
}

fn parse_repl_line(line: &str) -> Result<Vec<String>, TshError> {
    parse_shell_words!(
        line.trim_end_matches(['\n', '\r']),
        TshError::usage("line ends with unfinished escape"),
        TshError::usage("line has unterminated quote")
    )
}
