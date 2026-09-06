use cortexfs_channels::platform::catalog::find;
use cortexfs_channels::{CHANNEL_CATALOG, ChannelTransport};
use std::error::Error;
use std::io::{self, Write};

use super::config::{CatalogAction, CommandConfig, ConfigError};

mod setup;
use setup::lookup;

pub(crate) fn load_action(
    action: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<CommandConfig, ConfigError> {
    match action {
        "list" if args.next().is_none() => Ok(CommandConfig::Catalog(CatalogAction::List)),
        "show" => {
            let family = args.next().ok_or(ConfigError::Usage)?;
            args.next()
                .is_none()
                .then_some(CommandConfig::Catalog(CatalogAction::Show { family }))
                .ok_or(ConfigError::Usage)
        }
        "preset" => {
            let family = args.next().ok_or(ConfigError::Usage)?;
            args.next()
                .is_none()
                .then_some(CommandConfig::Catalog(CatalogAction::Preset { family }))
                .ok_or(ConfigError::Usage)
        }
        _ => Err(ConfigError::Usage),
    }
}

pub(crate) fn run(action: CatalogAction) -> Result<(), Box<dyn Error>> {
    match action {
        CatalogAction::List => list(),
        CatalogAction::Show { family } => show(&family, false),
        CatalogAction::Preset { family } => show(&family, true),
    }
}

fn list() -> Result<(), Box<dyn Error>> {
    let mut out = io::stdout().lock();
    for spec in CHANNEL_CATALOG {
        let host = lookup(spec.id).map_or("cortexfs-channel driver", |setup| setup.command);
        let kind = if spec.native { "native" } else { "driver" };
        writeln!(
            out,
            "{}\t{}\t{kind}\t{host}",
            spec.id,
            transport(spec.transport)
        )?;
    }
    Ok(())
}

fn show(family: &str, preset: bool) -> Result<(), Box<dyn Error>> {
    let spec = find(family).ok_or_else(|| format!("unknown channel family: {family}"))?;
    let setup = lookup(spec.id);
    let mut out = io::stdout().lock();
    if preset {
        if spec.id == "discord" {
            write_discord_preset(&mut out)?;
            return Ok(());
        }
        writeln!(out, "# /etc/cortexfs/channels/{family}.env")?;
        writeln!(out, "CORTEXFS_AGENT=executor")?;
        writeln!(out, "CORTEXFS_AGENT_SOCKET=/ctx/agent/executor.sock")?;
        writeln!(out, "CORTEXFS_CHANNEL_ID={family}.primary")?;
        writeln!(
            out,
            "# Required: exact sender IDs, comma-separated; empty denies everyone"
        )?;
        writeln!(out, "CORTEXFS_CHANNEL_ALLOWED_SENDERS=")?;
        if spec.id == "slack" {
            writeln!(
                out,
                "# Copy agent, socket, channel ID, and allowed senders above to /etc/cortexfs/channels/{family}-driver.env"
            )?;
        }
        if let Some(setup) = setup {
            for secret in setup.secrets {
                if secret.contains('=') {
                    writeln!(out, "{secret}")?;
                } else {
                    writeln!(out, "{secret}=")?;
                }
            }
        }
        return Ok(());
    }
    writeln!(out, "family\t{}", spec.id)?;
    writeln!(out, "transport\t{}", transport(spec.transport))?;
    writeln!(
        out,
        "host\t{}",
        if spec.native { "native" } else { "driver" }
    )?;
    if let Some(setup) = setup {
        writeln!(out, "command\t{}", setup.command)?;
        writeln!(out, "unit\t{}", setup.unit)?;
        writeln!(out, "secrets\t{}", setup.secrets.join(","))?;
        if spec.id == "slack" {
            writeln!(
                out,
                "driver_env\t/etc/cortexfs/channels/{family}-driver.env"
            )?;
        }
    }
    Ok(())
}

fn write_discord_preset(out: &mut impl Write) -> Result<(), Box<dyn Error>> {
    writeln!(out, "# /etc/cortexfs/channels/discord.toml")?;
    writeln!(out, "application_id = \"DISCORD_APPLICATION_ID\"")?;
    writeln!(out, "bot_token = \"DISCORD_BOT_TOKEN\"")?;
    writeln!(out, "agent_socket = \"/ctx/agent/executor.sock\"")?;
    writeln!(out, "agent = \"executor\"")?;
    writeln!(out, "session_prefix = \"discord\"")?;
    writeln!(out, "# Required: Discord user IDs; empty denies everyone")?;
    writeln!(out, "allowed_senders = []")?;
    Ok(())
}

fn transport(value: ChannelTransport) -> &'static str {
    match value {
        ChannelTransport::Polling => "polling",
        ChannelTransport::Webhook => "webhook",
        ChannelTransport::WebSocket => "websocket",
        ChannelTransport::Stdio => "stdio",
        ChannelTransport::LocalApi => "local",
        ChannelTransport::External => "external",
    }
}

#[cfg(test)]
mod tests {
    use super::setup::{SETUPS, lookup};
    use super::write_discord_preset;

    #[test]
    fn catalog_setup_and_presets() -> Result<(), Box<dyn std::error::Error>> {
        for id in ["telegram", "discord", "slack", "feishu", "matrix"] {
            assert!(lookup(id).is_some(), "{id}");
        }
        assert!(SETUPS.iter().any(|setup| setup.id == "telegram"));
        assert!(lookup("slack").is_some_and(|setup| setup.command == "cortexfs-channel-slack"));
        let mut buf = Vec::new();
        write_discord_preset(&mut buf)?;
        let text = String::from_utf8(buf)?;
        assert!(text.contains("bot_token"));
        Ok(())
    }
}
