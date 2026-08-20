use cortexfs_channels::MessageTarget;
use reqwest::blocking::Client;
use serde_json::Value;

use super::{DiscordConfig, DiscordError, command, component, embed, thread, upload};

pub(super) fn run(
    client: &Client,
    config: &DiscordConfig,
    target: &MessageTarget,
    command_id: &str,
    name: &str,
    payload: &Value,
) -> Result<Value, DiscordError> {
    let channel = target.conversation.as_str();
    if !channel.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(DiscordError::Invalid("conversation"));
    }
    match name {
        "discord.send_embed" => embed::send(client, config, channel, command_id, payload),
        "discord.send_file" => upload::send(client, config, channel, command_id, payload),
        "discord.create_thread" => thread::create(client, config, channel, payload),
        "discord.send_component" => component::send(client, config, channel, command_id, payload),
        "discord.register_command" => command::register(client, config, payload),
        "discord.autocomplete" => command::autocomplete(client, config, payload),
        "discord.gate_prompt" => command::gate_prompt(client, config, channel, command_id, payload),
        "discord.gate_finalize" => command::gate_finalize(client, config, channel, payload),
        "discord.draft_update" => command::draft_update(client, config, channel, payload),
        "discord.invoke" => invoke(client, config, target, command_id, payload),
        _ => Err(DiscordError::Invalid("unsupported operation")),
    }
}

fn invoke(
    client: &Client,
    config: &DiscordConfig,
    target: &MessageTarget,
    command_id: &str,
    payload: &Value,
) -> Result<Value, DiscordError> {
    let name = payload
        .get("operation")
        .and_then(Value::as_str)
        .filter(|value| value.starts_with("discord."))
        .ok_or(DiscordError::Invalid("operation"))?;
    let args = payload.get("payload").unwrap_or(payload);
    run(client, config, target, command_id, name, args)
}
