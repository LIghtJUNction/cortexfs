use super::manifest::{PackageAgent, PackageTool};
use crate::{
    CliError, DEFAULT_AGENT_PROMPT_TEMPLATE, agent_new_mount_control, agent_new_policy,
    current_supplementary_groups_control,
};
use std::collections::BTreeMap;

pub(super) fn agent_controls(agent: &PackageAgent) -> Result<BTreeMap<String, String>, CliError> {
    let (uid, gid, groups) = if let Some(identity) = agent.identity.as_ref() {
        (
            identity.uid.to_string(),
            identity.gid.to_string(),
            identity
                .groups
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
    } else {
        (
            nix::unistd::Uid::effective().as_raw().to_string(),
            nix::unistd::Gid::effective().as_raw().to_string(),
            current_supplementary_groups_control()?,
        )
    };
    let model = agent.model.as_deref().unwrap_or("main");
    let subject = format!("{}_t", agent.name);
    let permissions = cortexfs::AgentPermissions::for_tools(agent.tools.iter().map(String::as_str));
    Ok(BTreeMap::from([
        ("owner".to_owned(), uid.clone()),
        ("uid".to_owned(), uid.clone()),
        ("gid".to_owned(), gid),
        ("groups".to_owned(), groups),
        (
            "perm".to_owned(),
            permissions.control().trim_end().to_owned(),
        ),
        ("label".to_owned(), format!("user_u:agent_r:{subject}:s0")),
        ("iso".to_owned(), "shared".to_owned()),
        (
            "parent".to_owned(),
            agent
                .parent
                .clone()
                .unwrap_or_else(|| "agent:architect".to_owned()),
        ),
        ("life".to_owned(), "owned".to_owned()),
        (
            "root".to_owned(),
            format!("/ctx/home/{uid}/agent/{}/root", agent.name),
        ),
        ("cwd".to_owned(), "/workspace".to_owned()),
        ("env".to_owned(), "CTX_ROOT=/ctx".to_owned()),
        ("path".to_owned(), format!("/ctx/tool:/ctx/home/{uid}/tool")),
        (
            "mount".to_owned(),
            agent_new_mount_control(&uid, &agent.name, &[]),
        ),
        ("model".to_owned(), model.to_owned()),
        (
            "abi".to_owned(),
            cortexfs_runtime_client::agent::AGENT_LAUNCH_ABI.to_owned(),
        ),
        ("tools".to_owned(), agent.tools.join("\n")),
        (
            "system.md".to_owned(),
            agent.instructions.clone().unwrap_or_default(),
        ),
        (
            "prompt.template.md".to_owned(),
            DEFAULT_AGENT_PROMPT_TEMPLATE.to_owned(),
        ),
        (
            "policy".to_owned(),
            agent_new_policy(&subject, model, &agent.tools),
        ),
        (
            "meta.json".to_owned(),
            serde_json::json!({"description": agent.description, "source": "package"}).to_string(),
        ),
    ]))
}

pub(super) fn tool_controls(
    tool: &PackageTool,
    policy: Option<&String>,
) -> Result<BTreeMap<String, String>, CliError> {
    let schema = tool
        .schema
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| CliError::unavailable(format!("cannot encode tool schema: {error}")))?
        .unwrap_or_else(|| "{}".to_owned());
    Ok(BTreeMap::from([
        (
            "description".to_owned(),
            tool.description
                .clone()
                .unwrap_or_else(|| tool.name.clone()),
        ),
        ("schema".to_owned(), schema),
        (
            "cap".to_owned(),
            tool.cap.clone().unwrap_or_else(|| "text".to_owned()),
        ),
        ("policy".to_owned(), policy.cloned().unwrap_or_default()),
    ]))
}
