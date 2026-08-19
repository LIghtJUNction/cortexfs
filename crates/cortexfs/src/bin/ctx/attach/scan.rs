use super::ChannelRecord;
use super::records::{plain_directory, plain_file, read_channel, session_state};
use crate::*;
use cortexfs::runtime::channel::register_channel;
use cortexfs_runtime_client::interaction::InteractionOrigin;
use std::collections::HashSet;

pub(crate) fn visible_channels(root: &Path) -> Result<Vec<ChannelRecord>, CliError> {
    let mut channels = Vec::new();
    collect_agents(&ctx_home(root)?.join("agent"), false, &mut channels);
    collect_shared(root, &mut channels);
    channels.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(channels)
}

fn collect_shared(root: &Path, channels: &mut Vec<ChannelRecord>) {
    let Ok(spaces) = fs::read_dir(cortexfs_paths::shared_root_path(root)) else {
        return;
    };
    for space in spaces.flatten().filter(plain_directory) {
        collect_agents(&space.path().join("agent"), true, channels);
    }
}

fn collect_agents(agent_root: &Path, shared: bool, channels: &mut Vec<ChannelRecord>) {
    let Ok(agents) = fs::read_dir(agent_root) else {
        return;
    };
    for agent in agents.flatten().filter(plain_directory) {
        collect_agent(&agent.path(), shared, channels);
    }
}

fn collect_agent(agent: &Path, shared: bool, channels: &mut Vec<ChannelRecord>) {
    let Some(agent_name) = agent.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    let session_root = agent.join("session");
    let mut terminal_sessions = HashSet::new();
    let channel_root = cortexfs_paths::session_channel_index_path(&session_root);
    if let Ok(entries) = fs::read_dir(&channel_root) {
        for entry in entries.flatten().filter(plain_file) {
            if let Some(channel) = read_channel(&entry.path(), agent_name, shared) {
                if channel.transport == "terminal" {
                    terminal_sessions.insert(channel.session.clone());
                }
                channels.push(channel);
            }
        }
    }
    let Ok(sessions) = fs::read_dir(&session_root) else {
        return;
    };
    for session in sessions.flatten().filter(plain_directory) {
        let Some(session_name) = session.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if session_name == "index" || terminal_sessions.contains(&session_name) {
            continue;
        }
        let scope = if shared {
            SocketSessionScope::Shared
        } else {
            SocketSessionScope::Private
        };
        let origin = InteractionOrigin {
            transport: "terminal".to_owned(),
            ..InteractionOrigin::default()
        };
        if let Ok(path) = register_channel(&session_root, &session_name, scope, &origin) {
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("terminal");
            channels.push(ChannelRecord {
                name: name.to_owned(),
                agent: agent_name.to_owned(),
                session: session_name,
                transport: "terminal".to_owned(),
                state: session_state(&session.path()),
                shared,
            });
        }
    }
}
