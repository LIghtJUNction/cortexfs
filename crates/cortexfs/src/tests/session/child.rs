#[test]
fn exclusive_child_handoff_publishes_complete_channel_and_rolls_back() {
    let session = clean_test_dir("child-handoff-exclusive");
    create_complete_session_layout(&session);

    let receipt = ok!(publish_child_handoff(
        &session, "worker-1", "worker", "default", "task"
    ));

    assert_file_text(&receipt.path().join("status"), "pending\n");
    assert!(receipt.path().join("artifact").is_dir());
    assert!(rollback_child_handoff(&receipt).is_ok());
    assert!(!receipt.path().exists());
}

#[test]
fn child_handoff_claim_is_receipt_and_identity_bound() {
    let session = clean_test_dir("child-handoff-claim");
    create_complete_session_layout(&session);
    let receipt = ok!(publish_child_handoff(
        &session,
        "worker-1",
        "worker",
        "dedicated",
        "task"
    ));
    let before = ok!(fs::metadata(receipt.path().join("status")));

    assert_eq!(
        claim_child_handoff_active(&receipt, "worker", "dedicated"),
        Ok(())
    );
    assert_file_text(&receipt.path().join("status"), "active\n");
    let after = ok!(fs::metadata(receipt.path().join("status")));
    assert_eq!(
        (after.uid(), after.gid(), after.mode() & 0o7777),
        (before.uid(), before.gid(), before.mode() & 0o7777)
    );
    assert_eq!(
        claim_child_handoff_active(&receipt, "worker", "dedicated"),
        Err(ChildContextRecordError::InvalidStatus)
    );
}

#[test]
fn stop_lease_linearizes_cancel_before_completion() {
    let session = clean_test_dir("child-stop-lease-cancel");
    create_complete_session_layout(&session);
    let receipt = ok!(publish_child_handoff(
        &session,
        "worker-1",
        "worker",
        "dedicated",
        "task"
    ));
    assert!(claim_child_handoff_active(&receipt, "worker", "dedicated").is_ok());
    let lease = ok!(crate::runtime::record::child::acquire_child_finish_lease(
        &receipt
    ));
    let (sent, received) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let result = crate::finish_child_result(
                &receipt,
                "worker",
                "dedicated",
                ChildContextStatus::Done,
                "done",
                "",
            );
            assert!(sent.send(result).is_ok());
        });
        assert!(
            received
                .recv_timeout(std::time::Duration::from_millis(50))
                .is_err()
        );
        assert_eq!(
            crate::runtime::record::child::finish_child_result_with_lease(
                lease,
                &receipt,
                "worker",
                "dedicated",
                ChildContextStatus::Cancelled,
                "cancelled",
                ""
            ),
            Ok(())
        );
    });
    assert_eq!(
        received.recv().ok(),
        Some(Err(ChildContextRecordError::InvalidStatus))
    );
    assert_file_text(&receipt.path().join("status"), "cancelled\n");
}

#[test]
fn stop_lease_preserves_completion_that_committed_first() {
    let session = clean_test_dir("child-stop-lease-done");
    create_complete_session_layout(&session);
    let receipt = ok!(publish_child_handoff(
        &session,
        "worker-1",
        "worker",
        "dedicated",
        "task"
    ));
    assert!(claim_child_handoff_active(&receipt, "worker", "dedicated").is_ok());
    assert!(
        crate::finish_child_result(
            &receipt,
            "worker",
            "dedicated",
            ChildContextStatus::Done,
            "done",
            ""
        )
        .is_ok()
    );
    let lease = ok!(crate::runtime::record::child::acquire_child_finish_lease(
        &receipt
    ));
    assert_eq!(
        crate::runtime::record::child::child_finish_lease_status(&lease),
        Ok(ChildContextStatus::Done)
    );
    drop(lease);
    assert_file_text(&receipt.path().join("status"), "done\n");
}

#[test]
fn child_handoff_claim_rejects_wrong_identity_and_replacement() {
    for (agent, session_name) in [("other", "dedicated"), ("worker", "other")] {
        let session = clean_test_dir(&format!("child-handoff-claim-{agent}-{session_name}"));
        create_complete_session_layout(&session);
        let receipt = ok!(publish_child_handoff(
            &session,
            "worker-1",
            "worker",
            "dedicated",
            "task"
        ));
        assert!(claim_child_handoff_active(&receipt, agent, session_name).is_err());
        assert_file_text(&receipt.path().join("status"), "pending\n");
    }

    let session = clean_test_dir("child-handoff-claim-replacement");
    create_complete_session_layout(&session);
    let receipt = ok!(publish_child_handoff(
        &session,
        "worker-1",
        "worker",
        "dedicated",
        "task"
    ));
    assert!(fs::remove_dir_all(receipt.path()).is_ok());
    assert!(fs::create_dir_all(receipt.path()).is_ok());
    write_text_file(&receipt.path().join("status"), "pending\n");
    write_text_file(&receipt.path().join("third-party"), "keep\n");
    assert!(claim_child_handoff_active(&receipt, "worker", "dedicated").is_err());
    assert_file_text(&receipt.path().join("third-party"), "keep\n");
}

#[test]
fn child_handoff_claim_faults_leave_pending_status() {
    for stage in [ChildClaimStage::Staging, ChildClaimStage::Publish] {
        let session = clean_test_dir(&format!("child-handoff-claim-fault-{stage:?}"));
        create_complete_session_layout(&session);
        let receipt = ok!(publish_child_handoff(
            &session,
            "worker-1",
            "worker",
            "dedicated",
            "task"
        ));
        let result =
            claim_child_handoff_active_with_hook(&receipt, "worker", "dedicated", |current| {
                if current == stage {
                    Err(ChildContextRecordError::CannotRecord)
                } else {
                    Ok(())
                }
            });
        assert!(result.is_err());
        assert_file_text(&receipt.path().join("status"), "pending\n");
    }
}

