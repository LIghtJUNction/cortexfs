use crate::*;

use std::env;
use std::path::{Path, PathBuf};

/// Write session load snapshots for one run.
///
/// `AGENTS.md` is the effective merged rules body. `SKILLS.md` is skill
/// metadata only (`name` / `description` / `path`). Ordinary session files,
/// not authority.
pub fn write_snapshot(session: &Path, rules: &str, skills: &str) -> std::io::Result<()> {
    atomic_replace_text(&session.join("AGENTS.md"), &with_nl(rules))?;
    atomic_replace_text(&session.join("SKILLS.md"), &with_nl(skills))?;
    Ok(())
}

/// Best-effort snapshot for the active agent run. Never fails the caller.
pub fn write_run_snapshot(ctx_root: &Path, agent: &str, rules: &str, skills: &str) {
    for dir in snapshot_dirs(ctx_root, agent) {
        if plain_dir(&dir) && write_snapshot(&dir, rules, skills).is_ok() {
            return;
        }
    }
}

/// Candidate private session dirs for snapshots.
///
/// Prefer sandbox agent home (`HOME`) over read-only `/ctx` views.
#[must_use]
pub fn snapshot_dirs(ctx_root: &Path, agent: &str) -> Vec<PathBuf> {
    if !is_object_name(agent) {
        return Vec::new();
    }
    let session = env::var("CTX_SESSION")
        .ok()
        .filter(|s| is_object_name(s))
        .unwrap_or_else(|| "default".to_owned());

    let mut dirs = Vec::new();
    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        push_unique(&mut dirs, home.join("session").join(&session));
    }
    if let Some(ctx_home) = env::var_os("CTX_HOME").map(PathBuf::from) {
        push_unique(
            &mut dirs,
            cortexfs_paths::agent_sessions_from_home_path(&ctx_home, agent).join(&session),
        );
    }
    if let Ok(view) = derive_agent_runtime_view(ctx_root, agent) {
        push_unique(&mut dirs, view.home().join("session").join(&session));
    }
    let uid = nix::unistd::Uid::effective().as_raw().to_string();
    push_unique(
        &mut dirs,
        cortexfs_paths::agent_sessions_path(ctx_root, &uid, agent).join(session),
    );
    dirs
}

fn push_unique(dirs: &mut Vec<PathBuf>, path: PathBuf) {
    if !dirs.iter().any(|p| p == &path) {
        dirs.push(path);
    }
}

fn plain_dir(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
}

fn with_nl(s: &str) -> String {
    if s.is_empty() || s.ends_with('\n') {
        s.to_owned()
    } else {
        format!("{s}\n")
    }
}
