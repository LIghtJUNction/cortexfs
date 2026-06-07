use crate::CortexFs;

struct WorkerRuntimeExpectation<'a> {
    state: &'a str,
    heartbeat: &'a str,
    load: &'a str,
    current_task: &'a str,
}

fn assert_worker_runtime(
    fs: &CortexFs,
    expected: &WorkerRuntimeExpectation<'_>,
) -> fuse3::Result<()> {
    for (file, value) in [
        ("state", expected.state),
        ("heartbeat", expected.heartbeat),
        ("load", expected.load),
        ("current_task", expected.current_task),
    ] {
        assert_eq!(
            fs.node_content(fs.resolve_path_inode([
                "cluster",
                "local",
                "worker",
                "local-worker",
                file,
            ])?)?,
            value,
            "worker runtime file mismatch: {file}"
        );
    }
    Ok(())
}

fn assert_worker_runtime_files_are_runtime_owned(fs: &CortexFs) -> fuse3::Result<()> {
    let worker = fs.path_inode(["cluster", "local", "worker", "local-worker"])?;
    let entries = fs.children(worker);
    for name in ["state", "heartbeat", "load", "current_task"] {
        assert!(
            fs.tree
                .path_inode(&["cluster", "local", "worker", "local-worker", name])
                .is_none(),
            "cluster worker {name} must be runtime-owned, not a static placeholder"
        );
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.name.to_str() == Some(name))
                .count(),
            1,
            "cluster worker directory must expose one {name} entry"
        );
    }
    Ok(())
}

fn assert_cluster_state_is_runtime_owned(fs: &CortexFs) -> fuse3::Result<()> {
    let cluster = fs.path_inode(["cluster", "local"])?;
    let entries = fs.children(cluster);
    assert!(
        fs.tree.path_inode(&["cluster", "local", "state"]).is_none(),
        "cluster/local/state must be runtime-owned, not a static placeholder"
    );
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.name.to_str() == Some("state"))
            .count(),
        1,
        "cluster/local must expose one state entry"
    );
    Ok(())
}

fn assert_cluster_control_files_are_runtime_owned(fs: &CortexFs) -> fuse3::Result<()> {
    let control = fs.path_inode(["cluster", "local", "control"])?;
    let entries = fs.children(control);
    for name in ["rebalance", "drain", "pause"] {
        assert!(
            fs.tree
                .path_inode(&["cluster", "local", "control", name])
                .is_none(),
            "cluster control {name} must be runtime-owned, not a static placeholder"
        );
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.name.to_str() == Some(name))
                .count(),
            1,
            "cluster/local/control must expose one {name} entry"
        );
        assert_eq!(
            fs.node_attr(fs.resolve_path_inode(["cluster", "local", "control", name,])?)?
                .perm,
            0o222
        );
    }
    Ok(())
}

fn assert_collab_task_runtime_files_are_runtime_owned(fs: &CortexFs) -> fuse3::Result<()> {
    let task = fs.path_inode(["shared", "project-a", "collab", "task", "demo"])?;
    let entries = fs.children(task);
    for (name, expected) in [
        ("owner", "agent/helper\n"),
        ("state", "open\n"),
        (
            "events.jsonl",
            "{\"event\":\"created\",\"agent\":\"helper\",\"state\":\"open\"}\n",
        ),
    ] {
        assert!(
            fs.tree
                .path_inode(&["shared", "project-a", "collab", "task", "demo", name])
                .is_none(),
            "collab task {name} must be runtime-owned, not a static placeholder"
        );
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.name.to_str() == Some(name))
                .count(),
            1,
            "collab task directory must expose one {name} entry"
        );
        assert_eq!(
            fs.node_content(fs.resolve_path_inode([
                "shared",
                "project-a",
                "collab",
                "task",
                "demo",
                name,
            ])?)?,
            expected
        );
    }
    Ok(())
}