#[test]
fn exclusive_child_handoff_refuses_symlink_destination() {
    let session = clean_test_dir("child-handoff-symlink");
    let outside = clean_test_dir("child-handoff-symlink-outside");
    create_complete_session_layout(&session);
    let child = session.join("context/child/worker-1");
    assert!(std::os::unix::fs::symlink(&outside, &child).is_ok());

    assert!(publish_child_handoff(&session, "worker-1", "worker", "default", "task").is_err());
    assert!(
        child
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
    );
}

#[test]
fn concurrent_child_handoff_has_one_publisher() {
    let session = clean_test_dir("child-handoff-concurrent");
    create_complete_session_layout(&session);
    let left = session.to_path_buf();
    let right = session.to_path_buf();
    let first = std::thread::spawn(move || {
        publish_child_handoff(&left, "worker-1", "worker", "default", "left")
    });
    let second = std::thread::spawn(move || {
        publish_child_handoff(&right, "worker-1", "worker", "default", "right")
    });

    let successes = usize::from(first.join().is_ok_and(|result| result.is_ok()))
        + usize::from(second.join().is_ok_and(|result| result.is_ok()));
    assert_eq!(successes, 1);
}

#[test]
fn child_handoff_receipt_does_not_remove_replacement() {
    let session = clean_test_dir("child-handoff-replacement");
    create_complete_session_layout(&session);
    let receipt = ok!(publish_child_handoff(
        &session, "worker-1", "worker", "default", "task"
    ));
    assert!(fs::remove_dir_all(receipt.path()).is_ok());
    assert!(fs::create_dir_all(receipt.path()).is_ok());
    write_text_file(&receipt.path().join("third-party"), "keep\n");

    assert!(rollback_child_handoff(&receipt).is_err());
    assert_file_text(&receipt.path().join("third-party"), "keep\n");
}

#[test]
fn child_handoff_fault_matrix_leaves_no_partial_channel_or_staging() {
    for stage in [
        ChildHandoffStage::Staging,
        ChildHandoffStage::Artifact,
        ChildHandoffStage::Agent,
        ChildHandoffStage::Session,
        ChildHandoffStage::Status,
        ChildHandoffStage::Handoff,
        ChildHandoffStage::Result,
        ChildHandoffStage::Refs,
        ChildHandoffStage::Publish,
    ] {
        let session = clean_test_dir(&format!("child-handoff-fault-{stage:?}"));
        create_complete_session_layout(&session);
        let result = publish_child_handoff_with_hook(
            &session,
            "worker-1",
            "worker",
            "default",
            "task",
            |current| {
                if current == stage {
                    Err(ChildContextRecordError::CannotRecord)
                } else {
                    Ok(())
                }
            },
        );

        assert_eq!(result, Err(ChildContextRecordError::CannotRecord));
        let child_parent = session.join("context/child");
        assert!(!child_parent.join("worker-1").exists());
        let entries = ok!(fs::read_dir(child_parent));
        assert!(
            entries
                .filter_map(Result::ok)
                .all(|entry| { !entry.file_name().to_string_lossy().contains(".stage-") })
        );
    }
}

#[test]
fn child_handoff_publish_failure_compensates_agent_receipt_without_orphan() {
    let root = clean_test_dir("child-handoff-agent-compensation");
    let parent_session = root.join("home/1000/agent/parent/session/default");
    create_complete_session_layout(&parent_session);
    let receipt = ok!(create_agent_files(
        &root,
        "1000",
        "worker-1",
        "#!/bin/sh\nexec /bin/false \"$@\"\n",
        &[],
    ));

    let publish = publish_child_handoff_with_hook(
        &parent_session,
        "worker-1",
        "worker-1",
        "default",
        "task",
        |stage| {
            if stage == ChildHandoffStage::Publish {
                Err(ChildContextRecordError::CannotRecord)
            } else {
                Ok(())
            }
        },
    );
    assert!(publish.is_err());
    assert!(rollback_agent_files(receipt).is_ok());

    assert!(!root.join("agent/worker-1").exists());
    assert!(!root.join("agent/worker-1.d").exists());
    assert!(!root.join("agent/worker-1.sock").exists());
    assert!(!root.join("home/1000/agent/worker-1").exists());
    assert!(!parent_session.join("context/child/worker-1").exists());
}

#[test]
fn agent_create_fault_matrix_leaves_no_objects_or_private_home() {
    for stage in [
        AgentCreateStage::Control,
        AgentCreateStage::Controls,
        AgentCreateStage::Wrapper,
        AgentCreateStage::Executable,
        AgentCreateStage::Socket,
        AgentCreateStage::Home,
        AgentCreateStage::HomeBound,
        AgentCreateStage::Skeleton,
        AgentCreateStage::SessionBound,
    ] {
        let root = clean_test_dir(&format!("agent-create-fault-{stage:?}"));
        let result = create_agent_files_with_hook(
            &root,
            "1000",
            "worker-1",
            "#!/bin/sh\nexec /bin/false \"$@\"\n",
            &[],
            |current| {
                if current == stage {
                    Err(AgentCreateError::CannotCreate)
                } else {
                    Ok(())
                }
            },
        );

        assert!(result.is_err());
        assert!(!root.join("agent/worker-1").exists());
        assert!(!root.join("agent/worker-1.d").exists());
        assert!(!root.join("agent/worker-1.sock").exists());
        assert!(!root.join("home/1000/agent/worker-1").exists());
    }
}

