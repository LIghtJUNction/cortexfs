fn run_repl(
    root: &Path,
    cache: &mut DynamicToolCache,
    context: &mut ToolContext,
) -> Result<ExitCode, TshError> {
    let mut history = Vec::new();
    loop {
        let Some(line) = read_repl_line("tsh> ", &history)? else {
            return Ok(ExitCode::SUCCESS);
        };
        let words = match parse_repl_line(&line) {
            Ok(words) => words,
            Err(error) => {
                report_repl_error(&error)?;
                continue;
            }
        };
        if words.is_empty() {
            continue;
        }
        push_history(&mut history, line.as_str());
        match words.first().map(String::as_str) {
            Some("exit" | "quit") => match parse_exit_code(&words) {
                Ok(code) => return Ok(code),
                Err(error) => report_repl_error(&error)?,
            },
            Some("help") => {
                if let Err(error) = repl_help(root, &words) {
                    report_repl_error(&error)?;
                }
            }
            Some("tools") => {
                if let Err(error) = repl_tools(root, &words) {
                    report_repl_error(&error)?;
                }
            }
            Some("which") => {
                if let Err(error) = repl_which(root, &words) {
                    report_repl_error(&error)?;
                }
            }
            Some("type") => {
                if let Err(error) = repl_type(root, &words) {
                    report_repl_error(&error)?;
                }
            }
            Some("command") => {
                if let Err(error) = repl_command(root, &words) {
                    report_repl_error(&error)?;
                }
            }
            Some("load") => {
                if let Err(error) = repl_load(root, cache, context, &words) {
                    report_repl_error(&error)?;
                }
            }
            Some("unload") => {
                if let Err(error) = repl_unload(root, cache, context, &words) {
                    report_repl_error(&error)?;
                }
            }
            Some("loads") => {
                if let Err(error) = repl_loads(context, &words) {
                    report_repl_error(&error)?;
                }
            }
            Some("pin") => {
                if let Err(error) = repl_pin(root, cache, context, &words) {
                    report_repl_error(&error)?;
                }
            }
            Some("unpin") => {
                if let Err(error) = repl_unpin(root, cache, context, &words) {
                    report_repl_error(&error)?;
                }
            }
            Some("pins") => {
                if let Err(error) = repl_pins(context, &words) {
                    report_repl_error(&error)?;
                }
            }
            Some(name) => {
                let args = words.iter().skip(1).map(OsString::from).collect::<Vec<_>>();
                if let Err(error) = run_repl_tool(root, context, name, args) {
                    report_repl_error(&error)?;
                }
            }
            None => {}
        }
    }
}

fn repl_help(root: &Path, words: &[String]) -> Result<(), TshError> {
    if words.len() == 1 {
        return print_help();
    }
    if words.len() != 2 {
        return write_stdout("tsh: help accepts at most one topic\n");
    }
    let Some(name) = words.get(1) else {
        return print_help();
    };
    if is_tsh_builtin(name) {
        print_builtin_help(name)
    } else {
        print_tool_help(root, name)
    }
}

fn run_builtin_once(
    root: &Path,
    cache: &mut DynamicToolCache,
    context: &mut ToolContext,
    name: &str,
    args: Vec<OsString>,
) -> Result<ExitCode, TshError> {
    let words = builtin_words(name, args)?;
    match name {
        "exit" | "quit" => parse_exit_code(&words),
        "help" => repl_help(root, &words).map(|()| ExitCode::SUCCESS),
        "tools" => repl_tools(root, &words).map(|()| ExitCode::SUCCESS),
        "which" => repl_which(root, &words).map(|()| ExitCode::SUCCESS),
        "type" => repl_type(root, &words).map(|()| ExitCode::SUCCESS),
        "command" => repl_command(root, &words).map(|()| ExitCode::SUCCESS),
        "load" => repl_load(root, cache, context, &words).map(|()| ExitCode::SUCCESS),
        "unload" => repl_unload(root, cache, context, &words).map(|()| ExitCode::SUCCESS),
        "loads" => repl_loads(context, &words).map(|()| ExitCode::SUCCESS),
        "pin" => repl_pin(root, cache, context, &words).map(|()| ExitCode::SUCCESS),
        "unpin" => repl_unpin(root, cache, context, &words).map(|()| ExitCode::SUCCESS),
        "pins" => repl_pins(context, &words).map(|()| ExitCode::SUCCESS),
        _ => command_not_found(name),
    }
}

fn builtin_words(name: &str, args: Vec<OsString>) -> Result<Vec<String>, TshError> {
    let mut words = Vec::with_capacity(args.len() + 1);
    words.push(name.to_owned());
    for arg in args {
        words.push(os_string(arg)?);
    }
    Ok(words)
}

fn repl_tools(root: &Path, words: &[String]) -> Result<(), TshError> {
    if words.len() == 1 {
        return list_tools_with_mode(root, ToolListMode::Names);
    }
    if words.len() == 2
        && words
            .get(1)
            .is_some_and(|flag| flag == "-l" || flag == "--long")
    {
        return list_tools_with_mode(root, ToolListMode::Long);
    }
    write_stdout("tsh: tools accepts only -l/--long\n")
}