fn assert_blackboard_runtime_files_are_runtime_owned(fs: &CortexFs) -> fuse3::Result<()> {
    let blackboard = fs.path_inode(["shared", "project-a", "collab", "blackboard"])?;
    let entries = fs.children(blackboard);
    for (name, expected) in [
        (
            "notes.jsonl",
            "{\"agent\":\"helper\",\"note\":\"project collaboration space initialized\"}\n",
        ),
        ("state", "open\n"),
    ] {
        assert!(
            fs.tree
                .path_inode(&["shared", "project-a", "collab", "blackboard", name])
                .is_none(),
            "collab blackboard {name} must be runtime-owned, not a static placeholder"
        );
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.name.to_str() == Some(name))
                .count(),
            1,
            "collab blackboard directory must expose one {name} entry"
        );
        assert_eq!(
            fs.node_content(fs.resolve_path_inode([
                "shared",
                "project-a",
                "collab",
                "blackboard",
                name,
            ])?)?,
            expected
        );
    }
    Ok(())
}

fn assert_collab_handoff_state_is_runtime_owned(fs: &CortexFs) -> fuse3::Result<()> {
    let handoff = fs.path_inode(["shared", "project-a", "collab", "handoff", "demo"])?;
    let entries = fs.children(handoff);
    assert!(
        fs.tree
            .path_inode(&["shared", "project-a", "collab", "handoff", "demo", "state"])
            .is_none(),
        "collab handoff state must be runtime-owned, not a static placeholder"
    );
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.name.to_str() == Some("state"))
            .count(),
        1,
        "collab handoff directory must expose one state entry"
    );
    assert_eq!(
        fs.node_content(fs.resolve_path_inode([
            "shared",
            "project-a",
            "collab",
            "handoff",
            "demo",
            "state",
        ])?)?,
        "ready\n"
    );
    Ok(())
}

fn assert_collab_lock_demo_files_are_runtime_owned(fs: &CortexFs) -> fuse3::Result<()> {
    let lock = fs.path_inode(["shared", "project-a", "collab", "lock", "demo"])?;
    let entries = fs.children(lock);
    for (name, expected) in [
        ("owner", "agent/helper\n"),
        ("state", "released\n"),
        ("lease_expires", "\n"),
    ] {
        assert!(
            fs.tree
                .path_inode(&["shared", "project-a", "collab", "lock", "demo", name])
                .is_none(),
            "collab lock demo {name} must be runtime-owned, not a static placeholder"
        );
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.name.to_str() == Some(name))
                .count(),
            1,
            "collab lock demo directory must expose one {name} entry"
        );
        assert_eq!(
            fs.node_content(fs.resolve_path_inode([
                "shared",
                "project-a",
                "collab",
                "lock",
                "demo",
                name,
            ])?)?,
            expected
        );
    }
    Ok(())
}

fn assert_cluster_task_queue_after_submit(
    fs: &CortexFs,
    pending: fuse3::Inode,
    running: fuse3::Inode,
    done: fuse3::Inode,
) -> fuse3::Result<()> {
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert!(
        runtime
            .lookup_child(pending, "task-001.req.json")
            .and_then(crate::Node::content)
            .is_some_and(|spec| spec.contains("cluster visible")),
        "cluster task submit must materialize the pending queue entry"
    );
    assert!(
        runtime.lookup_child(done, "task-001.resp.json").is_none(),
        "cluster task submit queues work until control/drain"
    );
    assert!(
        runtime.lookup_child(running, "task-001.req.json").is_none(),
        "cluster task submit must not skip pending and appear running early"
    );
    drop(runtime);
    Ok(())
}