#[test]
fn agent_create_home_skeleton_is_child_owned() {
    let root = clean_test_dir("agent-create-home-owner");
    let uid = nix::unistd::geteuid().as_raw();
    let gid = nix::unistd::getegid().as_raw();
    let uid_text = uid.to_string();
    let gid_text = gid.to_string();
    let result = create_agent_files(
        &root,
        &uid_text,
        "worker-1",
        "#!/bin/sh\nexit 0\n",
        &[("gid", &gid_text)],
    );
    assert!(result.is_ok());
    let home = root.join(format!("home/{uid}/agent/worker-1"));
    for relative in [
        "",
        "root",
        "data",
        "cache",
        "log",
        "session",
        "session/index",
        "session/index/by-cwd",
        "session/index/by-hash",
        "session/index/by-uuid",
    ] {
        let metadata = fs::symlink_metadata(home.join(relative));
        assert!(
            matches!(metadata, Ok(ref metadata) if metadata.uid() == uid && metadata.gid() == gid)
        );
    }
}

#[test]
fn agent_create_rollback_surfaces_nonempty_owned_directory() {
    let root = clean_test_dir("agent-create-rollback-nonempty");
    let result = create_agent_files_with_hook(
        &root,
        "1000",
        "worker-1",
        "#!/bin/sh\nexit 0\n",
        &[],
        |stage| {
            if stage == AgentCreateStage::SessionBound {
                write_text_file(
                    &root.join("home/1000/agent/worker-1/cache/injected"),
                    "keep\n",
                );
                Err(AgentCreateError::CannotCreate)
            } else {
                Ok(())
            }
        },
    );
    assert!(matches!(result, Err(AgentCreateError::RollbackConflict(_))));
    let Err(AgentCreateError::RollbackConflict(conflict)) = result else {
        return;
    };
    assert_eq!(conflict.stage, "rmdir");
    let Some(quarantine) = conflict.quarantine else {
        return;
    };
    assert_file_text(&quarantine.join("injected"), "keep\n");
}

#[test]
fn agent_create_control_injection_after_receipts_is_preserved_as_conflict() {
    let root = clean_test_dir("agent-create-control-injection");
    let result = create_agent_files_with_hook(
        &root,
        "1000",
        "worker-1",
        "#!/bin/sh\nexit 0\n",
        &[],
        |stage| {
            if stage == AgentCreateStage::Executable {
                write_text_file(&root.join("agent/worker-1.d/injected"), "keep\n");
                Err(AgentCreateError::CannotCreate)
            } else {
                Ok(())
            }
        },
    );
    assert!(matches!(result, Err(AgentCreateError::RollbackConflict(_))));
    let Err(AgentCreateError::RollbackConflict(conflict)) = result else {
        return;
    };
    assert_eq!(conflict.stage, "rmdir");
    let Some(quarantine) = conflict.quarantine else {
        return;
    };
    assert_file_text(&quarantine.join("injected"), "keep\n");
}

#[test]
fn agent_create_control_enumeration_rejects_excessive_depth() {
    let root = clean_test_dir("agent-create-control-depth");
    let result = create_agent_files_with_hook(
        &root,
        "1000",
        "worker-1",
        "#!/bin/sh\nexit 0\n",
        &[],
        |stage| {
            if stage == AgentCreateStage::Wrapper {
                fs::create_dir_all(root.join("agent/worker-1.d/a/b/c/d/e/f/g/h/i/j"))
                    .map_err(|_error| AgentCreateError::CannotCreate)?;
            }
            Ok(())
        },
    );
    assert!(result.is_err());
}

#[test]
fn agent_home_replacement_is_not_written_or_removed() {
    for stage in [AgentCreateStage::HomeBound, AgentCreateStage::SessionBound] {
        let root = clean_test_dir(&format!("agent-home-replacement-{stage:?}"));
        let result = create_agent_files_with_hook(
            &root,
            "1000",
            "worker-1",
            "#!/bin/sh\nexec /bin/false \"$@\"\n",
            &[],
            |current| {
                if current != stage {
                    return Ok(());
                }
                let path = if stage == AgentCreateStage::HomeBound {
                    root.join("home/1000/agent/worker-1")
                } else {
                    root.join("home/1000/agent/worker-1/session")
                };
                fs::remove_dir(&path).map_err(|_error| AgentCreateError::CannotCreate)?;
                fs::create_dir_all(&path).map_err(|_error| AgentCreateError::CannotCreate)?;
                write_text_file(&path.join("third-party"), "keep\n");
                Ok(())
            },
        );

        assert!(matches!(result, Err(AgentCreateError::RollbackConflict(_))));
        let marker = if stage == AgentCreateStage::HomeBound {
            root.join("home/1000/agent/worker-1/third-party")
        } else {
            root.join("home/1000/agent/worker-1/session/third-party")
        };
        assert_file_text(&marker, "keep\n");
    }
}

#[test]
fn child_stage_replacement_is_not_published_or_removed() {
    let session = clean_test_dir("child-stage-replacement");
    create_complete_session_layout(&session);
    let child_parent = session.join("context/child");
    let result = publish_child_handoff_with_hook(
        &session,
        "worker-1",
        "worker",
        "default",
        "task",
        |stage| {
            if stage == ChildHandoffStage::Artifact {
                let entry = fs::read_dir(&child_parent)
                    .map_err(|_error| ChildContextRecordError::CannotRecord)?
                    .filter_map(Result::ok)
                    .find(|entry| entry.file_name().to_string_lossy().contains(".stage-"));
                let Some(entry) = entry else {
                    return Err(ChildContextRecordError::CannotRecord);
                };
                fs::remove_dir(entry.path())
                    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
                fs::create_dir_all(entry.path())
                    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
                write_text_file(&entry.path().join("third-party"), "keep\n");
            }
            Ok(())
        },
    );

    assert!(result.is_err());
    assert!(!child_parent.join("worker-1").exists());
    let replacement = ok!(fs::read_dir(child_parent))
        .filter_map(Result::ok)
        .find(|entry| entry.file_name().to_string_lossy().contains(".stage-"));
    assert!(replacement.is_some());
    let Some(replacement) = replacement else {
        return;
    };
    assert_file_text(&replacement.path().join("third-party"), "keep\n");
}

