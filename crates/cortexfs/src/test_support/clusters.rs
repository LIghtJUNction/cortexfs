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
            fs.node_content(fs.path_inode([
                "clusters",
                "local",
                "workers",
                "local-worker",
                file,
            ])?)?,
            value,
            "worker runtime file mismatch: {file}"
        );
    }
    Ok(())
}

#[test]
fn projection_exposes_cluster_worker_and_queue_shape() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    assert_eq!(
        fs.lookup_path(["clusters", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["clusters", "list"])
            .and_then(crate::Node::content),
        Some("local\n")
    );
    assert_eq!(
        fs.lookup_path(["clusters", "local", "state"])
            .and_then(crate::Node::content),
        Some("idle\n")
    );
    assert_eq!(
        fs.lookup_path(["clusters", "local", "context"])
            .and_then(crate::Node::content),
        Some("local:cluster_r:cluster_t:s0\n")
    );
    assert_eq!(
        fs.lookup_path(["clusters", "local", "agents", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["clusters", "local", "agents", "list"])
            .and_then(crate::Node::content),
        Some("helper\n")
    );
    assert_eq!(
        fs.lookup_path(["clusters", "local", "workers", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["clusters", "local", "workers", "list"])
            .and_then(crate::Node::content),
        Some("local-worker\n")
    );
    assert_eq!(
        fs.lookup_path([
            "clusters",
            "local",
            "workers",
            "local-worker",
            "capabilities"
        ])
        .and_then(crate::Node::content),
        Some("fuse\nprovider.registry\nlocal_runtime\n")
    );
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
        fs.lookup_path(["clusters", "local", "queues", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["clusters", "local", "queues", "list"])
            .and_then(crate::Node::content),
        Some("default\n")
    );
    assert_eq!(
        fs.lookup_path(["clusters", "local", "queues", "default", "states"])
            .and_then(crate::Node::content),
        Some("pending\nrunning\ndone\nfailed\n")
    );
    for directory in ["pending", "running", "done", "failed"] {
        assert!(
            fs.lookup_path(["clusters", "local", "queues", "default", directory])
                .is_some(),
            "cluster queue state directory must exist"
        );
    }
    assert!(
        fs.lookup_path(["clusters", "local", "tasks"]).is_some(),
        "cluster task namespace must exist"
    );
    assert!(
        fs.lookup_path(["clusters", "local", "control", "rebalance"])
            .is_some(),
        "cluster rebalance control node must exist"
    );
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
            fs.lookup_path(["clusters", "local", "state"])
                .map(crate::Node::inode)
                .map(|inode| fs.node_content(inode))
                .transpose()?,
            Some(expected_state.to_owned())
        );
        assert_eq!(
            fs.node_content(fs.control_file_inode("last_control")?)?,
            format!("clusters/local/{control_name}\n")
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
fn projection_exposes_shared_collaboration_space() {
    let fs = CortexFs::new();

    assert_eq!(
        fs.lookup_path(["spaces", "shared", "project-a", "context"])
            .and_then(crate::Node::content),
        Some("local:shared_project_a:object_r:shared_space_t:s0:c_project_a\n")
    );
    assert_eq!(
        fs.lookup_path([
            "spaces",
            "shared",
            "project-a",
            "collab",
            "blackboard",
            "state"
        ])
        .and_then(crate::Node::content),
        Some("open\n")
    );
    assert!(
        fs.lookup_path([
            "spaces",
            "shared",
            "project-a",
            "collab",
            "blackboard",
            "artifacts"
        ])
        .is_some(),
        "blackboard artifacts directory must exist"
    );
    assert_eq!(
        fs.lookup_path([
            "spaces",
            "shared",
            "project-a",
            "collab",
            "tasks",
            "demo",
            "owner"
        ])
        .and_then(crate::Node::content),
        Some("agents/helper\n")
    );
    assert!(
        fs.lookup_path([
            "spaces",
            "shared",
            "project-a",
            "collab",
            "tasks",
            "demo",
            "claims"
        ])
        .is_some(),
        "collab task claims directory must exist"
    );
    assert_eq!(
        fs.lookup_path([
            "spaces",
            "shared",
            "project-a",
            "collab",
            "handoffs",
            "demo",
            "state"
        ])
        .and_then(crate::Node::content),
        Some("ready\n")
    );
    assert_eq!(
        fs.lookup_path([
            "spaces",
            "shared",
            "project-a",
            "collab",
            "locks",
            "demo",
            "state"
        ])
        .and_then(crate::Node::content),
        Some("released\n")
    );
    assert!(
        fs.lookup_path(["spaces", "shared", "project-a", "collab", "locks", "leases"])
            .is_some(),
        "collab lock lease submission directory must exist"
    );
    assert!(
        fs.lookup_path([
            "spaces",
            "shared",
            "project-a",
            "collab",
            "decisions",
            "000001.md"
        ])
        .is_some(),
        "collab decisions must be visible"
    );
}

#[test]
fn projection_exposes_collab_handoff_context_refs() {
    let fs = CortexFs::new();

    assert_eq!(
        fs.lookup_path([
            "spaces",
            "shared",
            "project-a",
            "collab",
            "handoffs",
            "demo",
            "from"
        ])
        .and_then(crate::Node::content),
        Some("agents/helper\n")
    );
    assert_eq!(
        fs.lookup_path([
            "spaces",
            "shared",
            "project-a",
            "collab",
            "handoffs",
            "demo",
            "to"
        ])
        .and_then(crate::Node::content),
        Some("clusters/local/workers/local-worker\n")
    );
    assert!(
        fs.lookup_path([
            "spaces",
            "shared",
            "project-a",
            "collab",
            "handoffs",
            "demo",
            "summary.md"
        ])
        .and_then(crate::Node::content)
        .is_some_and(|summary| summary.contains("Shared context")),
        "handoff summary must be readable"
    );
    assert_eq!(
        fs.lookup_path([
            "spaces",
            "shared",
            "project-a",
            "collab",
            "handoffs",
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
    fs.create_staged_collab_claim("helper.tmp", "agents/helper\n")?;
    fs.submit_collab_claim("helper.tmp", "helper.claim")?;

    let claims = fs
        .tree
        .path_inode(crate::SHARED_PROJECT_A_DEMO_CLAIMS_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert_eq!(
        runtime
            .lookup_child(claims, "helper.claim")
            .and_then(crate::Node::content),
        Some("agents/helper\n")
    );
    drop(runtime);
    assert_eq!(
        fs.lookup_path([
            "spaces",
            "shared",
            "project-a",
            "collab",
            "tasks",
            "demo",
            "owner"
        ])
        .map(crate::Node::inode)
        .map(|inode| fs.node_content(inode))
        .transpose()?,
        Some("agents/helper\n".to_owned())
    );
    assert_eq!(
        fs.lookup_path([
            "spaces",
            "shared",
            "project-a",
            "collab",
            "tasks",
            "demo",
            "state"
        ])
        .map(crate::Node::inode)
        .map(|inode| fs.node_content(inode))
        .transpose()?,
        Some("claimed\n".to_owned())
    );
    let events = fs
        .lookup_path([
            "spaces",
            "shared",
            "project-a",
            "collab",
            "tasks",
            "demo",
            "events.jsonl",
        ])
        .map(crate::Node::inode)
        .map(|inode| fs.node_content(inode))
        .transpose()?
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(events.contains("\"event\":\"created\""));
    assert!(events.contains("\"event\":\"claimed\""));
    assert!(events.contains("\"agent\":\"agents/helper\""));
    assert!(
        fs.node_content(fs.audit_events_inode()?)?
            .contains("\"format\":\"collab.task.claim\"")
    );
    Ok(())
}

#[test]
fn collab_lock_lease_uses_atomic_rename_and_materializes_lock() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    fs.create_staged_collab_lock_lease("handoff.tmp", "clusters/local/workers/local-worker\n")?;
    fs.submit_collab_lock_lease("handoff.tmp", "handoff.lease")?;

    let leases = fs
        .tree
        .path_inode(crate::SHARED_PROJECT_A_LOCK_LEASES_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert_eq!(
        runtime
            .lookup_child(leases, "handoff.lease")
            .and_then(crate::Node::content),
        Some("clusters/local/workers/local-worker\n")
    );
    drop(runtime);
    let locks = fs
        .tree
        .path_inode(&["spaces", "shared", "project-a", "collab", "locks"])
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
        Some("clusters/local/workers/local-worker\n")
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
    {
        let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        assert!(
            runtime.lookup_child(done, "task-001.resp.json").is_none(),
            "cluster task submit queues work until control/drain"
        );
        drop(runtime);
    }

    let drain = fs.control_file_inode("drain")?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
    }

    let tasks = fs
        .tree
        .path_inode(crate::CLUSTER_TASKS_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
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
    let result = runtime
        .lookup_child(task_dir, "result.resp.json")
        .and_then(crate::Node::content)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert!(result.contains("\"status\":\"done\""));
    assert!(result.contains("cluster visible"));
    assert!(
        runtime
            .lookup_child(done, "task-001.resp.json")
            .and_then(crate::Node::content)
            .is_some_and(|done_result| done_result.contains("local-worker"))
    );
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
            current_task: "task-001\n",
        },
    )?;
    assert!(
        fs.node_content(fs.audit_events_inode()?)?
            .contains("\"format\":\"cluster.task\"")
    );
    Ok(())
}
