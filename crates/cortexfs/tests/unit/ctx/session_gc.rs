use crate::{SessionIndexGuard, agent_session_select};
use cortexfs::update_session_index;
use std::thread;
use std::time::Duration;

#[test]
fn session_gc_storage_root_is_pinned_across_current_switch() {
    let root = clean_test_dir("session-gc-current-pin");
    let generations = root.join("generations");
    let relative = Path::new("home/1000/agent/executor/session");
    for generation in ["old", "new"] {
        assert!(fs::create_dir_all(generations.join(generation).join(relative)).is_ok());
    }
    assert!(
        std::os::unix::fs::symlink("generations/old", root.join("current")).is_ok()
    );
    let pinned = crate::gc_storage_session_root(&root.join("current"), relative);
    assert!(std::os::unix::fs::symlink("generations/new", root.join(".next")).is_ok());
    assert!(fs::rename(root.join(".next"), root.join("current")).is_ok());

    assert!(matches!(&pinned, Ok(Some(path)) if path == &generations.join("old").join(relative)));

    let pinned = pinned.ok().flatten();
    assert!(pinned.is_some());
    let archive = pinned
        .as_deref()
        .map(|path| gc_archive_agent_root(path, "executor", None));
    assert_eq!(
        archive,
        Some(Ok(generations
            .join("old/home/1000/archived_sessions/executor")))
    );
}

fn create_agent_session_gc_fixture(root: &Path) -> PathBuf {
    let home = ctx_home(root);
    assert!(home.is_ok(), "{home:?}");
    let session_root = home
        .unwrap_or_else(|_| root.join("home/0"))
        .join("agent")
        .join("executor")
        .join("session");
    for directory in ["index/by-cwd", "index/by-hash", "index/by-uuid"] {
        assert!(fs::create_dir_all(session_root.join(directory)).is_ok());
    }
    assert!(fs::write(session_root.join("index/current"), "current\n").is_ok());
    assert!(
        fs::write(
            session_root.join("index/list"),
            "current\ne2e-old\nsmoke-old\ne2e-keep\nmanual\ndefault\n",
        )
        .is_ok()
    );
    for session in [
        "default",
        "current",
        "e2e-old",
        "smoke-old",
        "e2e-keep",
        "manual",
    ] {
        assert!(fs::create_dir_all(session_root.join(session)).is_ok());
    }
    session_root
}

fn agent_session_gc_args(
    delete: bool,
    yes: bool,
    patterns: &[&str],
    keep: &[&str],
) -> AgentSessionGcArgs {
    AgentSessionGcArgs {
        name: "executor".to_owned(),
        dry_run: !yes,
        yes,
        delete,
        archive_dir: None,
        keep: keep.iter().map(|value| (*value).to_owned()).collect(),
        patterns: patterns
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
        older_than_days: None,
    }
}

/// Returns the default per-agent archive directory for a fixture session root.
fn fixture_archive_root(session_root: &Path) -> PathBuf {
    session_root
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .unwrap_or(session_root)
        .join("archived_sessions/executor")
}

#[test]
fn parses_agent_session_gc_delete_command() {
    let command = cmd!(
        "agent",
        "session",
        "gc",
        "executor",
        "--match",
        "e2e-*",
        "--keep",
        "keep-me",
        "--delete",
        "--yes",
        "--older-than-days",
        "7"
    );

    assert!(matches!(
        command,
        Ok(Command::Agent(AgentArgs::SessionGc(AgentSessionGcArgs {
            ref name,
            dry_run: false,
            yes: true,
            delete: true,
            archive_dir: None,
            ref keep,
            ref patterns,
            older_than_days: Some(7),
        }))) if name == "executor"
            && keep == &vec!["keep-me".to_owned()]
            && patterns == &vec!["e2e-*".to_owned()]
    ));
}

#[test]
/// Parses immediate session archive and its custom absolute root.
fn parses_agent_session_archive_command() {
    let command = cmd!(
        "agent",
        "session",
        "archive",
        "executor",
        "release-run",
        "--archive-dir",
        "/tmp/cortexfs-archive"
    );

    assert!(matches!(
        command,
        Ok(Command::Agent(AgentArgs::SessionArchive(AgentSessionArchiveArgs {
            ref name,
            ref session,
            archive_dir: Some(ref archive_dir),
        }))) if name == "executor"
            && session == "release-run"
            && archive_dir == &PathBuf::from("/tmp/cortexfs-archive")
    ));
}