fn repl_which(root: &Path, words: &[String]) -> Result<(), TshError> {
    if words.len() == 2 {
        let Some(name) = words.get(1) else {
            return write_stdout("tsh: which requires a tool name\n");
        };
        print_tool_path(root, name)
    } else if words.len() == 1 {
        write_stdout("tsh: which requires a tool name\n")
    } else {
        write_stdout("tsh: which accepts one tool name\n")
    }
}

fn repl_type(root: &Path, words: &[String]) -> Result<(), TshError> {
    if words.len() == 1 {
        return write_stdout("tsh: type requires a tool name\n");
    }
    if words.len() != 2 {
        return write_stdout("tsh: type accepts one tool name\n");
    }
    let Some(name) = words.get(1) else {
        return write_stdout("tsh: type requires a tool name\n");
    };
    print_command_type(root, name)
}

fn repl_command(root: &Path, words: &[String]) -> Result<(), TshError> {
    if words.len() == 3 && words.get(1).is_some_and(|flag| flag == "-v") {
        let Some(name) = words.get(2) else {
            return write_stdout("tsh: command supports only `command -v TOOL`\n");
        };
        return print_command_v(root, name);
    }
    write_stdout("tsh: command supports only `command -v TOOL`\n")
}

fn repl_load(
    root: &Path,
    cache: &mut DynamicToolCache,
    context: &mut ToolContext,
    words: &[String],
) -> Result<(), TshError> {
    if words.len() != 2 {
        return write_stdout("tsh: load requires one tool name\n");
    }
    let Some(name) = words.get(1) else {
        return write_stdout("tsh: load requires one tool name\n");
    };
    let mut loaded = load_tool_context(root, name, false)?;
    cache.load_path(&loaded.path);
    loaded.dynamic_resident = cache.contains_path(&loaded.path);
    let loaded_name = loaded.name.clone();
    let path = loaded.path.clone();
    let dynamic_resident = loaded.dynamic_resident;
    let evicted = context.insert(loaded);
    let state = if dynamic_resident {
        "metadata+resident"
    } else {
        "metadata"
    };
    write_stdout(&format!(
        "loaded {loaded_name}\t{}\t{state}\n",
        path.display()
    ))?;
    report_context_evictions(evicted)
}

fn repl_unload(
    root: &Path,
    cache: &mut DynamicToolCache,
    context: &mut ToolContext,
    words: &[String],
) -> Result<(), TshError> {
    if words.len() != 2 {
        return write_stdout("tsh: unload requires one tool name\n");
    }
    let Some(name) = words.get(1) else {
        return write_stdout("tsh: unload requires one tool name\n");
    };
    let loaded = context.remove_unpinned(name)?;
    let Some(loaded) = loaded else {
        return write_stdout(&format!("{name} is not loaded\n"));
    };
    let hit = resolve_tool_hit(root, name)?;
    let _was_pinned = cache.unpin_path(hit.path());
    write_stdout(&format!(
        "unloaded {}\t{}\n",
        loaded.name,
        loaded.path.display()
    ))
}

fn repl_loads(context: &ToolContext, words: &[String]) -> Result<(), TshError> {
    if words.len() != 1 {
        return write_stdout("tsh: loads does not accept arguments\n");
    }
    print_loaded_tools(context.values())
}

fn repl_pin(
    root: &Path,
    cache: &mut DynamicToolCache,
    context: &mut ToolContext,
    words: &[String],
) -> Result<(), TshError> {
    if words.len() != 2 {
        return write_stdout("tsh: pin requires one tool name\n");
    }
    let Some(name) = words.get(1) else {
        return write_stdout("tsh: pin requires one tool name\n");
    };
    let mut loaded = load_tool_context(root, name, true)?;
    cache.pin_path(&loaded.path);
    loaded.dynamic_resident = cache.contains_path(&loaded.path);
    let loaded_name = loaded.name.clone();
    let path = loaded.path.clone();
    let dynamic_resident = loaded.dynamic_resident;
    let evicted = context.insert(loaded);
    let state = if dynamic_resident {
        "pinned metadata+resident"
    } else {
        "pinned metadata"
    };
    write_stdout(&format!("{state} {loaded_name}\t{}\n", path.display()))?;
    report_context_evictions(evicted)
}

fn repl_unpin(
    root: &Path,
    cache: &mut DynamicToolCache,
    context: &mut ToolContext,
    words: &[String],
) -> Result<(), TshError> {
    if words.len() != 2 {
        return write_stdout("tsh: unpin requires one tool name\n");
    }
    let Some(name) = words.get(1) else {
        return write_stdout("tsh: unpin requires one tool name\n");
    };
    let hit = resolve_tool_hit(root, name)?;
    let memory_unpinned = cache.unpin_path(hit.path());
    if let Some(loaded) = context.get_mut(name) {
        loaded.pinned = false;
        if !memory_unpinned {
            loaded.dynamic_resident = false;
        }
        write_stdout(&format!("unpinned {name}\t{}\n", hit.path().display()))
    } else {
        write_stdout(&format!("{name} is not loaded\n"))
    }
}

