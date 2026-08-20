#![expect(
    clippy::redundant_pub_crate,
    reason = "private runtime channel context is shared by socket and executor layers"
)]

use std::collections::BTreeSet;
use std::path::Path;

use cortexfs_runtime_client::interaction::InteractionOrigin;

use super::channelcaps::{add_dir, read_caps, tool_name};
use crate::runtime::types::ChannelRuntimeError;
use crate::{ToolPath, is_object_name};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ChannelRuntimeContext {
    channel: String,
    conversation: Option<String>,
    caps: Vec<String>,
    tool_path: ToolPath,
    tools: BTreeSet<String>,
}

impl ChannelRuntimeContext {
    pub(crate) fn channel(&self) -> &str {
        &self.channel
    }
    pub(crate) fn conversation(&self) -> Option<&str> {
        self.conversation.as_deref()
    }
    pub(crate) fn caps(&self) -> String {
        self.caps.join(" ")
    }
    pub(crate) fn tool_path(&self) -> &ToolPath {
        &self.tool_path
    }
    pub(crate) fn is_channel_tool(&self, name: &str) -> bool {
        self.tools.contains(name)
    }
    pub(crate) fn allows_tool(&self, name: &str) -> bool {
        self.tools.contains(name)
            && self.caps.iter().any(|cap| {
                cap == "tool.*"
                    || cap == "tool:*"
                    || cap == &format!("tool.{name}")
                    || cap == &format!("tool:{name}")
            })
    }
}

pub(crate) fn base_tool_path(env: &[(String, String)], source: &Path, uid: u32) -> ToolPath {
    env.iter().find(|entry| entry.0 == "CTX_PATH").map_or_else(
        || {
            ToolPath::default(
                source,
                &cortexfs_paths::ctx_home_path(source, &uid.to_string()),
            )
        },
        |entry| ToolPath::parse(&entry.1),
    )
}

pub(crate) fn resolve(
    source: &Path,
    uid: u32,
    base: &ToolPath,
    origin: Option<&InteractionOrigin>,
) -> Result<Option<ChannelRuntimeContext>, ChannelRuntimeError> {
    let Some(origin) = origin else {
        return Ok(None);
    };
    if origin.transport != "channel" {
        return Ok(None);
    }
    let Some(channel) = origin.endpoint.as_deref() else {
        return Err(ChannelRuntimeError::InvalidOrigin);
    };
    if !is_object_name(channel) {
        return Err(ChannelRuntimeError::InvalidOrigin);
    }
    let mut dirs = Vec::new();
    add_dir(
        &mut dirs,
        &cortexfs_paths::home_channel_tool_path(source, &uid.to_string(), channel),
    )?;
    add_dir(
        &mut dirs,
        &cortexfs_paths::channel_tool_path(source, channel),
    )?;
    let channel_path = ToolPath::new(dirs.clone());
    let channel_hits = channel_path
        .list_limited(1024, 8192)
        .map_err(|_error| ChannelRuntimeError::InvalidDirectory)?;
    let base_hits = base
        .list_limited(4096, 16384)
        .map_err(|_error| ChannelRuntimeError::InvalidDirectory)?;
    let base_names = base_hits
        .iter()
        .filter_map(tool_name)
        .collect::<BTreeSet<_>>();
    let tools = channel_hits
        .iter()
        .filter_map(tool_name)
        .collect::<BTreeSet<_>>();
    if let Some(name) = tools.iter().find(|name| base_names.contains(*name)) {
        return Err(ChannelRuntimeError::ToolCollision((*name).clone()));
    }
    let mut all_dirs = dirs;
    all_dirs.extend(base.dirs().iter().cloned());
    let caps = read_caps(source, uid, channel)?;
    let mut context = ChannelRuntimeContext {
        channel: channel.to_owned(),
        conversation: origin.conversation.clone(),
        caps,
        tool_path: ToolPath::new(all_dirs),
        tools,
    };
    context
        .tools
        .retain(|name| channel_path.find(name).ok().flatten().is_some());
    Ok(Some(context))
}
