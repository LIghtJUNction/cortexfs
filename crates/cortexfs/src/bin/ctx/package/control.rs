use super::manifest::PackageAgent;
use crate::*;
use std::collections::BTreeMap;

pub(super) fn agent_controls(agent: &PackageAgent) -> Result<BTreeMap<String, String>, CliError> {
    let uid = nix::unistd::Uid::effective().as_raw().to_string();
    let gid = nix::unistd::Gid::effective().as_raw().to_string();
    let model = agent.model.as_deref().unwrap_or("main");
    let subject = format!("{}_t", agent.name);
    Ok(BTreeMap::from([
        ("owner".to_owned(), uid.clone()),
        ("uid".to_owned(), uid.clone()),
        ("gid".to_owned(), gid),
        ("groups".to_owned(), current_supplementary_groups_control()?),
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
