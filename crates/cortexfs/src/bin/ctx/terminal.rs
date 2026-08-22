use crate::*;

use cortexfs::runtime::terminal::{TerminalRecord, terminal_id};

mod find;

use find::find_terminals;

pub(crate) fn terminal_action(root: &Path, args: &TerminalArgs) -> Result<ExitCode, CliError> {
    match *args {
        TerminalArgs::Create {
            ref agent,
            ref session,
            ref cwd,
        } => {
            let start = AgentStartArgs {
                name: agent.clone(),
                session: session.clone(),
                cwd: cwd.clone(),
                default_workspace: true,
                mounts: Vec::new(),
            };
            let _receipt = agent_start_host(root, &start)?;
            print_line(&terminal_id(agent, session))?;
            Ok(ExitCode::SUCCESS)
        }
        TerminalArgs::List => terminal_list(root),
        TerminalArgs::Status { ref id } => terminal_status(root, id),
        TerminalArgs::Watch { ref id } => terminal_attach(root, id, false),
        TerminalArgs::Attach { ref id } => terminal_attach(root, id, true),
    }
}

fn terminal_list(root: &Path) -> Result<ExitCode, CliError> {
    for (directory, record) in find_terminals(root)? {
        print_line(&format!(
            "{}\t{}\t{}\t{}\t{}",
            terminal_safe_text(&record.id),
            terminal_safe_text(&record.state),
            terminal_safe_text(&record.agent),
            terminal_safe_text(&record.session),
            directory.display()
        ))?;
    }
    Ok(ExitCode::SUCCESS)
}

fn terminal_status(root: &Path, id: &str) -> Result<ExitCode, CliError> {
    let (directory, record) = find_terminal(root, id)?;
    let body = serde_json::to_string_pretty(&record).map_err(|error| {
        CliError::unavailable(format!("cannot render terminal status: {error}"))
    })?;
    print_terminal_text(&format!("path={}\n{body}", directory.display()))?;
    Ok(ExitCode::SUCCESS)
}

fn terminal_attach(root: &Path, id: &str, write: bool) -> Result<ExitCode, CliError> {
    let (_directory, record) = find_terminal(root, id)?;
    let socket = agent_terminal_socket(root, &record.agent, &record.session)?;
    stream_terminal_socket(&socket, write, &record.agent, &record.session)
}

fn find_terminal(root: &Path, id: &str) -> Result<(PathBuf, TerminalRecord), CliError> {
    find_terminals(root)?
        .into_iter()
        .find(|entry| entry.1.id == id)
        .ok_or_else(|| CliError::unavailable(format!("terminal is not found: {id}")))
}