fn assert_cluster_task_queue_after_drain(
    fs: &CortexFs,
    pending: fuse3::Inode,
    running: fuse3::Inode,
    done: fuse3::Inode,
) -> fuse3::Result<()> {
    let tasks = fs
        .tree
        .path_inode(crate::CLUSTER_TASKS_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert!(
        runtime.lookup_child(pending, "task-001.req.json").is_none(),
        "cluster task drain must remove the pending queue entry"
    );
    assert!(
        runtime.lookup_child(running, "task-001.req.json").is_none(),
        "cluster task drain must remove the transient running queue entry"
    );
    let task_dir = runtime
        .lookup_child(tasks, "task-001")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert_eq!(
        runtime
            .lookup_child(task_dir, "state")
            .and_then(crate::Node::content),
        Some("done\n")
    );
    assert_eq!(
        runtime
            .lookup_child(task_dir, "assigned_worker")
            .and_then(crate::Node::content),
        Some("local-worker\n")
    );
    assert_eq!(
        runtime
            .lookup_child(task_dir, "spec.req.json")
            .and_then(crate::Node::content),
        Some("{\"task\":\"summarize\",\"input\":\"cluster visible\"}\n")
    );
    assert_eq!(
        runtime
            .lookup_child(task_dir, "error")
            .and_then(crate::Node::content),
        Some("\n")
    );
    let result = runtime
        .lookup_child(task_dir, "result.resp.json")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(result.contains("\"status\":\"done\""));
    assert!(result.contains("cluster visible"));
    let audit = runtime
        .lookup_child(task_dir, "audit")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(audit.contains("\"event\":\"done\""));
    assert!(audit.contains("\"worker\":\"local-worker\""));
    assert!(audit.contains("\"fingerprint\":\"fnv1a64:"));
    let events = runtime
        .lookup_child(task_dir, "events.jsonl")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(events.contains("\"event\":\"running\""));
    assert!(events.contains("\"event\":\"done\""));
    assert!(
        runtime
            .lookup_child(done, "task-001.resp.json")
            .and_then(crate::Node::content)
            .is_some_and(|done_result| done_result.contains("local-worker"))
    );
    drop(runtime);
    Ok(())
}

fn assert_failed_cluster_task_queue_entry(
    runtime: &crate::RuntimeState,
    pending: fuse3::Inode,
    running: fuse3::Inode,
    done: fuse3::Inode,
    failed: fuse3::Inode,
) {
    assert!(
        runtime
            .lookup_child(pending, "task-fail.req.json")
            .is_none()
    );
    assert!(
        runtime
            .lookup_child(running, "task-fail.req.json")
            .is_none()
    );
    assert!(runtime.lookup_child(done, "task-fail.resp.json").is_none());
    assert!(
        runtime
            .lookup_child(failed, "task-fail.error")
            .and_then(crate::Node::content)
            .is_some_and(|error| error.contains("\"status\":\"failed\"")
                && error.contains("cluster task requested failure")),
        "cluster task errors must materialize in the failed queue"
    );
}

fn assert_failed_cluster_task_directory(
    runtime: &crate::RuntimeState,
    tasks: fuse3::Inode,
) -> fuse3::Result<()> {
    let task_dir = runtime
        .lookup_child(tasks, "task-fail")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    for (name, expected) in [
        ("state", "failed\n"),
        ("error", "cluster task requested failure\n"),
        ("assigned_worker", "local-worker\n"),
        (
            "spec.req.json",
            "{\"task\":\"summarize\",\"input\":\"cluster visible\",\"fail\":true}\n",
        ),
        ("result.resp.json", "\n"),
    ] {
        assert_eq!(
            runtime
                .lookup_child(task_dir, name)
                .and_then(crate::Node::content),
            Some(expected),
            "failed cluster task file mismatch: {name}"
        );
    }
    let audit = runtime
        .lookup_child(task_dir, "audit")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(audit.contains("\"event\":\"failed\""));
    assert!(audit.contains("\"worker\":\"local-worker\""));
    assert!(audit.contains("\"error\":\"cluster task requested failure\""));
    assert!(audit.contains("\"fingerprint\":\"fnv1a64:"));
    let events = runtime
        .lookup_child(task_dir, "events.jsonl")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(events.contains("\"event\":\"running\""));
    assert!(events.contains("\"event\":\"failed\""));
    Ok(())
}

#[test]
fn projection_exposes_cluster_worker_and_queue_shape() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    assert_eq!(
        fs.lookup_path(["cluster", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["cluster", "list"])
            .and_then(crate::Node::content),
        Some("local\n")
    );
    assert_cluster_state_is_runtime_owned(&fs)?;
    assert_eq!(
        fs.node_content(fs.resolve_path_inode(["cluster", "local", "state"])?)?,
        "idle\n"
    );
    assert_eq!(
        fs.lookup_path(["cluster", "local", "context"])
            .and_then(crate::Node::content),
        Some("local:cluster_r:cluster_t:s0\n")
    );
    assert_eq!(
        fs.lookup_path(["cluster", "local", "agent", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["cluster", "local", "agent", "list"])
            .and_then(crate::Node::content),
        Some("helper\n")
    );
    assert_eq!(
        fs.lookup_path(["cluster", "local", "worker", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["cluster", "local", "worker", "list"])
            .and_then(crate::Node::content),
        Some("local-worker\n")
    );
    assert_eq!(
        fs.lookup_path(["cluster", "local", "worker", "local-worker", "cap"])
            .and_then(crate::Node::content),
        Some("fuse\nprovider.registry\nlocal_runtime\n")
    );
    assert_worker_runtime_files_are_runtime_owned(&fs)?;
    assert_worker_runtime(
        &fs,
        &WorkerRuntimeExpectation {
            state: "idle\n",
            heartbeat: "\n",
            load: "0\n",
            current_task: "\n",
        },
    )?;
    assert_eq!(
        fs.lookup_path(["cluster", "local", "queue", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["cluster", "local", "queue", "list"])
            .and_then(crate::Node::content),
        Some("default\n")
    );
    assert_eq!(
        fs.lookup_path(["cluster", "local", "queue", "default", "states"])
            .and_then(crate::Node::content),
        Some("pending\nrunning\ndone\nfailed\n")
    );
    for directory in ["pending", "running", "done", "failed"] {
        assert!(
            fs.lookup_path(["cluster", "local", "queue", "default", directory])
                .is_some(),
            "cluster queue state directory must exist"
        );
    }
    assert!(
        fs.lookup_path(["cluster", "local", "task"]).is_some(),
        "cluster task namespace must exist"
    );
    assert_cluster_control_files_are_runtime_owned(&fs)?;
    assert!(
        fs.lookup_path(["cluster", "local", "worker", "local-worker"])
            .is_some()
    );
    assert!(
        fs.lookup_path(["cluster", "local", "queue", "default"])
            .is_some()
    );
    assert!(fs.lookup_path(["cluster", "local", "task"]).is_some());
    Ok(())
}

#[test]
fn cluster_control_nodes_update_state_last_control_and_audit() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    for (control_name, expected_state) in [
        ("rebalance", "rebalancing\n"),
        ("drain", "draining\n"),
        ("pause", "paused\n"),
    ] {
        let control_inode = fs.cluster_control_file_inode(control_name)?;
        {
            let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
            assert_eq!(
                runtime.write(control_inode, 0, b"1\n")?,
                2,
                "cluster control write should report accepted bytes"
            );
            drop(runtime);
        }

        assert_eq!(
            fs.node_content(fs.resolve_path_inode(["cluster", "local", "state"])?)?,
            expected_state
        );
        assert_eq!(
            fs.node_content(fs.control_file_inode("last_control")?)?,
            format!("cluster/local/{control_name}\n")
        );
        let audit = fs.node_content(fs.audit_events_inode()?)?;
        assert!(audit.contains("\"format\":\"cluster.local.control\""));
        assert!(audit.contains(&format!("\"name\":\"{control_name}\"")));
        assert!(audit.contains(&format!("\"event\":\"{}\"", expected_state.trim())));
    }
    Ok(())
}

#[test]
fn cluster_control_nodes_reject_invalid_input() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let pause = fs.cluster_control_file_inode("pause")?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;

    assert!(
        runtime.write(pause, 0, b"yes\n").is_err(),
        "cluster control accepts only 1"
    );
    assert!(
        runtime.write(pause, 1, b"1\n").is_err(),
        "cluster control requires offset zero"
    );
    drop(runtime);
    Ok(())
}

#[test]
fn projection_exposes_shared_collaboration_space() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    assert_eq!(
        fs.lookup_path(["shared", "project-a", "context"])
            .and_then(crate::Node::content),
        Some("local:shared_project_a:object_r:shared_space_t:s0:c_project_a\n")
    );
    assert!(
        fs.tree
            .path_inode(&["space", "shared", "project-a"])
            .is_none()
    );
    assert_blackboard_runtime_files_are_runtime_owned(&fs)?;
    assert!(
        fs.lookup_path(["shared", "project-a", "collab", "blackboard", "artifact"])
            .is_some(),
        "blackboard artifact directory must be the primary namespace"
    );
    assert_collab_task_runtime_files_are_runtime_owned(&fs)?;
    assert!(
        fs.lookup_path(["shared", "project-a", "collab", "task", "demo", "claim"])
            .is_some(),
        "collab task claim directory must exist"
    );
    assert_collab_handoff_state_is_runtime_owned(&fs)?;
    assert_collab_lock_demo_files_are_runtime_owned(&fs)?;
    assert!(
        fs.lookup_path(["shared", "project-a", "collab", "lock", "lease"])
            .is_some(),
        "collab lock lease submission directory must exist"
    );
    assert!(
        fs.lookup_path(["shared", "project-a", "collab", "decision", "000001.md"])
            .is_some(),
        "collab decisions must be visible"
    );
    for primary in ["task", "handoff", "lock", "decision"] {
        assert!(
            fs.lookup_path(["shared", "project-a", "collab", primary])
                .is_some(),
            "shared collab/{primary} must be the primary namespace"
        );
    }
    Ok(())
}

#[test]
fn projection_exposes_collab_handoff_context_refs() {
    let fs = CortexFs::new();

    assert_eq!(
        fs.lookup_path(["shared", "project-a", "collab", "handoff", "demo", "from"])
            .and_then(crate::Node::content),
        Some("agent/helper\n")
    );
    assert_eq!(
        fs.lookup_path(["shared", "project-a", "collab", "handoff", "demo", "to"])
            .and_then(crate::Node::content),
        Some("cluster/local/worker/local-worker\n")
    );
    assert!(
        fs.lookup_path([
            "shared",
            "project-a",
            "collab",
            "handoff",
            "demo",
            "summary.md"
        ])
        .and_then(crate::Node::content)
        .is_some_and(|summary| summary.contains("Shared context")),
        "handoff summary must be readable"
    );
    assert_eq!(
        fs.lookup_path([
            "shared",
            "project-a",
            "collab",
            "handoff",
            "demo",
            "context_refs"
        ])
        .and_then(crate::Node::content),
        Some("collab/blackboard/notes.jsonl\n")
    );
}

#[test]
fn collab_task_claim_uses_atomic_rename_and_writes_events() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_collab_claim("helper.tmp", "agent/helper\n")?;
    fs.submit_collab_claim("helper.tmp", "helper.claim")?;

    let claim_dir = fs
        .tree
        .path_inode(crate::SHARED_PROJECT_A_DEMO_CLAIM_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert_eq!(
        runtime
            .lookup_child(claim_dir, "helper.claim")
            .and_then(crate::Node::content),
        Some("agent/helper\n")
    );
    drop(runtime);
    assert_eq!(
        fs.node_content(fs.resolve_path_inode([
            "shared",
            "project-a",
            "collab",
            "task",
            "demo",
            "owner",
        ])?)?,
        "agent/helper\n"
    );
    assert_eq!(
        fs.node_content(fs.resolve_path_inode([
            "shared",
            "project-a",
            "collab",
            "task",
            "demo",
            "state",
        ])?)?,
        "claimed\n"
    );
    let events = fs.node_content(fs.resolve_path_inode([
        "shared",
        "project-a",
        "collab",
        "task",
        "demo",
        "events.jsonl",
    ])?)?;
    assert!(events.contains("\"event\":\"created\""));
    assert!(events.contains("\"event\":\"claimed\""));
    assert!(events.contains("\"agent\":\"agent/helper\""));
    assert!(
        fs.node_content(fs.audit_events_inode()?)?
            .contains("\"format\":\"collab.task.claim\"")
    );
    Ok(())
}

#[test]
fn collab_lock_lease_uses_atomic_rename_and_materializes_lock() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_collab_lock_lease("handoff.tmp", "cluster/local/worker/local-worker\n")?;
    fs.submit_collab_lock_lease("handoff.tmp", "handoff.lease")?;

    let lease_dir = fs
        .tree
        .path_inode(crate::SHARED_PROJECT_A_LOCK_LEASE_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert_eq!(
        runtime
            .lookup_child(lease_dir, "handoff.lease")
            .and_then(crate::Node::content),
        Some("cluster/local/worker/local-worker\n")
    );
    drop(runtime);
    let locks = fs
        .tree
        .path_inode(&["shared", "project-a", "collab", "lock"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let lock = runtime
        .lookup_child(locks, "handoff")
        .ok_or_else(fuse3::Errno::new_not_exist)?
        .inode();
    assert_eq!(
        runtime
            .lookup_child(lock, "owner")
            .and_then(crate::Node::content),
        Some("cluster/local/worker/local-worker\n")
    );
    assert_eq!(
        runtime
            .lookup_child(lock, "state")
            .and_then(crate::Node::content),
        Some("held\n")
    );
    assert_eq!(
        runtime
            .lookup_child(lock, "lease_expires")
            .and_then(crate::Node::content),
        Some("daemon-clock-pending\n")
    );
    drop(runtime);
    assert!(
        fs.node_content(fs.audit_events_inode()?)?
            .contains("\"format\":\"collab.lock.lease\"")
    );
    Ok(())
}

#[test]
fn cluster_task_submit_drains_to_task_result_and_done_queue() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_cluster_task(
        "task-001.tmp",
        "{\"task\":\"summarize\",\"input\":\"cluster visible\"}\n",
    )?;
    fs.submit_cluster_task("task-001.tmp", "task-001.req.json")?;

    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "1\n"
    );
    assert_worker_runtime(
        &fs,
        &WorkerRuntimeExpectation {
            state: "busy\n",
            heartbeat: "1\n",
            load: "1\n",
            current_task: "task-001\n",
        },
    )?;
    let done = fs
        .tree
        .path_inode(crate::CLUSTER_TASK_DONE_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let pending = fs
        .tree
        .path_inode(crate::CLUSTER_TASK_PENDING_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let running = fs
        .tree
        .path_inode(&["cluster", "local", "queue", "default", "running"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert_cluster_task_queue_after_submit(&fs, pending, running, done)?;

    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    assert_cluster_task_queue_after_drain(&fs, pending, running, done)?;
    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "0\n"
    );
    assert_worker_runtime(
        &fs,
        &WorkerRuntimeExpectation {
            state: "idle\n",
            heartbeat: "2\n",
            load: "0\n",
            current_task: "task-001\n",
        },
    )?;
    assert!(
        fs.node_content(fs.audit_events_inode()?)?
            .contains("\"format\":\"cluster.task\"")
    );
    Ok(())
}

#[test]
fn cluster_task_error_drains_to_failed_queue() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_cluster_task(
        "task-fail.tmp",
        "{\"task\":\"summarize\",\"input\":\"cluster visible\",\"fail\":true}\n",
    )?;
    fs.submit_cluster_task("task-fail.tmp", "task-fail.req.json")?;

    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let pending = fs
        .tree
        .path_inode(crate::CLUSTER_TASK_PENDING_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let running = fs
        .tree
        .path_inode(&["cluster", "local", "queue", "default", "running"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let done = fs
        .tree
        .path_inode(crate::CLUSTER_TASK_DONE_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let failed = fs
        .tree
        .path_inode(crate::abi::CLUSTER_TASK_FAILED_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let tasks = fs
        .tree
        .path_inode(crate::CLUSTER_TASKS_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert_failed_cluster_task_queue_entry(&runtime, pending, running, done, failed);
    assert_failed_cluster_task_directory(&runtime, tasks)?;
    drop(runtime);

    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "0\n"
    );
    assert_worker_runtime(
        &fs,
        &WorkerRuntimeExpectation {
            state: "idle\n",
            heartbeat: "2\n",
            load: "0\n",
            current_task: "task-fail\n",
        },
    )?;
    assert!(
        fs.node_content(fs.audit_events_inode()?)?
            .contains("\"format\":\"cluster.task\"")
    );
    assert!(
        fs.node_content(fs.audit_events_inode()?)?
            .contains("\"event\":\"error\"")
    );
    Ok(())
}

#[test]
fn failed_cluster_task_retry_requeues_pending_task() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_cluster_task(
        "task-retry.tmp",
        "{\"task\":\"summarize\",\"input\":\"retry visible\",\"fail\":true}\n",
    )?;
    fs.submit_cluster_task("task-retry.tmp", "task-retry.req.json")?;

    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let pending = fs
        .tree
        .path_inode(crate::CLUSTER_TASK_PENDING_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let failed = fs
        .tree
        .path_inode(crate::abi::CLUSTER_TASK_FAILED_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let tasks = fs
        .tree
        .path_inode(crate::CLUSTER_TASKS_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let retry = {
        let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let task_dir = runtime
            .lookup_child(tasks, "task-retry")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let retry = runtime
            .lookup_child(task_dir, "retry")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        assert_eq!(
            runtime
                .lookup_child(task_dir, "state")
                .and_then(crate::Node::content),
            Some("failed\n")
        );
        drop(runtime);
        retry
    };

    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(retry, 0, b"1\n")?;
    }

    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert!(
        runtime
            .lookup_child(pending, "task-retry.req.json")
            .is_some(),
        "retry must requeue the failed task into pending"
    );
    assert!(
        runtime.lookup_child(failed, "task-retry.error").is_none(),
        "retry must remove the failed queue entry"
    );
    let task_dir = runtime
        .lookup_child(tasks, "task-retry")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert_eq!(
        runtime
            .lookup_child(task_dir, "state")
            .and_then(crate::Node::content),
        Some("pending\n")
    );
    assert_eq!(
        runtime
            .lookup_child(task_dir, "error")
            .and_then(crate::Node::content),
        Some("\n"),
        "retry must clear the per-task error file"
    );
    let audit = runtime
        .lookup_child(task_dir, "audit")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(audit.contains("\"event\":\"retry\""));
    assert!(audit.contains("\"fingerprint\":\"fnv1a64:"));
    let events = runtime
        .lookup_child(task_dir, "events.jsonl")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(events.contains("\"event\":\"retry\""));
    drop(runtime);

    assert_eq!(
        fs.node_content(fs.control_file_inode("queue_depth")?)?,
        "1\n"
    );
    assert!(
        fs.node_content(fs.audit_events_inode()?)?
            .contains("\"event\":\"retry\"")
    );
    Ok(())
}

#[test]
fn failed_cluster_task_retry_rejects_invalid_control_writes() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_cluster_task(
        "task-retry-invalid.tmp",
        "{\"task\":\"summarize\",\"input\":\"retry invalid\",\"fail\":true}\n",
    )?;
    fs.submit_cluster_task("task-retry-invalid.tmp", "task-retry-invalid.req.json")?;

    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let tasks = fs
        .tree
        .path_inode(crate::CLUSTER_TASKS_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let retry = {
        let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let task_dir = runtime
            .lookup_child(tasks, "task-retry-invalid")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        runtime
            .lookup_child(task_dir, "retry")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?
    };

    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert_eq!(
        runtime.write(retry, 0, b"yes\n").map_err(i32::from),
        Err(-libc::EINVAL)
    );
    assert_eq!(
        runtime.write(retry, 1, b"1\n").map_err(i32::from),
        Err(-libc::EINVAL)
    );
    drop(runtime);
    Ok(())
}
