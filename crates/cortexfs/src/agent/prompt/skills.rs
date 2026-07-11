use super::read::{push_str_byte_limit, read_bounded_regular_utf8};
use super::*;
use crate::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
}

#[must_use]
pub fn collect_skill_metadata(max_chars: usize) -> String {
    let skills = discover_skill_metadata();
    format_skill_metadata_with_budget(&skills, max_chars)
}

#[must_use]
pub fn skill_metadata_budget_from_env() -> usize {
    env::var("CTX_CONTEXT_WINDOW_CHARS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .map_or(MAX_SKILL_METADATA_CHARS, |window| {
            window.saturating_mul(2).saturating_div(100)
        })
}

#[must_use]
pub fn format_skill_metadata_with_budget(skills: &[SkillMetadata], max_chars: usize) -> String {
    let full = format_skill_metadata(skills, false);
    if full.len() <= max_chars {
        return full;
    }
    let shortened = format_skill_metadata(skills, true);
    if shortened.len() <= max_chars {
        return format!(
            "WARNING: skill descriptions were shortened to fit the {max_chars} character budget.\n\n{shortened}"
        );
    }

    let warning = format!(
        "WARNING: skill metadata exceeded the {max_chars} character budget; some skills were omitted.\n\n"
    );
    let mut output = String::new();
    push_str_byte_limit(&mut output, &warning, max_chars);
    for skill in skills {
        let line = format_skill_metadata_item(skill, true);
        if output.len() + line.len() > max_chars {
            break;
        }
        output.push_str(&line);
    }
    if max_chars > 0 && output.trim().is_empty() {
        "(no skills discovered)".to_owned()
    } else {
        output
    }
}

pub(crate) fn discover_skill_metadata() -> Vec<SkillMetadata> {
    discover_skill_metadata_from_roots(default_skill_roots())
}

pub(crate) fn default_skill_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        push_project_skill_roots(&mut roots, &cwd);
    }
    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        roots.push(home.join(".agents").join("skills"));
        roots.push(home.join(".codex").join("skills"));
        roots.push(home.join(".codex").join("plugins").join("cache"));
    }
    roots
}

pub(crate) fn push_project_skill_roots(roots: &mut Vec<PathBuf>, cwd: &Path) {
    for ancestor in cwd.ancestors() {
        roots.push(ancestor.join(".agents").join("skills"));
        roots.push(ancestor.join(".codex").join("skills"));
    }
}

pub(crate) fn discover_skill_metadata_from_roots(
    roots: impl IntoIterator<Item = PathBuf>,
) -> Vec<SkillMetadata> {
    let mut paths = Vec::new();
    for root in roots {
        let mut root_paths = Vec::new();
        collect_skill_files(&root, &mut root_paths, 0);
        root_paths.sort();
        for path in root_paths {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    paths
        .into_iter()
        .filter_map(|path| read_skill_metadata(&path))
        .collect()
}

pub(crate) fn collect_skill_files(root: &Path, paths: &mut Vec<PathBuf>, depth: usize) {
    if depth > 8 || paths.len() >= MAX_SKILL_FILES {
        return;
    }
    let Ok(root_dir) = open_plain_directory(root) else {
        return;
    };
    let Ok(entries) = fs::read_dir(support::plain::proc_fd_path(&root_dir)) else {
        return;
    };
    for entry in entries.flatten() {
        if paths.len() >= MAX_SKILL_FILES {
            break;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let path = root.join(name.as_ref());
        if name.as_ref() == "SKILL.md" {
            if fd_entry_is_regular_file(&root_dir, &name) {
                paths.push(path);
            }
        } else if fd_entry_is_directory(&root_dir, &name) {
            collect_skill_files(&path, paths, depth + 1);
        }
    }
}

pub(crate) fn read_skill_metadata(path: &Path) -> Option<SkillMetadata> {
    let content = read_bounded_regular_utf8(path, MAX_SKILL_FILE_BYTES)?;
    let (name, description) = parse_skill_frontmatter(&content);
    let name = name.unwrap_or_else(|| {
        path.parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or("skill")
            .to_owned()
    });
    Some(SkillMetadata {
        name,
        description: description.unwrap_or_default(),
        path: path.to_path_buf(),
    })
}

pub(crate) fn parse_skill_frontmatter(content: &str) -> (Option<String>, Option<String>) {
    let mut lines = content.lines();
    if lines.next() != Some("---") {
        return (None, None);
    }
    let mut name = None;
    let mut description = None;
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix("name:") {
            name = Some(value.trim().trim_matches('"').to_owned());
        } else if let Some(value) = line.strip_prefix("description:") {
            description = Some(value.trim().trim_matches('"').to_owned());
        }
    }
    (name, description)
}

pub(crate) fn format_skill_metadata(skills: &[SkillMetadata], shorten: bool) -> String {
    if skills.is_empty() {
        return "(no skills discovered)".to_owned();
    }
    let mut output = String::new();
    for skill in skills {
        output.push_str(&format_skill_metadata_item(skill, shorten));
    }
    output
}

pub(crate) fn format_skill_metadata_item(skill: &SkillMetadata, shorten: bool) -> String {
    let description = if shorten {
        shorten_description(&skill.description, 160)
    } else {
        skill.description.clone()
    };
    format!(
        "- name: {}\n  description: {}\n  path: {}\n",
        skill.name,
        description,
        skill.path.display()
    )
}

pub(crate) fn shorten_description(description: &str, max_chars: usize) -> String {
    let normalized = description.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    normalized.chars().take(max_chars).collect::<String>()
}
