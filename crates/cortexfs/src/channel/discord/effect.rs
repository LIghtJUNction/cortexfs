use reqwest::blocking::{Client, RequestBuilder};

use super::{DiscordConfig, DiscordError};

pub(super) fn auth(request: RequestBuilder, config: &DiscordConfig) -> RequestBuilder {
    request.header(
        reqwest::header::AUTHORIZATION,
        format!("Bot {}", config.bot_token),
    )
}

pub(super) fn channel_url(config: &DiscordConfig, channel: &str, suffix: &str) -> String {
    format!(
        "{}/channels/{channel}/{suffix}",
        config.api_base.trim_end_matches('/')
    )
}

pub(super) fn react(
    client: &Client,
    config: &DiscordConfig,
    channel: &str,
    message: &str,
    emoji: &str,
) -> Result<(), DiscordError> {
    auth(
        client.put(channel_url(
            config,
            channel,
            &format!("messages/{message}/reactions/{emoji}/@me"),
        )),
        config,
    )
    .send()
    .map_err(DiscordError::Http)?
    .error_for_status()
    .map_err(DiscordError::Http)?;
    Ok(())
}

pub(super) fn typing(
    client: &Client,
    config: &DiscordConfig,
    channel: &str,
) -> Result<(), DiscordError> {
    auth(client.post(channel_url(config, channel, "typing")), config)
        .send()
        .map_err(DiscordError::Http)?
        .error_for_status()
        .map_err(DiscordError::Http)?;
    Ok(())
}

pub(super) fn remove(
    client: &Client,
    config: &DiscordConfig,
    channel: &str,
    message: &str,
    emoji: &str,
) -> Result<(), DiscordError> {
    auth(
        client.delete(channel_url(
            config,
            channel,
            &format!("messages/{message}/reactions/{emoji}/@me"),
        )),
        config,
    )
    .send()
    .map_err(DiscordError::Http)?
    .error_for_status()
    .map_err(DiscordError::Http)?;
    Ok(())
}

pub(super) fn pin(
    client: &Client,
    config: &DiscordConfig,
    channel: &str,
    message: &str,
) -> Result<(), DiscordError> {
    auth(
        client.put(channel_url(config, channel, &format!("pins/{message}"))),
        config,
    )
    .send()
    .map_err(DiscordError::Http)?
    .error_for_status()
    .map_err(DiscordError::Http)?;
    Ok(())
}

pub(super) fn unpin(
    client: &Client,
    config: &DiscordConfig,
    channel: &str,
    message: &str,
) -> Result<(), DiscordError> {
    auth(
        client.delete(channel_url(config, channel, &format!("pins/{message}"))),
        config,
    )
    .send()
    .map_err(DiscordError::Http)?
    .error_for_status()
    .map_err(DiscordError::Http)?;
    Ok(())
}
