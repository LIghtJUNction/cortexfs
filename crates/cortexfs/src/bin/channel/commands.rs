use std::error::Error;

use super::config::CommandConfig;
use super::webhook::WebhookConfig;

mod bluesky;
mod common;
mod dingtalk;
mod discord;
mod driver;
mod email;
mod gmail;
mod irc;
mod matrix;
mod mattermost;
mod mochat;
mod notion;
mod qq;
mod reddit;
mod signal;
mod telegram;
mod twitch;
mod twitter;
mod web;
mod webhook;

pub(crate) fn run(config: CommandConfig) -> Result<(), Box<dyn Error>> {
    match config {
        CommandConfig::Discord { config } => discord::run(&config),
        CommandConfig::Telegram {
            common,
            token,
            api_base,
            poll_seconds,
        } => telegram::run(common, token, api_base, poll_seconds),
        CommandConfig::Bluesky {
            common,
            handle,
            app_password,
            api_base,
            poll_seconds,
        } => bluesky::run(common, handle, app_password, api_base, poll_seconds),
        CommandConfig::DingTalk {
            common,
            client_id,
            client_secret,
            gateway_url,
        } => dingtalk::run(common, client_id, client_secret, gateway_url),
        CommandConfig::Matrix {
            common,
            homeserver,
            access_token,
            rooms,
            sync_seconds,
        } => matrix::run(common, homeserver, access_token, rooms, sync_seconds),
        CommandConfig::Mattermost {
            common,
            base_url,
            token,
            channels,
            reconnect_seconds,
        } => mattermost::run(common, base_url, token, channels, reconnect_seconds),
        CommandConfig::Qq { common, config } => qq::run(common, &config),
        CommandConfig::Reddit { common, config } => reddit::run(common, &config),
        CommandConfig::Irc {
            common,
            server,
            port,
            nickname,
            channels,
            password,
        } => irc::run(common, server, port, nickname, channels, password),
        CommandConfig::Twitch { common, config } => twitch::run(common, &config),
        CommandConfig::Twitter { common, config } => twitter::run(common, &config),
        CommandConfig::Mochat { common, config } => mochat::run(common, &config),
        CommandConfig::Notion { common, config } => notion::run(common, &config),
        CommandConfig::Signal {
            common,
            account,
            executable,
        } => signal::run(common, account, executable),
        CommandConfig::Gmail {
            common,
            bind,
            path,
            access_token,
            api_base,
            token,
        } => gmail::run(common, bind, path, access_token, api_base, token),
        CommandConfig::Email { common, config } => email::run(common, &config),
        CommandConfig::Webhook {
            common,
            bind,
            path,
            platform,
            outbound_url,
            token,
            verify_token,
        } => {
            let channel = common.channel.clone();
            webhook::run(
                common,
                &WebhookConfig {
                    bind,
                    path,
                    platform,
                    outbound_url,
                    token,
                    verify_token,
                    channel,
                },
            )
        }
        CommandConfig::Web {
            common,
            bind,
            path,
            token,
        } => web::run(common, bind, path, token),
        CommandConfig::Driver {
            common,
            channel,
            socket,
        } => driver::run(common, channel, socket),
    }
}
