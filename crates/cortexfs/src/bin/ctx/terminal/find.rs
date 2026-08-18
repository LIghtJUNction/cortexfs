use crate::*;

use cortexfs::runtime::terminal::{TerminalRecord, read_record};

pub(super) fn find_terminals(root: &Path) -> Result<Vec<(PathBuf, TerminalRecord)>, CliError> {
    let agents = cortexfs_paths::home_agent_root_from_home_path(&ctx_home(root)?);
    let mut output = Vec::new();
    let Ok(agent_entries) = fs::read_dir(&agents) else {
        return Ok(output);
    };
    for agent in agent_entries {
        let agent = read_terminal_entry(agent)?;
        if !plain_dir(&agent) {
            continue;
        }
        let Ok(session_entries) = fs::read_dir(agent.path().join("session")) else {
            continue;
        };
        for session in session_entries {
            let session = read_terminal_entry(session)?;
            if !plain_dir(&session) {
                continue;
            }
            let Ok(entries) = fs::read_dir(session.path().join("terminal")) else {
                continue;
            };
            for entry in entries {
                let entry = read_terminal_entry(entry)?;
                if plain_dir(&entry)
                    && let Ok(record) = read_record(&entry.path())
                {
                    output.push((entry.path(), record));
                }
            }
        }
    }
    output.sort_by(|left, right| left.1.id.cmp(&right.1.id));
    Ok(output)
}

fn read_terminal_entry(entry: io::Result<fs::DirEntry>) -> Result<fs::DirEntry, CliError> {
    entry.map_err(|error| CliError::unavailable(format!("cannot read terminals: {error}")))
}

fn plain_dir(entry: &fs::DirEntry) -> bool {
    entry
        .file_type()
        .is_ok_and(|kind| kind.is_dir() && !kind.is_symlink())
}