#[test]
/// Rejects archive roots that are ambiguous or incompatible with delete mode.
fn rejects_invalid_session_archive_options() {
    assert!(matches!(
        cmd!("agent", "session", "archive", "executor", "run", "--archive-dir", "relative"),
        Err(ref error) if error.code == 2 && error.message.contains("absolute")
    ));
    assert!(matches!(
        cmd!("agent", "session", "gc", "executor", "--delete", "--archive-dir", "/tmp/archive"),
        Err(ref error) if error.code == 2 && error.message.contains("cannot be used")
    ));
}

#[test]
/// Resolves nested session help requests to implemented help topics.
fn parses_agent_session_archive_help_topics() {
    assert!(matches!(
        cmd!("agent", "session", "--help"),
        Ok(Command::HelpTopic(ref topic)) if topic == "agent session"
    ));
    assert!(matches!(
        cmd!("agent", "session", "archive", "--help"),
        Ok(Command::HelpTopic(ref topic)) if topic == "agent session archive"
    ));
    assert!(matches!(
        cmd!("agent", "session", "gc", "--help"),
        Ok(Command::HelpTopic(ref topic)) if topic == "agent session gc"
    ));
    assert!(matches!(
        cmd!("agent", "session", "select", "--help"),
        Ok(Command::HelpTopic(ref topic)) if topic == "agent session select"
    ));
    assert!(print_help_topic("agent session").is_ok());
    assert!(print_help_topic("agent session archive").is_ok());
    assert!(print_help_topic("agent session gc").is_ok());
    assert!(print_help_topic("agent session select").is_ok());
}

#[test]
fn parses_and_applies_agent_session_select_compare_and_swap() {
    let command = cmd!("agent", "session", "select", "executor", "default", "--from", "current");
    assert!(matches!(
        command,
        Ok(Command::Agent(AgentArgs::SessionSelect {
            ref name,
            ref target,
            ref from,
        })) if name == "executor" && target == "default" && from == "current"
    ));

    let root = clean_test_dir("ctx-agent-session-select");
    let session_root = create_agent_session_gc_fixture(&root);
    let list = fs::read_to_string(session_root.join("index/list")).unwrap_or_default();
    assert!(agent_session_select(&root, "executor", "default", "current").is_ok());
    assert_eq!(
        fs::read_to_string(session_root.join("index/current")).ok().as_deref(),
        Some("default\n")
    );
    assert_eq!(
        fs::read_to_string(session_root.join("index/list")).unwrap_or_default(),
        list
    );
    assert!(agent_session_select(&root, "executor", "current", "current").is_err());
    assert!(agent_session_select(&root, "executor", "missing", "default").is_err());
}

#[test]
fn agent_session_gc_and_select_share_index_guard() {
    let root = clean_test_dir("ctx-agent-session-index-guard");
    let session_root = create_agent_session_gc_fixture(&root);
    let guard = SessionIndexGuard::exclusive(&session_root);
    assert!(guard.is_ok());
    let select_root = root.to_path_buf();
    let (sent, received) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        let result = agent_session_select(&select_root, "executor", "default", "current");
        let _ignored = sent.send(result);
    });
    assert!(received.recv_timeout(Duration::from_millis(50)).is_err());
    drop(guard);
    assert_eq!(received.recv_timeout(Duration::from_secs(2)), Ok(Ok(())));
    assert!(worker.join().is_ok());

    let guard = SessionIndexGuard::exclusive(&session_root);
    assert!(guard.is_ok());
    let gc_root = root.to_path_buf();
    let (sent, received) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        let args = agent_session_gc_args(false, false, &["missing-*"], &[]);
        let result = agent_session_gc(&gc_root, &args);
        let _ignored = sent.send(result);
    });
    assert!(received.recv_timeout(Duration::from_millis(50)).is_err());
    drop(guard);
    assert_eq!(received.recv_timeout(Duration::from_secs(2)), Ok(Ok(())));
    assert!(worker.join().is_ok());
}

