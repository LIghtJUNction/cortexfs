use super::profile::AgentProfile;
use crate::{CliError, is_model_name, is_object_name, read_small_plain_text_file};
use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::Path;
const MAX_EVE_SOURCE_BYTES: u64 = 64 * 1024;
const MAX_EVE_CAPABILITIES: usize = 32;
pub(crate) fn is_eve_project(path: &Path) -> bool {
    let agent = path.join("agent");
    agent.is_dir()
        && ["agent.ts", "instructions.md", "instructions.ts"]
            .iter()
            .any(|name| agent.join(name).is_file())
}
pub(crate) fn load_eve_profile(root: &Path) -> Result<AgentProfile, CliError> {
    let agent = root.join("agent");
    let instructions = match (
        agent.join("instructions.md").is_file(),
        agent.join("instructions.ts").is_file(),
    ) {
        (true, _) => Some(read_eve_file(&agent.join("instructions.md"))?),
        (false, true) => {
            return Err(CliError::usage(
                "Eve instructions.ts cannot be evaluated by ctx; add static agent/instructions.md",
            ));
        }
        (false, false) => None,
    };
    let model = eve_model(&agent)?;
    let capabilities = eve_capabilities(&agent);
    let name = root
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| is_object_name(value))
        .map(str::to_owned);
    let description = format!("Eve project import: {}{}", root.display(), capabilities);
    let note = "\n\nCortexFS Eve import is static and authority-bound. TypeScript tools, channels, hooks, schedules, and skills remain source files; use governed CortexFS tools through tsh.";
    Ok(AgentProfile {
        name,
        description: Some(description),
        instructions: Some(format!("{}{}", instructions.unwrap_or_default(), note)),
        models: model.into_iter().collect(),
        ..AgentProfile::default()
    })
}

fn read_eve_file(path: &Path) -> Result<String, CliError> {
    read_small_plain_text_file(path, MAX_EVE_SOURCE_BYTES, "Eve source").map_err(|error| {
        CliError::usage(format!(
            "cannot read Eve source {}: {error}",
            path.display()
        ))
    })
}
fn eve_model(agent: &Path) -> Result<Option<String>, CliError> {
    let path = ["agent.ts", "agent.js", "agent.mjs"]
        .iter()
        .map(|name| agent.join(name))
        .find(|path| path.is_file());
    let Some(path) = path else { return Ok(None) };
    for line in read_eve_file(&path)?
        .lines()
        .filter(|line| !line.trim_start().starts_with("//") && line.contains("model:"))
    {
        let Some((_, value)) = line.split_once(':') else {
            continue;
        };
        let Some((start, quote)) = value
            .char_indices()
            .find(|&(_, character)| matches!(character, '"' | '\''))
        else {
            continue;
        };
        let Some(tail) = value.get(start + quote.len_utf8()..) else {
            continue;
        };
        let Some(candidate) = tail.find(quote).and_then(|end| tail.get(..end)) else {
            continue;
        };
        if is_model_name(candidate) {
            return Ok(Some(candidate.to_owned()));
        }
    }
    Ok(None)
}
fn eve_capabilities(agent: &Path) -> String {
    let mut capabilities = String::new();
    for kind in ["tools", "skills", "channels", "subagents", "schedules"] {
        let _ignored = write!(capabilities, " {kind}={}", eve_stems(&agent.join(kind)));
    }
    capabilities
}

fn eve_stems(path: &Path) -> String {
    let mut names = fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .take(MAX_EVE_CAPABILITIES)
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let kind = entry.file_type().ok()?;
            if kind.is_symlink() {
                return None;
            }
            let file = entry.file_name();
            let path = Path::new(&file);
            let stem = path.file_stem()?.to_str()?;
            let extension = path.extension().and_then(|value| value.to_str());
            ((kind.is_dir()
                || extension
                    .is_some_and(|value| ["md", "ts", "tsx", "js", "mjs", "cjs"].contains(&value)))
                && is_object_name(stem))
            .then_some(stem.to_owned())
        })
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    format!("[{}]", names.join(","))
}