#[test]
fn agent_create_executable_publish_does_not_replace_existing_file() {
    let root = clean_test_dir("agent-create-executable-conflict");
    assert!(fs::create_dir_all(root.join("agent")).is_ok());
    write_text_file(&root.join("agent/worker-1"), "third-party\n");
    let before = ok!(fs::symlink_metadata(root.join("agent/worker-1")));

    let result = create_agent_files(
        &root,
        "1000",
        "worker-1",
        "#!/bin/sh\nexec /bin/false \"$@\"\n",
        &[],
    );

    assert!(result.is_err());
    assert_file_text(&root.join("agent/worker-1"), "third-party\n");
    let after = ok!(fs::symlink_metadata(root.join("agent/worker-1")));
    assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
}

#[test]
fn agent_rollback_conflict_preserves_replacement_after_quarantine() {
    let root = clean_test_dir("agent-rollback-quarantine-conflict");
    let receipt = ok!(create_agent_files(
        &root,
        "1000",
        "worker-1",
        "#!/bin/sh\nexec /bin/false \"$@\"\n",
        &[],
    ));
    let mut injected = false;
    let mut replacement_identity = None;
    let mut replacement_path = None;
    let result = rollback_agent_files_with_hook(receipt, |stage, path| {
        if stage == AgentRollbackStage::Quarantined && path.ends_with("worker-1") && !injected {
            injected = true;
            assert!(fs::create_dir_all(path).is_ok());
            write_text_file(&path.join("third-party"), "keep\n");
            let metadata = ok!(fs::symlink_metadata(path));
            replacement_identity = Some((metadata.dev(), metadata.ino()));
            replacement_path = Some(path.to_path_buf());
        }
    });

    assert!(matches!(result, Err(AgentRollbackError::Conflict(_))));
    let Err(AgentRollbackError::Conflict(conflict)) = result else {
        return;
    };
    assert_eq!(Some(&conflict.original), replacement_path.as_ref());
    assert_eq!(conflict.stage, "original-recreated");
    assert!(conflict.quarantine.is_some());
    assert!(replacement_path.is_some());
    let Some(replacement) = replacement_path else {
        return;
    };
    assert_file_text(&replacement.join("third-party"), "keep\n");
    let metadata = ok!(fs::symlink_metadata(&replacement));
    assert_eq!(replacement_identity, Some((metadata.dev(), metadata.ino())));
    assert!(replacement.parent().is_some());
    let Some(parent) = replacement.parent() else {
        return;
    };
    let quarantines = ok!(fs::read_dir(parent))
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".ctx-rollback-")
        })
        .collect::<Vec<_>>();
    assert_eq!(quarantines.len(), 1);
    assert!(!root.join("agent/worker-1").exists());
    assert!(!root.join("agent/worker-1.d").exists());
    assert!(!root.join("agent/worker-1.sock").exists());
}

#[test]
fn child_context_recorder_rejects_bad_names_status_and_refs() {
    let root = clean_test_dir("child-context-record-bad");
    let session = root.join("default");

    create_complete_session_layout(&session);

    assert_eq!(
        record_child_handoff_to_parent_context(
            &session,
            "bad/child",
            "reviewer",
            "default",
            "Task: no\n",
        ),
        Err(ChildContextRecordError::InvalidChildName)
    );
    assert_eq!(
        record_child_handoff_to_parent_context(
            &session,
            "rev-2",
            "reviewer",
            "default",
            "Task: no\n",
        ),
        Ok(())
    );
    assert_eq!(
        record_child_result_to_parent_context(
            &session,
            "rev-2",
            ChildContextStatus::Pending,
            "not terminal",
            "",
        ),
        Err(ChildContextRecordError::InvalidStatus)
    );
    assert_eq!(
        record_child_result_to_parent_context(
            &session,
            "rev-2",
            ChildContextStatus::Done,
            "done",
            "{\"path\":\"../secret\"}\n",
        ),
        Err(ChildContextRecordError::InvalidRefs)
    );
    assert_eq!(ChildContextRecordError::InvalidRefs.errno(), "EINVAL");
}

#[test]
fn session_layout_inspector_accepts_transparent_context_tree() {
    let root = clean_test_dir("session-layout-ok");
    create_complete_session_layout(&root);

    let report = inspect_session_layout(&root);
    assert!(report.is_ok());
    assert!(report.issues().is_empty());
}

#[test]
fn session_layout_inspector_reports_missing_and_wrong_types() {
    let root = clean_test_dir("session-layout-bad");
    let context = root.join("context");
    let child = context.join("child").join("rev-1");
    assert!(fs::create_dir_all(root.join("messages.jsonl")).is_ok());
    assert!(fs::create_dir_all(&child).is_ok());
    assert!(fs::write(child.join("agent"), "reviewer\n").is_ok());
    assert!(fs::create_dir_all(context.join("pack.md")).is_ok());

    let report = inspect_session_layout(&root);
    assert!(!report.is_ok());
    assert!(report.issues().contains(&PathLayoutIssue::wrong_kind(
        "messages.jsonl".to_owned(),
        LayoutPathRole::File
    )));
    assert!(report.issues().contains(&PathLayoutIssue::missing(
        "events.jsonl".to_owned(),
        LayoutPathRole::File
    )));
    assert!(report.issues().contains(&PathLayoutIssue::wrong_kind(
        "context/pack.md".to_owned(),
        LayoutPathRole::File
    )));
    assert!(report.issues().contains(&PathLayoutIssue::missing(
        "context/child/rev-1/result.md".to_owned(),
        LayoutPathRole::File
    )));
    assert!(report.issues().contains(&PathLayoutIssue::missing(
        "context/child/rev-1/artifact".to_owned(),
        LayoutPathRole::Directory
    )));
}