#[test]
fn benchmark_like_secondary_index_create_and_gc_repeats_twice() {
    let root = clean_test_dir("ctx-agent-session-secondary-gc-cycle");
    let session_root = create_agent_session_gc_fixture(&root);
    let secondary = session_root.join("index/by-cwd/cwd-cycle");
    for _cycle in 0..2 {
        assert!(fs::create_dir_all(session_root.join("e2e-cycle")).is_ok());
        assert_eq!(
            update_session_index(&session_root, "e2e-cycle", Some("cwd-cycle")),
            Ok(())
        );
        assert!(secondary.is_file());
        assert!(agent_session_select(&root, "executor", "default", "e2e-cycle").is_ok());
        let args = agent_session_gc_args(true, true, &["e2e-cycle"], &[]);
        assert!(agent_session_gc(&root, &args).is_ok());
        assert!(!session_root.join("e2e-cycle").exists());
        assert!(!secondary.exists());
    }
}

#[test]
fn agent_session_gc_dry_run_has_no_archive_side_effect() {
    let root = clean_test_dir("ctx-agent-session-gc-dry-run");
    let session_root = create_agent_session_gc_fixture(&root);
    let list = fs::read_to_string(session_root.join("index/list"));
    assert!(list.is_ok(), "{list:?}");
    let list = list.unwrap_or_default();

    let args = agent_session_gc_args(false, false, &["e2e-*", "*smoke*"], &[]);
    assert!(agent_session_gc(&root, &args).is_ok());

    assert!(!fixture_archive_root(&session_root).exists());
    assert!(!session_root.join(".archive").exists());
    assert!(session_root.join("e2e-old").is_dir());
    assert!(session_root.join("smoke-old").is_dir());
    assert!(
        fs::read_to_string(session_root.join("index/list")).is_ok_and(|content| content == list)
    );
}

#[test]
fn agent_session_gc_no_candidates_does_not_create_archive() {
    let root = clean_test_dir("ctx-agent-session-gc-empty");
    let session_root = create_agent_session_gc_fixture(&root);
    let args = agent_session_gc_args(false, true, &["missing-*"], &[]);

    assert!(agent_session_gc(&root, &args).is_ok());
    assert!(!fixture_archive_root(&session_root).exists());
}

#[test]
fn agent_session_gc_yes_archives_and_preserves_content() {
    let root = clean_test_dir("ctx-agent-session-gc-archive");
    let session_root = create_agent_session_gc_fixture(&root);
    assert!(fs::write(session_root.join("e2e-old/messages.jsonl"), "history\n").is_ok());
    let args = agent_session_gc_args(false, true, &["e2e-old"], &[]);

    assert!(agent_session_gc(&root, &args).is_ok());

    assert!(!session_root.join("e2e-old").exists());
    assert!(
        fs::read_to_string(fixture_archive_root(&session_root).join("e2e-old/messages.jsonl"))
            .is_ok_and(|content| content == "history\n")
    );
    assert!(session_root.join("smoke-old").is_dir());
}

#[test]
/// Archives one session immediately to the default external archive tree.
fn agent_session_archive_uses_default_destination() {
    let root = clean_test_dir("ctx-agent-session-archive-default");
    let session_root = create_agent_session_gc_fixture(&root);
    assert!(fs::write(session_root.join("manual/events.jsonl"), "raw-event\n").is_ok());
    let args = AgentSessionArchiveArgs {
        name: "executor".to_owned(),
        session: "manual".to_owned(),
        archive_dir: None,
    };

    assert!(agent_session_archive(&root, &args).is_ok());
    assert!(!session_root.join("manual").exists());
    assert_eq!(
        fs::read_to_string(fixture_archive_root(&session_root).join("manual/events.jsonl"))
            .ok()
            .as_deref(),
        Some("raw-event\n")
    );
    assert!(!session_root.join(".archive").exists());
}

#[test]
/// Archives one session under a custom absolute archive root.
fn agent_session_archive_uses_custom_destination() {
    let root = clean_test_dir("ctx-agent-session-archive-custom");
    let session_root = create_agent_session_gc_fixture(&root);
    let archive = root.join("external-archive");
    let args = AgentSessionArchiveArgs {
        name: "executor".to_owned(),
        session: "manual".to_owned(),
        archive_dir: Some(archive.clone()),
    };

    assert!(agent_session_archive(&root, &args).is_ok());
    assert!(archive.join("executor/manual").is_dir());
    assert!(!session_root.join("manual").exists());
}

