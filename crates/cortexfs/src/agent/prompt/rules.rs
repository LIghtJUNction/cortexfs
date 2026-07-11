use super::read::{push_str_byte_limit, read_bounded_regular_utf8};
use super::*;
use crate::*;
use std::env;

#[must_use]
pub fn collect_agent_rules() -> String {
    let mut paths = Vec::new();
    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        paths.push(home.join(".codex").join("AGENTS.md"));
        paths.push(home.join(".agents").join("AGENTS.md"));
        paths.push(home.join("AGENTS.md"));
    }
    paths.push(PathBuf::from("/etc/cortexfs/AGENTS.md"));
    if let Ok(cwd) = env::current_dir() {
        let mut ancestors = cwd.ancestors().map(Path::to_path_buf).collect::<Vec<_>>();
        ancestors.reverse();
        paths.extend(ancestors.into_iter().map(|path| path.join("AGENTS.md")));
    }

    collect_agent_rules_from_paths(paths)
}

#[must_use]
pub fn collect_agent_rules_from_paths(paths: impl IntoIterator<Item = PathBuf>) -> String {
    let mut output = String::new();
    let mut seen = Vec::new();
    for path in paths {
        let key = path.to_string_lossy().into_owned();
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        let Some(content) = read_bounded_regular_utf8(&path, MAX_AGENT_RULE_FILE_BYTES) else {
            continue;
        };
        let section = format!("### {}\n\n{}\n\n", path.display(), content.trim());
        if output.len() + section.len() > MAX_AGENT_RULES_CHARS {
            let remaining = MAX_AGENT_RULES_CHARS.saturating_sub(output.len());
            push_str_byte_limit(&mut output, &section, remaining);
            break;
        }
        output.push_str(&section);
    }
    if output.trim().is_empty() {
        "(no AGENTS.md rules discovered)".to_owned()
    } else {
        output
    }
}