fn repl_pins(context: &ToolContext, words: &[String]) -> Result<(), TshError> {
    if words.len() != 1 {
        return write_stdout("tsh: pins does not accept arguments\n");
    }
    print_loaded_tools(context.pinned_values())
}

fn print_loaded_tools<'a>(tools: impl Iterator<Item = &'a LoadedTool>) -> Result<(), TshError> {
    let mut stdout = io::stdout().lock();
    for tool in tools {
        let state = match (tool.pinned, tool.dynamic_resident) {
            (true, true) => "pinned,resident",
            (true, false) => "pinned",
            (false, true) => "resident",
            (false, false) => "metadata",
        };
        if tool.description.is_empty() {
            writeln!(stdout, "{}\t{}\t{state}", tool.name, tool.path.display())
                .map_err(|error| write_error_to_tsh(&error))?;
        } else {
            writeln!(
                stdout,
                "{}\t{}\t{state}\t{}",
                tool.name,
                tool.path.display(),
                tool.description
            )
            .map_err(|error| write_error_to_tsh(&error))?;
        }
    }
    stdout.flush().map_err(|error| write_error_to_tsh(&error))
}

fn print_tool_path(root: &Path, name: &str) -> Result<(), TshError> {
    let tool_path = ctx_tool_path(root)?;
    let Some(hit) = tool_path.find(name).map_err(tool_path_error)? else {
        return write_stdout(&format!(
            "tsh: tool not found in CTX_PATH: {name}\ntry: tools\n"
        ));
    };
    write_stdout(&format!("{}\n", hit.path().display()))
}

fn print_command_type(root: &Path, name: &str) -> Result<(), TshError> {
    if is_tsh_builtin(name) {
        return write_stdout(&format!("{name} is a tsh builtin\n"));
    }
    let tool_path = ctx_tool_path(root)?;
    let Some(hit) = tool_path.find(name).map_err(tool_path_error)? else {
        return command_not_found(name);
    };
    write_stdout(&format!("{name} is {}\n", hit.path().display()))
}

fn print_command_v(root: &Path, name: &str) -> Result<(), TshError> {
    if is_tsh_builtin(name) {
        return write_stdout(&format!("{name}\n"));
    }
    print_tool_path(root, name)
}

fn print_builtin_help(name: &str) -> Result<(), TshError> {
    let text = match name {
        "exit" | "quit" => "exit [CODE]\n  leave tsh\n",
        "help" => "help [TOOL]\n  show tsh help or visible tool metadata\n",
        "tools" => "tools [-l]\n  list visible tools from CTX_PATH\n",
        "which" => "which TOOL\n  print the resolved tool path\n",
        "type" => "type TOOL\n  show whether TOOL is a tsh builtin or visible tool\n",
        "command" => "command -v TOOL\n  print the command that tsh would run\n",
        "load" => "load TOOL\n  load tool metadata into this tsh context\n",
        "unload" => "unload TOOL\n  remove unpinned tool metadata from this tsh context\n",
        "loads" => "loads\n  list loaded tool context entries\n",
        "pin" => "pin TOOL\n  load TOOL metadata and keep it from context eviction\n",
        "unpin" => "unpin TOOL\n  allow a pinned tool to be unloaded from context again\n",
        "pins" => "pins\n  list pinned tool context entries\n",
        _ => "unknown builtin\n",
    };
    write_stdout(text)
}

fn print_tool_help(root: &Path, name: &str) -> Result<(), TshError> {
    let tool_path = ctx_tool_path(root)?;
    let Some(hit) = tool_path.find(name).map_err(tool_path_error)? else {
        return command_not_found(name);
    };
    let description = tool_description(&hit);
    let schema = tool_schema(&hit);
    let mut text = format!("{name}\n  path: {}\n", hit.path().display());
    if !description.is_empty() {
        let _ignored = writeln!(text, "  description: {description}");
    }
    if let Some(schema) = schema {
        append_schema_help(&mut text, &schema);
    }
    write_stdout(&text)
}

fn run_repl_tool(
    root: &Path,
    context: &mut ToolContext,
    name: &str,
    args: Vec<OsString>,
) -> Result<ExitCode, TshError> {
    let tool_path = ctx_tool_path(root)?;
    if tool_path.find(name).map_err(tool_path_error)?.is_none() {
        return command_not_found(name);
    }
    if args.len() == 1
        && matches!(
            args.first().and_then(|arg| arg.to_str()),
            Some("-h" | "--help")
        )
    {
        return print_tool_help(root, name).map(|()| ExitCode::SUCCESS);
    }
    if args.is_empty() && requires_explicit_repl_input(name) {
        write_stdout(&format!(
            "tsh: {name} needs input; pass arguments instead of leaving stdin open\ntry: {name} PATH or {name} '{{\"path\":\"PATH\"}}'\n"
        ))?;
        return Ok(ExitCode::from(2));
    }
    context.touch(name);
    run_tool(root, name, args)
}