#[test]
/// Applies a custom archive root to GC archive mode.
fn agent_session_gc_uses_custom_destination() {
    let root = clean_test_dir("ctx-agent-session-gc-custom");
    let session_root = create_agent_session_gc_fixture(&root);
    let archive = root.join("gc-archive");
    let mut args = agent_session_gc_args(false, true, &["e2e-old"], &[]);
    args.archive_dir = Some(archive.clone());

    assert!(agent_session_gc(&root, &args).is_ok());
    assert!(archive.join("executor/e2e-old").is_dir());
    assert!(!session_root.join("e2e-old").exists());
}

#[test]
/// Refuses protected and active sessions without creating an archive.
fn agent_session_archive_refuses_current_default_and_active() {
    let root = clean_test_dir("ctx-agent-session-archive-protected");
    let session_root = create_agent_session_gc_fixture(&root);
    assert!(fs::write(session_root.join("manual/state"), "active\n").is_ok());

    for session in ["default", "current", "manual"] {
        let args = AgentSessionArchiveArgs {
            name: "executor".to_owned(),
            session: session.to_owned(),
            archive_dir: None,
        };
        assert!(agent_session_archive(&root, &args).is_err());
        assert!(session_root.join(session).is_dir());
    }
    assert!(!fixture_archive_root(&session_root).exists());
}

/// Verifies an overlapping archive root fails without changing live state.
fn assert_archive_overlap_refused(
    root: &Path,
    session_root: &Path,
    archive_dir: PathBuf,
    unexpected: &Path,
) {
    assert!(fs::write(session_root.join("manual/source"), "live\n").is_ok());
    let list = fs::read_to_string(session_root.join("index/list")).unwrap_or_default();
    let args = AgentSessionArchiveArgs {
        name: "executor".to_owned(),
        session: "manual".to_owned(),
        archive_dir: Some(archive_dir),
    };

    let result = agent_session_archive(root, &args);
    assert!(matches!(result, Err(ref error) if error.message.contains("overlaps live session")));
    assert_eq!(
        fs::read_to_string(session_root.join("manual/source"))
            .ok()
            .as_deref(),
        Some("live\n")
    );
    assert_eq!(
        fs::read_to_string(session_root.join("index/list"))
            .ok()
            .as_deref(),
        Some(list.as_str())
    );
    assert!(!unexpected.exists());
}

#[test]
/// Rejects an archive root equal to the live session root.
fn agent_session_archive_rejects_equal_live_root() {
    let root = clean_test_dir("ctx-agent-session-archive-overlap-equal");
    let session_root = create_agent_session_gc_fixture(&root);
    assert_archive_overlap_refused(
        &root,
        &session_root,
        session_root.clone(),
        &session_root.join("executor"),
    );
}

#[test]
/// Rejects an archive root inside a live session directory.
fn agent_session_archive_rejects_inside_live_session() {
    let root = clean_test_dir("ctx-agent-session-archive-overlap-inside");
    let session_root = create_agent_session_gc_fixture(&root);
    assert_archive_overlap_refused(
        &root,
        &session_root,
        session_root.join("manual"),
        &session_root.join("manual/executor"),
    );
}

#[test]
/// Rejects an archive agent root that is an ancestor of live sessions.
fn agent_session_archive_rejects_live_ancestor() {
    let root = clean_test_dir("ctx-agent-session-archive-overlap-ancestor");
    let session_root = create_agent_session_gc_fixture(&root);
    let archive_root = session_root
        .parent()
        .and_then(Path::parent)
        .unwrap_or(&session_root)
        .to_path_buf();
    assert_archive_overlap_refused(
        &root,
        &session_root,
        archive_root,
        &session_root
            .parent()
            .unwrap_or(&session_root)
            .join("manual"),
    );
}

#[test]
/// Rejects parent traversal in a custom archive path before mutation.
fn agent_session_archive_rejects_parent_components() {
    let root = clean_test_dir("ctx-agent-session-archive-parent-component");
    let session_root = create_agent_session_gc_fixture(&root);
    let args = AgentSessionArchiveArgs {
        name: "executor".to_owned(),
        session: "manual".to_owned(),
        archive_dir: Some(root.join("archive/../elsewhere")),
    };

    assert!(matches!(
        agent_session_archive(&root, &args),
        Err(ref error) if error.message.contains("parent path components")
    ));
    assert!(session_root.join("manual").is_dir());
    assert!(!root.join("archive").exists());
}

