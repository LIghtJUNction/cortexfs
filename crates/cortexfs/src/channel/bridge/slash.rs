use std::fs;
use std::path::{Path, PathBuf};

use crate::is_model_name;
use cortexfs_channels::{InboundMessage, MessageBody, OutboundMessage};
use cortexfs_runtime_client::status;

use super::AgentChannelBridge;

const HELP: &str = "/help — commands\n/models — list projected models\n/model — current session model\n/model PROVIDER/MODEL — host-side switch hint\n/new — start a fresh session for this conversation";

pub(super) fn reply(
    bridge: &AgentChannelBridge,
    inbound: &InboundMessage,
) -> Option<Result<OutboundMessage, super::ChannelBridgeError>> {
    let text = inbound.body.text.trim();
    let (command, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    let body = match command {
        "/help" => Some(HELP.to_owned()),
        "/new" => Some(bridge.rotate_session(inbound)),
        "/models" => Some(list_models()),
        "/model" if rest.is_empty() => Some(current_model(bridge, inbound)),
        "/model" => Some(switch_hint(rest.trim())),
        _ => None,
    }?;
    Some(outbound(inbound, body))
}

fn outbound(
    inbound: &InboundMessage,
    text: String,
) -> Result<OutboundMessage, super::ChannelBridgeError> {
    let mut target = inbound.target.clone();
    target.reply_to = Some(inbound.id.clone());
    Ok(OutboundMessage {
        target,
        body: MessageBody::text(text)?,
        metadata: inbound.metadata.clone(),
    })
}

fn current_model(bridge: &AgentChannelBridge, inbound: &InboundMessage) -> String {
    let session = bridge.session_for_inbound(inbound);
    match status::status(&bridge.socket, &session) {
        Ok(state) => format!(
            "session {session}\nmodel {}",
            state.model.as_deref().unwrap_or("unknown")
        ),
        Err(_error) => format!("session {session}\nmodel unknown"),
    }
}

fn switch_hint(model: &str) -> String {
    if !is_model_name(model) {
        return format!("invalid model name: {model}");
    }
    format!(
        "model {model} is projected when present under /ctx/model.\nset it on the agent with: ctx set agent/<name>.d/model {model}\nthen refresh storage and restart the agent unit"
    )
}

fn list_models() -> String {
    let root = std::env::var_os("CTX_ROOT").map_or_else(|| PathBuf::from("/ctx"), PathBuf::from);
    let mut names = projected_models(&root);
    if names.is_empty() {
        return "no projected models under /ctx/model".to_owned();
    }
    names.sort();
    names.join("\n")
}

fn projected_models(root: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let Ok(providers) = fs::read_dir(root.join("model")) else {
        return names;
    };
    for provider in providers.flatten() {
        let pname = provider.file_name();
        let Some(pname) = pname.to_str() else {
            continue;
        };
        if !crate::is_object_name(pname) {
            continue;
        }
        let Ok(models) = fs::read_dir(provider.path()) else {
            continue;
        };
        for model in models.flatten() {
            let mname = model.file_name();
            let Some(mname) = mname.to_str() else {
                continue;
            };
            if mname.ends_with(".d") || mname.ends_with(".sock") {
                continue;
            }
            let name = format!("{pname}/{mname}");
            if is_model_name(&name) {
                names.push(name);
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::{HELP, switch_hint};

    #[test]
    fn model_switch_hint_validates_names() {
        assert!(switch_hint("not-a-model").contains("invalid"));
        assert!(switch_hint("openrouter/gpt-5.6").contains("ctx set"));
        assert!(HELP.contains("/new"));
    }
}
