use crate::{SessionIndexGuard, agent_session_select};
use cortexfs::update_session_index;
use std::thread;
use std::time::Duration;

fn create_agent_session_gc_fixture(root: &Path) -> PathBuf {
    let home = ctx_home(root);
    assert!(home.is_ok(), "{home:?}");
    let session_root = home
        .unwrap_or_else(|_| root.join("home/0"))
        .join("agent")
        .join("coder")
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
        name: "coder".to_owned(),
        dry_run: !yes,
        yes,
        delete,
        keep: keep.iter().map(|value| (*value).to_owned()).collect(),
        patterns: patterns
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
        older_than_days: None,
    }
}

#[test]
fn parses_agent_session_gc_delete_command() {
    let command = cmd!(
        "agent",
        "session",
        "gc",
        "coder",
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
            ref keep,
            ref patterns,
            older_than_days: Some(7),
        }))) if name == "coder"
            && keep == &vec!["keep-me".to_owned()]
            && patterns == &vec!["e2e-*".to_owned()]
    ));
}

#[test]
fn parses_and_applies_agent_session_select_compare_and_swap() {
    let command = cmd!("agent", "session", "select", "coder", "default", "--from", "current");
    assert!(matches!(
        command,
        Ok(Command::Agent(AgentArgs::SessionSelect {
            ref name,
            ref target,
            ref from,
        })) if name == "coder" && target == "default" && from == "current"
    ));

    let root = clean_test_dir("ctx-agent-session-select");
    let session_root = create_agent_session_gc_fixture(&root);
    let list = fs::read_to_string(session_root.join("index/list")).unwrap_or_default();
    assert!(agent_session_select(&root, "coder", "default", "current").is_ok());
    assert_eq!(
        fs::read_to_string(session_root.join("index/current")).ok().as_deref(),
        Some("default\n")
    );
    assert_eq!(
        fs::read_to_string(session_root.join("index/list")).unwrap_or_default(),
        list
    );
    assert!(agent_session_select(&root, "coder", "current", "current").is_err());
    assert!(agent_session_select(&root, "coder", "missing", "default").is_err());
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
        let result = agent_session_select(&select_root, "coder", "default", "current");
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
        assert!(agent_session_select(&root, "coder", "default", "e2e-cycle").is_ok());
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
    assert!(!session_root.join(".archive").exists());
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
        fs::read_to_string(session_root.join(".archive/e2e-old/messages.jsonl"))
            .is_ok_and(|content| content == "history\n")
    );
    assert!(session_root.join("smoke-old").is_dir());
}

#[test]
fn agent_session_gc_archive_conflict_preserves_both_directories() {
    let root = clean_test_dir("ctx-agent-session-gc-archive-conflict");
    let session_root = create_agent_session_gc_fixture(&root);
    assert!(fs::write(session_root.join("e2e-old/source"), "live\n").is_ok());
    assert!(fs::create_dir_all(session_root.join(".archive/e2e-old")).is_ok());
    assert!(fs::write(session_root.join(".archive/e2e-old/sentinel"), "old\n").is_ok());
    let args = agent_session_gc_args(false, true, &["e2e-old"], &[]);

    assert!(agent_session_gc(&root, &args).is_err());

    assert!(
        fs::read_to_string(session_root.join("e2e-old/source"))
            .is_ok_and(|content| content == "live\n")
    );
    assert!(
        fs::read_to_string(session_root.join(".archive/e2e-old/sentinel"))
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
    assert!(!session_root.join(".archive").exists());
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
            !session_root.join(".archive").join(session).exists(),
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
    assert!(session_root.join(".archive/legacy-run").is_dir());
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