#[test]
/// Applies the same live-tree overlap rejection to GC archive mode.
fn agent_session_gc_rejects_overlapping_archive_root() {
    let root = clean_test_dir("ctx-agent-session-gc-overlap");
    let session_root = create_agent_session_gc_fixture(&root);
    assert!(fs::write(session_root.join("e2e-old/source"), "live\n").is_ok());
    let list = fs::read_to_string(session_root.join("index/list")).unwrap_or_default();
    let mut args = agent_session_gc_args(false, true, &["e2e-old"], &[]);
    args.archive_dir = Some(session_root.clone());

    assert!(matches!(
        agent_session_gc(&root, &args),
        Err(ref error) if error.message.contains("overlaps live session")
    ));
    assert_eq!(
        fs::read_to_string(session_root.join("e2e-old/source"))
            .ok()
            .as_deref(),
        Some("live\n")
    );
    assert_eq!(
        fs::read_to_string(session_root.join("index/list"))
            .ok()
            .as_deref(),
        Some(list.as_str())
    );
    assert!(!session_root.join("executor").exists());
}

#[test]
fn agent_session_gc_archive_conflict_preserves_both_directories() {
    let root = clean_test_dir("ctx-agent-session-gc-archive-conflict");
    let session_root = create_agent_session_gc_fixture(&root);
    assert!(fs::write(session_root.join("e2e-old/source"), "live\n").is_ok());
    let archive = fixture_archive_root(&session_root);
    assert!(fs::create_dir_all(archive.join("e2e-old")).is_ok());
    assert!(fs::write(archive.join("e2e-old/sentinel"), "old\n").is_ok());
    let args = agent_session_gc_args(false, true, &["e2e-old"], &[]);

    assert!(agent_session_gc(&root, &args).is_err());

    assert!(
        fs::read_to_string(session_root.join("e2e-old/source"))
            .is_ok_and(|content| content == "live\n")
    );
    assert!(
        fs::read_to_string(archive.join("e2e-old/sentinel"))
            .is_ok_and(|content| content == "old\n")
    );
}

#[test]
fn agent_session_gc_delete_yes_removes_without_archive() {
    let root = clean_test_dir("ctx-agent-session-gc-delete");
    let session_root = create_agent_session_gc_fixture(&root);
    let args = agent_session_gc_args(true, true, &["e2e-old"], &[]);

    assert!(agent_session_gc(&root, &args).is_ok());

    assert!(!session_root.join("e2e-old").exists());
    assert!(!fixture_archive_root(&session_root).exists());
    assert!(session_root.join("smoke-old").is_dir());
}

#[test]
fn agent_session_gc_protects_default_current_and_keep() {
    let root = clean_test_dir("ctx-agent-session-gc-protected");
    let session_root = create_agent_session_gc_fixture(&root);
    let args = agent_session_gc_args(false, true, &["*"], &["e2e-keep"]);

    assert!(agent_session_gc(&root, &args).is_ok());

    for session in ["default", "current", "e2e-keep"] {
        assert!(session_root.join(session).is_dir(), "{session} should remain");
        assert!(
            !fixture_archive_root(&session_root).join(session).exists(),
            "{session} should not be archived"
        );
    }
}

#[test]
fn agent_session_gc_protects_active_and_unsafe_state_but_accepts_missing_state() {
    let root = clean_test_dir("ctx-agent-session-gc-active");
    let session_root = create_agent_session_gc_fixture(&root);
    for session in ["active-run", "unsafe-run", "legacy-run"] {
        assert!(fs::create_dir_all(session_root.join(session)).is_ok());
    }
    assert!(fs::write(session_root.join("active-run/state"), "active\n").is_ok());
    assert!(symlink("../active-run/state", session_root.join("unsafe-run/state")).is_ok());
    let args = agent_session_gc_args(false, true, &["*-run"], &[]);

    assert!(agent_session_gc(&root, &args).is_ok());

    assert!(session_root.join("active-run").is_dir());
    assert!(session_root.join("unsafe-run").is_dir());
    assert!(fixture_archive_root(&session_root).join("legacy-run").is_dir());
}