#[test]
fn session_layout_inspector_rejects_symlink_files_and_directories_without_following() {
    let root = clean_test_dir("session-layout-symlink");
    let outside = clean_test_dir("session-layout-symlink-outside");
    create_complete_session_layout(&root);
    write_text_file(&outside.join("state"), "running\n");
    assert!(fs::remove_file(root.join("state")).is_ok());
    assert!(symlink(outside.join("state"), root.join("state")).is_ok());
    assert!(fs::remove_dir_all(root.join("context")).is_ok());
    assert!(fs::create_dir_all(outside.join("context")).is_ok());
    assert!(symlink(outside.join("context"), root.join("context")).is_ok());

    let report = inspect_session_layout(&root);

    assert!(report.issues().contains(&PathLayoutIssue::wrong_kind(
        "state".to_owned(),
        LayoutPathRole::File
    )));
    assert!(!report.issues().contains(&PathLayoutIssue::invalid_value(
        "state".to_owned(),
        "running".to_owned()
    )));
    assert!(report.issues().contains(&PathLayoutIssue::wrong_kind(
        "context".to_owned(),
        LayoutPathRole::Directory
    )));
    assert_file_text(&outside.join("state"), "running\n");
}

#[test]
fn session_layout_inspector_rejects_symlink_session_root_without_following() {
    let root = clean_test_dir("session-layout-symlink-root");
    let outside = clean_test_dir("session-layout-symlink-root-outside");
    let link = root.join("default");
    create_complete_session_layout(&outside);
    write_text_file(&outside.join("state"), "running\n");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(symlink(&outside, &link).is_ok());

    let report = inspect_session_layout(&link);

    assert!(report.issues().contains(&PathLayoutIssue::wrong_kind(
        ".".to_owned(),
        LayoutPathRole::Directory
    )));
    assert!(report.issues().contains(&PathLayoutIssue::missing(
        "state".to_owned(),
        LayoutPathRole::File
    )));
    assert!(!report.issues().contains(&PathLayoutIssue::invalid_value(
        "state".to_owned(),
        "running".to_owned()
    )));
    assert_file_text(&outside.join("state"), "running\n");
}
use super::*;

#[test]
fn channel_directory_validation_accepts_nlink_one() {
    let directory = clean_test_dir("child-channel-nlink-one");
    assert!(fs::create_dir_all(&directory).is_ok());
    let mut stat = ok!(nix::sys::stat::stat(directory.as_path()));
    stat.st_nlink = 1;
    assert!(crate::runtime::record::child::is_plain_channel_directory(
        &stat
    ));
}

#[test]
fn finish_child_result_is_ordered_idempotent_and_terminal_safe() {
    let session = clean_test_dir("finish-child-result");
    create_complete_session_layout(&session);
    let receipt = publish_child_handoff(&session, "worker", "worker", "run", "work").ok();
    assert!(receipt.is_some());
    let Some(receipt) = receipt else { return };
    assert!(claim_child_handoff_active(&receipt, "worker", "run").is_ok());

    assert!(
        crate::finish_child_result(
            &receipt,
            "worker",
            "run",
            ChildContextStatus::Done,
            "answer",
            "",
        )
        .is_ok()
    );
    let channel = receipt.path();
    assert_file_text(&channel.join("result.md"), "answer\n");
    assert_file_text(&channel.join("refs.jsonl"), "");
    assert_file_text(&channel.join("status"), "done\n");
    assert!(
        crate::finish_child_result(
            &receipt,
            "worker",
            "run",
            ChildContextStatus::Done,
            "answer",
            "",
        )
        .is_ok()
    );
    assert_eq!(
        crate::finish_child_result(
            &receipt,
            "worker",
            "run",
            ChildContextStatus::Error,
            "other",
            "",
        ),
        Err(ChildContextRecordError::InvalidStatus)
    );
}

#[test]
fn exclusive_finish_rejects_same_owner_without_mutation() {
    let session = clean_test_dir("finish-child-exclusive-same-owner");
    create_complete_session_layout(&session);
    let receipt = ok!(publish_child_handoff(
        &session, "worker", "worker", "run", "work"
    ));
    assert!(claim_child_handoff_active(&receipt, "worker", "run").is_ok());
    assert_eq!(
        crate::runtime::record::child::finish_child_result_exclusive(
            &receipt,
            "worker",
            "run",
            ChildContextStatus::Done,
            "answer",
            "",
        ),
        Err(ChildContextRecordError::CannotRecord)
    );
    assert_file_text(&receipt.path().join("result.md"), "");
    assert_file_text(&receipt.path().join("status"), "active\n");
}

#[test]
fn exclusive_cancelled_finish_is_payload_bound_and_idempotent() {
    if !nix::unistd::Uid::effective().is_root() {
        return;
    }
    let session = clean_test_dir("finish-child-exclusive-cancelled");
    create_complete_session_layout(&session);
    let receipt = ok!(publish_child_handoff(
        &session, "worker", "worker", "run", "work"
    ));
    assert!(claim_child_handoff_active(&receipt, "worker", "run").is_ok());
    assert!(
        nix::unistd::chown(
            receipt.path(),
            Some(nix::unistd::Uid::from_raw(65_534)),
            Some(nix::unistd::Gid::from_raw(65_534)),
        )
        .is_ok()
    );
    for _ in 0..2 {
        assert_eq!(
            crate::runtime::record::child::finish_child_result_exclusive(
                &receipt,
                "worker",
                "run",
                ChildContextStatus::Cancelled,
                "cancelled",
                "",
            ),
            Ok(())
        );
    }
    assert_eq!(
        crate::runtime::record::child::finish_child_result_exclusive(
            &receipt,
            "worker",
            "run",
            ChildContextStatus::Cancelled,
            "different cancellation",
            "",
        ),
        Err(ChildContextRecordError::InvalidStatus)
    );
}

