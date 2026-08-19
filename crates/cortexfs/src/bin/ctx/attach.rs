use crate::*;

mod records;
mod scan;

use scan::visible_channels;

/// Attachable frontend/session entry shown by `ctx attach`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ChannelRecord {
    pub(crate) name: String,
    pub(crate) agent: String,
    pub(crate) session: String,
    pub(crate) transport: String,
    pub(crate) state: String,
    pub(crate) shared: bool,
}

pub(crate) fn channel_attach(root: &Path, selector: Option<&str>) -> Result<ExitCode, CliError> {
    let channels = visible_channels(root)?;
    let Some(channel) = select_channel(&channels, selector) else {
        if channels.is_empty() {
            return Err(CliError::unavailable("no attachable channels are visible"));
        }
        let header = selector.map_or_else(
            || "Please specify a channel to attach to.".to_owned(),
            |value| format!("No channel matched '{value}'. Please specify a channel."),
        );
        print_channel_list(&header, &channels)?;
        return Ok(ExitCode::from(2));
    };
    if channel.transport == "terminal" {
        agent_terminal(root, &channel.agent, Some(&channel.session), true)
    } else {
        agent_chat(root, &channel.agent, Some(&channel.session), false, &[])
    }
}

fn select_channel<'a>(
    channels: &'a [ChannelRecord],
    selector: Option<&str>,
) -> Option<&'a ChannelRecord> {
    let Some(selector) = selector else {
        return if channels.len() == 1 {
            channels.first()
        } else {
            None
        };
    };
    if let Some(channel) = channels.iter().find(|channel| channel.name == selector) {
        return Some(channel);
    }
    let matches = channels
        .iter()
        .filter(|channel| channel.name.starts_with(selector))
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        matches.into_iter().next()
    } else {
        None
    }
}

fn print_channel_list(header: &str, channels: &[ChannelRecord]) -> Result<(), CliError> {
    print_line(header)?;
    print_line("The following channels are visible:")?;
    for channel in channels {
        let shared = if channel.shared { " (SHARED)" } else { "" };
        print_line(&format!(
            "{} [transport={} state={}]{} agent={} session={}",
            terminal_safe_text(&channel.name),
            terminal_safe_text(&channel.transport),
            terminal_safe_text(&channel.state),
            shared,
            terminal_safe_text(&channel.agent),
            terminal_safe_text(&channel.session),
        ))?;
    }
    Ok(())
}