#[test]
fn agent_session_gc_cleans_only_matching_index_references() {
    let root = clean_test_dir("ctx-agent-session-gc-index");
    let session_root = create_agent_session_gc_fixture(&root);
    for directory in ["by-cwd", "by-hash", "by-uuid"] {
        assert!(
            fs::write(
                session_root.join("index").join(directory).join("target"),
                "e2e-old\n",
            )
            .is_ok()
        );
        assert!(
            fs::write(
                session_root.join("index").join(directory).join("other"),
                "manual\n",
            )
            .is_ok()
        );
    }
    let args = agent_session_gc_args(false, true, &["e2e-old"], &[]);

    assert!(agent_session_gc(&root, &args).is_ok());

    let list = fs::read_to_string(session_root.join("index/list"));
    assert!(list.is_ok(), "{list:?}");
    let list = list.unwrap_or_default();
    assert!(!list.lines().any(|session| session == "e2e-old"));
    assert!(list.lines().any(|session| session == "manual"));
    assert!(list.lines().any(|session| session == "current"));
    for directory in ["by-cwd", "by-hash", "by-uuid"] {
        assert!(!session_root
            .join("index")
            .join(directory)
            .join("target")
            .exists());
        assert!(
            fs::read_to_string(
                session_root
                    .join("index")
                    .join(directory)
                    .join("other")
            )
            .is_ok_and(|content| content == "manual\n")
        );
    }
}

#[test]
fn agent_session_gc_stage_list_replacement_preserves_foreign_list() {
    let root = clean_test_dir("ctx-agent-session-gc-list-replacement");
    let session_root = create_agent_session_gc_fixture(&root);
    let index = session_root.join("index");
    let list = index.join("list");
    let replacement = index.join("foreign-list");
    let foreign_content = "foreign\nmanual\ndefault\n";
    assert!(fs::write(&replacement, foreign_content).is_ok());
    let foreign_metadata = fs::symlink_metadata(&replacement);
    assert!(foreign_metadata.is_ok(), "{foreign_metadata:?}");
    let Ok(foreign_metadata) = foreign_metadata else {
        return;
    };
    let mapping = index.join("by-cwd/target");
    assert!(fs::write(&mapping, "e2e-old\n").is_ok());
    let args = agent_session_gc_args(false, true, &["e2e-old"], &[]);

    set_gc_list_publish_replacement_for_test(Some(replacement));
    let result = agent_session_gc(&root, &args);
    set_gc_list_publish_replacement_for_test(None);

    assert!(result.is_err(), "foreign index replacement must fail closed");
    let Err(error) = result else {
        return;
    };
    assert!(error.message.contains("rollback conflict"));
    assert!(session_root.join("e2e-old").is_dir());
    assert!(
        fs::read_to_string(&mapping).is_ok_and(|content| content == "e2e-old\n"),
        "secondary index claim must be restored"
    );
    assert!(!fs::read_dir(index.join("by-cwd")).is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".target.gc-")
        })
    }));
    assert!(fs::read_to_string(&list).is_ok_and(|content| content == foreign_content));
    let current_metadata = fs::symlink_metadata(&list);
    assert!(current_metadata.is_ok(), "{current_metadata:?}");
    let Ok(current_metadata) = current_metadata else {
        return;
    };
    assert_eq!(
        (current_metadata.dev(), current_metadata.ino()),
        (foreign_metadata.dev(), foreign_metadata.ino())
    );
}

#[test]
fn agent_session_gc_rollback_list_replacement_preserves_foreign_list() {
    let root = clean_test_dir("ctx-agent-session-gc-rollback-replacement");
    let session_root = create_agent_session_gc_fixture(&root);
    let index = session_root.join("index");
    let list = index.join("list");
    let replacement = index.join("rollback-foreign-list");
    let foreign_content = "foreign-rollback\nmanual\ndefault\n";
    assert!(fs::write(&replacement, foreign_content).is_ok());
    let foreign_metadata = fs::symlink_metadata(&replacement);
    assert!(foreign_metadata.is_ok(), "{foreign_metadata:?}");
    let Ok(foreign_metadata) = foreign_metadata else {
        return;
    };
    let mapping = index.join("by-cwd/target");
    assert!(fs::write(&mapping, "e2e-old\n").is_ok());
    let args = agent_session_gc_args(false, true, &["e2e-old"], &[]);

    set_gc_list_rollback_replacement_for_test(Some(replacement));
    set_gc_source_claim_fault_for_test(true);
    let result = agent_session_gc(&root, &args);
    set_gc_source_claim_fault_for_test(false);
    set_gc_list_rollback_replacement_for_test(None);

    assert!(result.is_err(), "rollback replacement must fail closed");
    let Err(error) = result else {
        return;
    };
    assert!(error.message.contains("rollback conflict"));
    assert!(session_root.join("e2e-old").is_dir());
    assert!(
        fs::read_to_string(&mapping).is_ok_and(|content| content == "e2e-old\n"),
        "secondary index claim must be restored"
    );
    assert!(!fs::read_dir(index.join("by-cwd")).is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".target.gc-")
        })
    }));
    assert!(fs::read_to_string(&list).is_ok_and(|content| content == foreign_content));
    let current_metadata = fs::symlink_metadata(&list);
    assert!(current_metadata.is_ok(), "{current_metadata:?}");
    let Ok(current_metadata) = current_metadata else {
        return;
    };
    assert_eq!(
        (current_metadata.dev(), current_metadata.ino()),
        (foreign_metadata.dev(), foreign_metadata.ino())
    );
}

