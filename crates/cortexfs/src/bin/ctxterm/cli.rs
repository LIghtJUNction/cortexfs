fn parse_args(args: Vec<OsString>) -> Result<CtxtermCommand, CtxtermError> {
    let mut values = args.into_iter();
    let mut listen = None;
    let mut log = None;
    let mut stdio = true;
    let Some(mut first) = values.next() else {
        return Ok(CtxtermCommand::Run {
            listen: None,
            log: None,
            stdio: true,
            program: OsString::from(DEFAULT_SHELL),
            args: Vec::new(),
        });
    };
    if first == "watch" || first == "attach" {
        let write = first == "attach";
        let Some(socket) = values.next() else {
            return Err(CtxtermError::usage("watch/attach requires a socket path"));
        };
        if let Some(extra) = values.next() {
            return Err(CtxtermError::usage(format!(
                "unexpected argument: {}",
                extra.to_string_lossy()
            )));
        }
        return Ok(CtxtermCommand::Client {
            socket: PathBuf::from(socket),
            write,
        });
    }
    if first == "--help" || first == "-h" {
        return Ok(CtxtermCommand::Help);
    }
    while first == "--listen" || first == "--log" || first == "--no-stdio" {
        match first.to_str() {
            Some("--listen") => {
                let Some(path) = values.next() else {
                    return Err(CtxtermError::usage("--listen requires a socket path"));
                };
                listen = Some(PathBuf::from(path));
            }
            Some("--log") => {
                let Some(path) = values.next() else {
                    return Err(CtxtermError::usage("--log requires a path"));
                };
                log = Some(PathBuf::from(path));
            }
            Some("--no-stdio") => {
                stdio = false;
            }
            _ => {}
        }
        let Some(next) = values.next() else {
            return Ok(CtxtermCommand::Run {
                listen,
                log,
                stdio,
                program: OsString::from(DEFAULT_SHELL),
                args: Vec::new(),
            });
        };
        first = next;
    }
    if first == "--" {
        let Some(program) = values.next() else {
            return Err(CtxtermError::usage("-- requires a command"));
        };
        return Ok(CtxtermCommand::Run {
            listen,
            log,
            stdio,
            program,
            args: values.collect(),
        });
    }
    Ok(CtxtermCommand::Run {
        listen,
        log,
        stdio,
        program: first,
        args: values.collect(),
    })
}

fn print_help() -> Result<(), CtxtermError> {
    write_stdout(
        "\
ctxterm - CortexFS agent terminal emulator

usage:
  ctxterm
  ctxterm --listen SOCKET [--log PATH] [--no-stdio] [-- COMMAND [ARG...]]
  ctxterm -- COMMAND [ARG...]
  ctxterm watch SOCKET
  ctxterm attach SOCKET

default:
  ctxterm starts /usr/bin/tsh
",
    )
}