#[test]
fn finish_child_result_preserves_non_root_artifact_metadata() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let uid = nix::unistd::Uid::current().as_raw();
    let gid = nix::unistd::Gid::current().as_raw();
    assert_ne!(uid, 0, "this metadata regression test must run as non-root");
    for (status, suffix) in [
        (ChildContextStatus::Done, "done"),
        (ChildContextStatus::Error, "error"),
        (ChildContextStatus::Cancelled, "cancelled"),
    ] {
        let session = clean_test_dir(&format!("finish-child-metadata-{suffix}"));
        create_complete_session_layout(&session);
        let receipt = ok!(publish_child_handoff(
            &session, "worker", "worker", "run", "work"
        ));
        assert!(claim_child_handoff_active(&receipt, "worker", "run").is_ok());
        assert!(fs::set_permissions(receipt.path(), fs::Permissions::from_mode(0o751)).is_ok());
        let channel_metadata = ok!(fs::symlink_metadata(receipt.path()));
        for (file, mode) in [
            ("result.md", 0o640),
            ("refs.jsonl", 0o604),
            ("status", 0o660),
        ] {
            assert!(
                fs::set_permissions(receipt.path().join(file), fs::Permissions::from_mode(mode))
                    .is_ok()
            );
        }
        assert!(crate::finish_child_result(&receipt, "worker", "run", status, suffix, "").is_ok());
        let restored_channel = ok!(fs::symlink_metadata(receipt.path()));
        assert_eq!(
            (
                restored_channel.uid(),
                restored_channel.gid(),
                restored_channel.mode() & 0o7777,
            ),
            (
                channel_metadata.uid(),
                channel_metadata.gid(),
                channel_metadata.mode() & 0o7777,
            )
        );
        for (file, mode) in [
            ("result.md", 0o640),
            ("refs.jsonl", 0o604),
            ("status", 0o660),
        ] {
            let metadata = ok!(fs::symlink_metadata(receipt.path().join(file)));
            assert_eq!((metadata.uid(), metadata.gid()), (uid, gid));
            assert_eq!(metadata.mode() & 0o7777, mode);
        }
    }
}

#[test]
fn finish_child_result_rejects_target_replacement_and_cleans_temporary() {
    use crate::runtime::record::child::{ChildFinishStage, finish_child_result_with_hook};
    let session = clean_test_dir("finish-child-target-race");
    create_complete_session_layout(&session);
    let receipt = ok!(publish_child_handoff(
        &session, "worker", "worker", "run", "work"
    ));
    assert!(claim_child_handoff_active(&receipt, "worker", "run").is_ok());
    let channel = receipt.path().to_path_buf();
    let result = finish_child_result_with_hook(
        &receipt,
        "worker",
        "run",
        ChildContextStatus::Done,
        "answer",
        "",
        |stage| {
            if stage == ChildFinishStage::BeforeResultPublish {
                fs::rename(channel.join("result.md"), channel.join("old-result"))
                    .and_then(|()| fs::write(channel.join("result.md"), "replacement\n"))
                    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
            }
            Ok(())
        },
    );
    assert_eq!(result, Err(ChildContextRecordError::CannotRecord));
    assert_file_text(&channel.join("result.md"), "replacement\n");
    let entries = ok!(fs::read_dir(&channel));
    assert!(!entries.filter_map(Result::ok).any(|entry| {
        entry
            .file_name()
            .to_string_lossy()
            .starts_with(".result.md.finish-")
    }));
    assert_file_text(&channel.join("status"), "active\n");
}

#[test]
fn finish_child_result_exchange_conflict_preserves_unowned_artifacts() {
    use crate::runtime::record::child::{ChildFinishStage, finish_child_result_with_hook};
    let session = clean_test_dir("finish-child-exchange-conflict");
    create_complete_session_layout(&session);
    let receipt = ok!(publish_child_handoff(
        &session, "worker", "worker", "run", "work"
    ));
    assert!(claim_child_handoff_active(&receipt, "worker", "run").is_ok());
    let channel = receipt.path().to_path_buf();
    let result = finish_child_result_with_hook(
        &receipt,
        "worker",
        "run",
        ChildContextStatus::Done,
        "answer",
        "",
        |stage| {
            if stage == ChildFinishStage::AfterResultRecheck {
                fs::rename(channel.join("result.md"), channel.join("original-result"))
                    .and_then(|()| fs::write(channel.join("result.md"), "racer\n"))
                    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
            }
            Ok(())
        },
    );
    assert_eq!(result, Err(ChildContextRecordError::CannotRecord));
    assert_file_text(&channel.join("result.md"), "racer\n");
    assert_file_text(&channel.join("original-result"), "");
    let temporary = ok!(fs::read_dir(&channel))
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".result.md.finish-")
        });
    assert!(temporary.is_none());
}

#[test]
fn finish_child_result_does_not_unlink_replaced_displaced_name() {
    use crate::runtime::record::child::{ChildFinishStage, finish_child_result_with_hook};
    let session = clean_test_dir("finish-child-temp-cleanup-race");
    create_complete_session_layout(&session);
    let receipt = ok!(publish_child_handoff(
        &session, "worker", "worker", "run", "work"
    ));
    assert!(claim_child_handoff_active(&receipt, "worker", "run").is_ok());
    let channel = receipt.path().to_path_buf();
    let result = finish_child_result_with_hook(
        &receipt,
        "worker",
        "run",
        ChildContextStatus::Done,
        "answer",
        "",
        |stage| {
            if stage == ChildFinishStage::BeforeResultCleanup {
                let temporary = fs::read_dir(&channel)
                    .map_err(|_error| ChildContextRecordError::CannotRecord)?
                    .filter_map(Result::ok)
                    .find(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with(".finish-quarantine-")
                    })
                    .ok_or(ChildContextRecordError::CannotRecord)?
                    .path();
                fs::rename(&temporary, channel.join("displaced-result"))
                    .and_then(|()| fs::write(&temporary, "unowned\n"))
                    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
            }
            Ok(())
        },
    );
    assert_eq!(result, Err(ChildContextRecordError::CannotRecord));
    assert_file_text(&channel.join("result.md"), "answer\n");
    assert_file_text(&channel.join("displaced-result"), "");
    let temporary = ok!(fs::read_dir(&channel))
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".finish-quarantine-")
        });
    assert!(temporary.is_some());
    if let Some(temporary) = temporary {
        assert_file_text(&temporary.path(), "unowned\n");
    }
}