#[test]
fn agent_session_gc_delete_failure_keeps_quarantine_and_commits_index_cleanup() {
    let root = clean_test_dir("ctx-agent-session-gc-delete-failure");
    let session_root = create_agent_session_gc_fixture(&root);
    let nested = session_root.join("e2e-old/nested");
    assert!(fs::create_dir_all(&nested).is_ok());
    assert!(fs::write(nested.join("payload"), "keep\n").is_ok());
    assert!(
        fs::set_permissions(&nested, fs::Permissions::from_mode(0o500)).is_ok()
    );
    let mapping = session_root.join("index/by-cwd/target");
    assert!(fs::write(&mapping, "e2e-old\n").is_ok());
    let args = agent_session_gc_args(true, true, &["e2e-old"], &[]);

    set_gc_delete_fault_for_test(true);
    let result = agent_session_gc(&root, &args);
    set_gc_delete_fault_for_test(false);

    assert!(
        result.is_err(),
        "injected recursive delete unexpectedly succeeded"
    );
    let Err(error) = result else {
        return;
    };
    let quarantine = fs::read_dir(&session_root)
        .ok()
        .and_then(|entries| {
            entries
                .filter_map(Result::ok)
                .find(|entry| entry.file_name().to_string_lossy().starts_with(".e2e-old.gc-"))
        })
        .map(|entry| entry.path());
    assert!(quarantine.is_some(), "delete residue should remain isolated");
    let quarantine = quarantine.unwrap_or_else(|| session_root.join("missing-quarantine"));
    assert!(!session_root.join("e2e-old").exists());
    assert!(quarantine.is_dir());
    assert!(!mapping.exists());
    let list = fs::read_to_string(session_root.join("index/list"));
    assert!(list.is_ok(), "{list:?}");
    assert!(!list
        .unwrap_or_default()
        .lines()
        .any(|session| session == "e2e-old"));
    assert!(error.message.contains("delete is irreversible"));
    assert!(error.message.contains("quarantine path"));
    assert!(error.message.contains(&quarantine.display().to_string()));

    assert!(
        fs::set_permissions(
            quarantine.join("nested"),
            fs::Permissions::from_mode(0o700),
        )
        .is_ok()
    );
}

#[test]
fn agent_session_gc_delete_parent_sync_failure_commits_index_cleanup() {
    let root = clean_test_dir("ctx-agent-session-gc-delete-sync-failure");
    let session_root = create_agent_session_gc_fixture(&root);
    let mapping = session_root.join("index/by-cwd/target");
    assert!(fs::write(&mapping, "e2e-old\n").is_ok());
    let args = agent_session_gc_args(true, true, &["e2e-old"], &[]);

    set_gc_delete_sync_fault_for_test(true);
    let result = agent_session_gc(&root, &args);
    set_gc_delete_sync_fault_for_test(false);

    assert!(
        result.is_err(),
        "injected delete parent sync failure unexpectedly succeeded"
    );
    let Err(error) = result else {
        return;
    };
    assert!(!session_root.join("e2e-old").exists());
    assert!(!fs::read_dir(&session_root).is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".e2e-old.gc-")
        })
    }));
    assert!(!mapping.exists());
    let list = fs::read_to_string(session_root.join("index/list"));
    assert!(list.is_ok(), "{list:?}");
    assert!(!list
        .unwrap_or_default()
        .lines()
        .any(|session| session == "e2e-old"));
    assert!(error.message.contains("quarantine already removed"));
    assert!(!error.message.contains("retained quarantine"));
}