#[test]
fn finish_child_result_surfaces_quarantine_unlink_failure() {
    use crate::runtime::record::child::{ChildFinishStage, finish_child_result_with_hook};
    use std::os::unix::fs::PermissionsExt;
    let session = clean_test_dir("finish-child-unlink-failure");
    create_complete_session_layout(&session);
    let receipt = ok!(publish_child_handoff(
        &session, "worker", "worker", "run", "work"
    ));
    assert!(claim_child_handoff_active(&receipt, "worker", "run").is_ok());
    let channel = receipt.path().to_path_buf();
    let result = finish_child_result_with_hook(
        &receipt,
        "worker",
        "run",
        ChildContextStatus::Done,
        "answer",
        "",
        |stage| {
            if stage == ChildFinishStage::BeforeResultCleanup {
                fs::set_permissions(&channel, fs::Permissions::from_mode(0o500))
                    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
            }
            Ok(())
        },
    );
    assert!(fs::set_permissions(&channel, fs::Permissions::from_mode(0o700)).is_ok());
    assert_eq!(result, Err(ChildContextRecordError::CannotRecord));
    assert_file_text(&channel.join("result.md"), "answer\n");
    assert!(
        ok!(fs::read_dir(&channel))
            .filter_map(Result::ok)
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".finish-quarantine-")
            })
    );
    assert_file_text(&channel.join("status"), "active\n");
}

#[test]
fn finish_child_result_rejects_replaced_channel_and_metadata() {
    let session = clean_test_dir("finish-child-replaced");
    create_complete_session_layout(&session);
    let receipt = publish_child_handoff(&session, "worker", "worker", "run", "work").ok();
    assert!(receipt.is_some());
    let Some(receipt) = receipt else { return };
    assert!(claim_child_handoff_active(&receipt, "worker", "run").is_ok());
    let channel = receipt.path().to_path_buf();
    assert!(fs::remove_file(channel.join("agent")).is_ok());
    assert!(symlink("/dev/null", channel.join("agent")).is_ok());
    assert_eq!(
        crate::finish_child_result(
            &receipt,
            "worker",
            "run",
            ChildContextStatus::Done,
            "answer",
            "",
        ),
        Err(ChildContextRecordError::CannotRecord)
    );
    assert_file_text(&channel.join("status"), "active\n");

    assert!(fs::remove_file(channel.join("agent")).is_ok());
    assert!(fs::remove_dir_all(&channel).is_ok());
    assert!(fs::create_dir_all(&channel).is_ok());
    assert_eq!(
        crate::finish_child_result(
            &receipt,
            "worker",
            "run",
            ChildContextStatus::Done,
            "answer",
            "",
        ),
        Err(ChildContextRecordError::CannotRecord)
    );
}

#[test]
fn finish_child_faults_never_publish_terminal_status() {
    use crate::runtime::record::child::{ChildFinishStage, finish_child_result_with_hook};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    for fail in [
        ChildFinishStage::AfterResultPublish,
        ChildFinishStage::AfterRefsPublish,
        ChildFinishStage::BeforeStatus,
    ] {
        let session = clean_test_dir(&format!("finish-child-fault-{fail:?}"));
        create_complete_session_layout(&session);
        let receipt = publish_child_handoff(&session, "worker", "worker", "run", "work").ok();
        assert!(receipt.is_some());
        let Some(receipt) = receipt else { return };
        assert!(claim_child_handoff_active(&receipt, "worker", "run").is_ok());
        assert!(fs::set_permissions(receipt.path(), fs::Permissions::from_mode(0o751)).is_ok());
        let before = ok!(fs::symlink_metadata(receipt.path()));
        let result = finish_child_result_with_hook(
            &receipt,
            "worker",
            "run",
            ChildContextStatus::Done,
            "answer",
            "",
            |stage| {
                if stage == fail {
                    Err(ChildContextRecordError::CannotRecord)
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(result, Err(ChildContextRecordError::CannotRecord));
        assert_file_text(&receipt.path().join("status"), "active\n");
        let after = ok!(fs::symlink_metadata(receipt.path()));
        assert_eq!(
            (after.uid(), after.gid(), after.mode() & 0o7777),
            (before.uid(), before.gid(), before.mode() & 0o7777)
        );
    }
}

#[test]
fn finish_child_result_surfaces_channel_restore_path_conflict() {
    use crate::runtime::record::child::{ChildFinishStage, finish_child_result_with_hook};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let session = clean_test_dir("finish-child-lease-restore-conflict");
    create_complete_session_layout(&session);
    let receipt = ok!(publish_child_handoff(
        &session, "worker", "worker", "run", "work"
    ));
    assert!(claim_child_handoff_active(&receipt, "worker", "run").is_ok());
    assert!(fs::set_permissions(receipt.path(), fs::Permissions::from_mode(0o751)).is_ok());
    let before = ok!(fs::symlink_metadata(receipt.path()));
    let channel = receipt.path().to_path_buf();
    let moved = channel.with_extension("leased");
    let result = finish_child_result_with_hook(
        &receipt,
        "worker",
        "run",
        ChildContextStatus::Done,
        "answer",
        "",
        |stage| {
            if stage == ChildFinishStage::BeforeResultPublish {
                fs::rename(&channel, &moved)
                    .and_then(|()| fs::create_dir_all(&channel))
                    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
            }
            Ok(())
        },
    );
    assert_eq!(result, Err(ChildContextRecordError::CannotRecord));
    let restored = ok!(fs::symlink_metadata(&moved));
    assert_eq!(
        (restored.uid(), restored.gid(), restored.mode() & 0o7777),
        (before.uid(), before.gid(), before.mode() & 0o7777)
    );
    assert!(channel.is_dir());
}

#[test]
fn finish_child_cancelled_race_is_not_overwritten() {
    use crate::runtime::record::child::{ChildFinishStage, finish_child_result_with_hook};
    let session = clean_test_dir("finish-child-cancel-race");
    create_complete_session_layout(&session);
    let receipt = publish_child_handoff(&session, "worker", "worker", "run", "work").ok();
    assert!(receipt.is_some());
    let Some(receipt) = receipt else { return };
    assert!(claim_child_handoff_active(&receipt, "worker", "run").is_ok());
    let channel = receipt.path().to_path_buf();
    let result = finish_child_result_with_hook(
        &receipt,
        "worker",
        "run",
        ChildContextStatus::Done,
        "answer",
        "",
        |stage| {
            if stage == ChildFinishStage::BeforeStatus {
                let temporary = channel.join(".cancelled");
                fs::write(&temporary, "cancelled\n")
                    .and_then(|()| fs::rename(temporary, channel.join("status")))
                    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
            }
            Ok(())
        },
    );
    assert_eq!(result, Err(ChildContextRecordError::InvalidStatus));
    assert_file_text(&channel.join("status"), "cancelled\n");
}

#[test]
fn concurrent_finishers_never_mix_terminal_payloads() {
    let session = clean_test_dir("finish-child-concurrent");
    create_complete_session_layout(&session);
    let receipt = publish_child_handoff(&session, "worker", "worker", "run", "work").ok();
    assert!(receipt.is_some());
    let Some(receipt) = receipt else { return };
    assert!(claim_child_handoff_active(&receipt, "worker", "run").is_ok());
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let mut joins = Vec::new();
    for (status, text) in [
        (ChildContextStatus::Done, "done"),
        (ChildContextStatus::Error, "error"),
    ] {
        let receipt = receipt.clone();
        let barrier = std::sync::Arc::clone(&barrier);
        joins.push(std::thread::spawn(move || {
            barrier.wait();
            crate::finish_child_result(&receipt, "worker", "run", status, text, "")
        }));
    }
    barrier.wait();
    let results = joins
        .into_iter()
        .filter_map(|join| join.join().ok())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let status = fs::read_to_string(receipt.path().join("status")).ok();
    let result = fs::read_to_string(receipt.path().join("result.md")).ok();
    assert!(matches!(
        (status.as_deref(), result.as_deref()),
        (Some("done\n"), Some("done\n")) | (Some("error\n"), Some("error\n"))
    ));
}

#[test]
fn terminal_status_read_is_receipt_bound_to_channel_fields_and_status() {
    let session = clean_test_dir("child-terminal-receipt");
    create_complete_session_layout(&session);
    let receipt = publish_child_handoff(&session, "worker", "worker", "run", "work").ok();
    assert!(receipt.is_some());
    let Some(receipt) = receipt else { return };
    assert!(claim_child_handoff_active(&receipt, "worker", "run").is_ok());
    assert!(
        crate::finish_child_result(
            &receipt,
            "worker",
            "run",
            ChildContextStatus::Done,
            "done",
            ""
        )
        .is_ok()
    );
    assert_eq!(
        crate::read_child_terminal_status(&receipt, "worker", "run"),
        Ok(ChildContextStatus::Done)
    );

    let status = receipt.path().join("status");
    assert!(fs::remove_file(&status).is_ok());
    assert!(symlink("/dev/null", &status).is_ok());
    assert_eq!(
        crate::read_child_terminal_status(&receipt, "worker", "run"),
        Err(ChildContextRecordError::CannotRecord)
    );
}

#[test]
fn terminal_status_read_rejects_replaced_agent_or_channel() {
    let session = clean_test_dir("child-terminal-replaced");
    create_complete_session_layout(&session);
    let receipt = publish_child_handoff(&session, "worker", "worker", "run", "work").ok();
    assert!(receipt.is_some());
    let Some(receipt) = receipt else { return };
    assert!(claim_child_handoff_active(&receipt, "worker", "run").is_ok());
    assert!(
        crate::finish_child_result(
            &receipt,
            "worker",
            "run",
            ChildContextStatus::Error,
            "error",
            ""
        )
        .is_ok()
    );
    write_text_file(&receipt.path().join("agent"), "other\n");
    assert_eq!(
        crate::read_child_terminal_status(&receipt, "worker", "run"),
        Err(ChildContextRecordError::CannotRecord)
    );
    let channel = receipt.path().to_path_buf();
    assert!(fs::remove_dir_all(&channel).is_ok());
    assert!(fs::create_dir_all(&channel).is_ok());
    assert_eq!(
        crate::read_child_terminal_status(&receipt, "worker", "run"),
        Err(ChildContextRecordError::CannotRecord)
    );
}

#[test]
fn matching_home_owner_does_not_require_chown_support() {
    let directory = fs::File::open("/proc/self").ok();
    assert!(directory.is_some());
    let Some(directory) = directory else { return };
    let metadata = directory.metadata().ok();
    assert!(metadata.is_some());
    let Some(metadata) = metadata else { return };
    assert_eq!(
        chown_home_entry(&directory, metadata.uid(), metadata.gid()),
        Ok(())
    );
}
