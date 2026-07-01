#[test]
fn agent_ps_reads_parent_status_and_pid_controls() {
    let root = clean_test_dir("ctx-agent-ps");
    let pid = std::process::id().to_string();
    create_agent_fixture(&root, "coder", "", "idle", "");
    create_agent_fixture(&root, "reviewer", "agent:coder session:default run:r1", "busy", &pid);
    create_agent_fixture(&root, "auditor", "agent:reviewer", "ready", "");

    let processes = read_agent_processes(&root);
    assert!(processes.is_ok());
    let mut processes = processes.unwrap_or_default();
    processes.sort_by(|left, right| left.name.cmp(&right.name));
    assert!(processes.iter().any(|process| {
        process.name == "reviewer"
            && process.parent.as_deref() == Some("coder")
            && process.status == "busy"
            && process.pid.as_deref() == Some(pid.as_str())
            && process.model == "main"
            && process.life == "owned"
    }));

    let root_process = processes
        .iter()
        .find(|process| process.name == "coder")
        .cloned();
    assert!(root_process.is_some());
    let Some(root_process) = root_process else {
        return;
    };
    let mut rendered = Vec::new();
    render_agent_process_tree(
        &root_process,
        &processes,
        "",
        true,
        true,
        &mut rendered,
    );
    assert_eq!(
        rendered,
        vec![
            "coder [idle]".to_owned(),
            format!("`- reviewer [busy] parent_session=default pid={pid}"),
            "   `- auditor [ready]".to_owned(),
        ]
    );
}

#[test]
fn agent_ps_marks_ready_agent_dead_when_recorded_pid_is_gone() {
    let root = clean_test_dir("ctx-agent-ps-stale-pid");
    create_agent_fixture(&root, "worker", "agent:coder", "ready", "999999999");

    assert_eq!(
        render_agent_status_lines(&read_agent_processes(&root).unwrap_or_default()),
        vec!["worker [dead] model=api.lmm.best/gpt-5.3-codex-spark role=worker".to_owned()]
    );
}

#[test]
fn agent_ps_defaults_worker_and_executor_prefixes_to_spark_model() {
    let root = clean_test_dir("ctx-agent-ps-worker-prefix-model");
    create_agent_fixture(&root, "coder", "", "idle", "");
    create_agent_fixture(&root, "worker-fast", "agent:coder", "ready", "");
    create_agent_fixture(&root, "executor-fast", "agent:coder", "ready", "");
    assert!(!root.join("agent/worker-fast.d/model").exists());
    assert!(!root.join("agent/executor-fast.d/model").exists());

    assert_eq!(
        render_agent_status_lines(&read_agent_processes(&root).unwrap_or_default()),
        vec![
            "coder [idle]".to_owned(),
            "+- executor-fast [ready] model=api.lmm.best/gpt-5.3-codex-spark role=worker".to_owned(),
            "`- worker-fast [ready] model=api.lmm.best/gpt-5.3-codex-spark role=worker".to_owned(),
        ]
    );
}

#[test]
fn agent_process_tree_reports_parent_cycles_as_visible_roots() {
    let mut processes = vec![
        AgentProcess {
            name: "coder".to_owned(),
            parent: Some("worker".to_owned()),
            parent_session: None,
            status: "busy".to_owned(),
            pid: Some("100".to_owned()),
            model: "main".to_owned(),
            life: "owned".to_owned(),
        },
        AgentProcess {
            name: "worker".to_owned(),
            parent: Some("coder".to_owned()),
            parent_session: None,
            status: "ready".to_owned(),
            pid: Some("101".to_owned()),
            model: "api.lmm.best/gpt-5.3-codex-spark".to_owned(),
            life: "owned".to_owned(),
        },
    ];
    processes.sort_by(|left, right| left.name.cmp(&right.name));

    assert_eq!(
        render_agent_status_lines(&processes),
        vec![
            "coder [busy] pid=100".to_owned(),
            "`- worker [ready] model=api.lmm.best/gpt-5.3-codex-spark role=worker pid=101".to_owned(),
        ]
    );
}

#[test]
fn agent_process_tree_escapes_control_file_values() {
    let process = AgentProcess {
        name: "coder".to_owned(),
        parent: None,
        parent_session: None,
        status: "idle\u{1b}]52;c;payload\u{7}".to_owned(),
        pid: Some("123\u{1b}[31m".to_owned()),
        model: "bad\u{1b}[31m".to_owned(),
        life: "temp\u{1b}[31m".to_owned(),
    };
    let mut rendered = Vec::new();

    render_agent_process_tree(
        &process,
        std::slice::from_ref(&process),
        "",
        true,
        true,
        &mut rendered,
    );

    assert_eq!(
        rendered,
        vec![
            "coder [idle\\u{1b}]52;c;payload\\u{7}] model=bad\\u{1b}[31m life=temp\\u{1b}[31m pid=123\\u{1b}[31m"
                .to_owned()
        ]
    );
    let line = rendered.first().map_or("", String::as_str);
    assert!(!line.as_bytes().contains(&0x1b));
    assert!(!line.as_bytes().contains(&0x07));
}

#[test]
fn agent_ps_shows_non_default_worker_model() {
    let root = clean_test_dir("ctx-agent-ps-worker-model");
    create_agent_fixture(&root, "coder", "", "idle", "");
    create_agent_fixture(&root, "worker", "agent:coder session:default", "ready", "");
    write_text_file(
        &root.join("agent/worker.d/model"),
        "api.lmm.best/gpt-5.3-codex-spark\n",
    );

    assert_eq!(
        render_agent_status_lines(&read_agent_processes(&root).unwrap_or_default()),
        vec![
            "coder [idle]".to_owned(),
            "`- worker [ready] model=api.lmm.best/gpt-5.3-codex-spark role=worker parent_session=default".to_owned(),
        ]
    );
}

#[test]
fn agent_ps_shows_non_owned_worker_lifecycle() {
    let root = clean_test_dir("ctx-agent-ps-worker-life");
    create_agent_fixture(&root, "coder", "", "idle", "");
    create_agent_fixture(&root, "worker", "agent:coder session:default", "ready", "");
    write_text_file(&root.join("agent/worker.d/life"), "temp\n");

    assert_eq!(
        render_agent_status_lines(&read_agent_processes(&root).unwrap_or_default()),
        vec![
            "coder [idle]".to_owned(),
            "`- worker [ready] model=api.lmm.best/gpt-5.3-codex-spark life=temp role=worker parent_session=default".to_owned(),
        ]
    );
}

#[test]
fn agent_status_reports_model_lifecycle_parent_pid_identity_and_paths() {
    let root = clean_test_dir("ctx-agent-status-process-fields");
    let pid = std::process::id().to_string();
    create_agent_fixture(&root, "task-a", "agent:worker session:default", "ready", "");
    create_agent_fixture(&root, "task-b", "agent:worker", "ready", "");
    create_agent_fixture(&root, "nested", "agent:task-a", "ready", "");
    assert!(fs::create_dir_all(root.join("agent/control-only.d")).is_ok());
    write_text_file(&root.join("agent/control-only.d/parent"), "agent:worker\n");
    create_agent_fixture(&root, "worker", "agent:coder session:default", "ready", &pid);
    write_text_file(
        &root.join("agent/worker.d/model"),
        "api.lmm.best/gpt-5.3-codex-spark\n",
    );
    write_text_file(&root.join("agent/worker.d/uid"), "1000\n");
    write_text_file(&root.join("agent/worker.d/gid"), "100\n");
    write_text_file(&root.join("agent/worker.d/groups"), "10\n20\n");
    write_text_file(&root.join("agent/worker.d/root"), "/ctx/home/1000/agent/worker/root\n");
    write_text_file(&root.join("agent/worker.d/cwd"), "/workspace\n");

    assert_eq!(
        agent_status_lines(&root, "worker"),
        Ok(vec![
            "ready".to_owned(),
            "model=api.lmm.best/gpt-5.3-codex-spark".to_owned(),
            "life=owned".to_owned(),
            "role=worker".to_owned(),
            "parent=agent:coder session:default".to_owned(),
            "children=3".to_owned(),
            format!("pid={pid}"),
            "uid=1000".to_owned(),
            "gid=100".to_owned(),
            "groups=10 20".to_owned(),
            "root=/ctx/home/1000/agent/worker/root".to_owned(),
            "cwd=/workspace".to_owned(),
        ])
    );
}

#[test]
fn agent_status_marks_ready_agent_dead_when_recorded_pid_is_gone() {
    let root = clean_test_dir("ctx-agent-status-stale-pid");
    create_agent_fixture(&root, "worker", "agent:coder", "ready", "999999999");

    assert_eq!(
        agent_status_lines(&root, "worker"),
        Ok(vec![
            "dead".to_owned(),
            "model=api.lmm.best/gpt-5.3-codex-spark".to_owned(),
            "life=owned".to_owned(),
            "role=worker".to_owned(),
            "parent=agent:coder".to_owned(),
            "children=0".to_owned(),
            "pid=-".to_owned(),
            "uid=-".to_owned(),
            "gid=-".to_owned(),
            "groups=-".to_owned(),
            "root=-".to_owned(),
            "cwd=-".to_owned(),
        ])
    );
}

#[test]
fn agent_status_child_count_skips_dead_and_stale_pid_children() {
    let root = clean_test_dir("ctx-agent-status-live-child-count");
    create_agent_fixture(&root, "coder", "agent:base", "ready", "");
    create_agent_fixture(&root, "worker-fast", "agent:coder", "ready", "");
    create_agent_fixture(&root, "dead", "agent:coder", "dead", "");
    create_agent_fixture(&root, "stale", "agent:coder", "busy", "999999999");

    assert_eq!(
        agent_status_lines(&root, "coder"),
        Ok(vec![
            "ready".to_owned(),
            "model=main".to_owned(),
            "life=owned".to_owned(),
            "role=agent".to_owned(),
            "parent=agent:base".to_owned(),
            "children=1".to_owned(),
            "pid=-".to_owned(),
            "uid=-".to_owned(),
            "gid=-".to_owned(),
            "groups=-".to_owned(),
            "root=-".to_owned(),
            "cwd=-".to_owned(),
        ])
    );
    assert_eq!(
        agent_status_lines(&root, "worker-fast")
            .map(|lines| lines.get(1).cloned()),
        Ok(Some(
            "model=api.lmm.best/gpt-5.3-codex-spark".to_owned()
        ))
    );
}

#[test]
fn agent_status_escapes_control_file_values() {
    let root = clean_test_dir("ctx-agent-status-escape-fields");
    create_agent_fixture(&root, "worker", "agent:coder\u{1b}[31m", "ready\u{1b}]52;c;x\u{7}", "");

    let lines = agent_status_lines(&root, "worker").unwrap_or_default();
    assert_eq!(
        lines,
        vec![
            "ready\\u{1b}]52;c;x\\u{7}".to_owned(),
            "model=api.lmm.best/gpt-5.3-codex-spark".to_owned(),
            "life=owned".to_owned(),
            "role=worker".to_owned(),
            "parent=agent:coder\\u{1b}[31m".to_owned(),
            "children=0".to_owned(),
            "pid=-".to_owned(),
            "uid=-".to_owned(),
            "gid=-".to_owned(),
            "groups=-".to_owned(),
            "root=-".to_owned(),
            "cwd=-".to_owned(),
        ]
    );
    assert!(!lines.join("\n").as_bytes().contains(&0x1b));
    assert!(!lines.join("\n").as_bytes().contains(&0x07));
}

#[test]
fn parses_agent_env_command() {
    assert!(matches!(
        cmd!("agent", "env", "worker"),
        Ok(Command::Agent(AgentArgs::Env { ref name })) if name == "worker"
    ));
}

#[test]
fn agent_env_reports_derived_sandbox_environment() {
    let root = clean_test_dir("ctx-agent-env-worker");
    create_complete_agent_control(&root, "worker");

    assert_eq!(
        agent_env_lines(&root, "worker"),
        Ok(vec![
            "CTX_ROOT=/ctx".to_owned(),
            "CTX_HOME=/ctx/home/1000".to_owned(),
            "CTX_AGENT=worker".to_owned(),
            "CTX_AGENT_ROLE=worker".to_owned(),
            "CTX_AGENT_MODEL=api.lmm.best/gpt-5.3-codex-spark".to_owned(),
            "CTX_AGENT_LIFE=owned".to_owned(),
            "CTX_AGENT_ROOT_PATH=/ctx/home/1000/agent/worker/root".to_owned(),
            "CTX_AGENT_CWD=/workspace".to_owned(),
            "CTX_AGENT_SUBJECT=worker_t".to_owned(),
            "CTX_AGENT_UID=1000".to_owned(),
            "CTX_AGENT_GID=100".to_owned(),
            "CTX_AGENT_GROUPS=10 20".to_owned(),
            "HOME=/home/agent".to_owned(),
            "USER=worker".to_owned(),
            "LOGNAME=worker".to_owned(),
            "SHELL=/usr/bin/bash".to_owned(),
            "TERM=xterm-256color".to_owned(),
            "LANG=C.UTF-8".to_owned(),
            "PATH=/usr/bin:/bin".to_owned(),
            "CTX_PATH=/ctx/tool:/ctx/home/1000/tool".to_owned(),
        ])
    );
}

#[test]
fn status_helpers_report_ctx_and_agent_tree() {
    let root = clean_test_dir("ctx-status-tree");
    let pid = std::process::id().to_string();
    write_text_file(&root.join("status"), "ready\n");
    create_agent_fixture(&root, "coder", "", "idle", "");
    create_agent_fixture(&root, "reviewer", "agent:coder session:default run:r1", "busy", &pid);

    assert_eq!(ctx_state(true, true, true), "running");
    assert_eq!(ctx_state(true, true, false), "available");
    assert_eq!(read_ctx_status(&root), "ready");

    let processes = read_status_agent_processes(&root);
    assert!(processes.is_ok());
    let rendered = render_agent_status_lines(&processes.unwrap_or_default());
    assert_eq!(
        rendered,
        vec![
            "coder [idle]".to_owned(),
            format!("`- reviewer [busy] parent_session=default pid={pid}"),
        ]
    );
}

#[test]
fn ctx_root_shape_does_not_follow_symlink_root() {
    let root = clean_test_dir("ctx-status-root-shape");
    let outside = clean_test_dir("ctx-status-root-shape-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    let link = root.join("ctx");
    assert!(std::os::unix::fs::symlink(&outside, &link).is_ok());

    assert_eq!(ctx_root_shape(&outside), (true, true));
    assert_eq!(ctx_root_shape(&link), (true, false));
    assert_eq!(ctx_state(true, false, false), "invalid");
}

#[test]
fn ctx_root_entry_present_does_not_follow_symlink_entry() {
    let root = clean_test_dir("ctx-status-root-entry-shape");
    let outside = clean_test_dir("ctx-status-root-entry-shape-outside");
    assert!(fs::create_dir_all(root.join("status")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("bin")).is_ok());

    assert!(ctx_root_entry_present(&root, "status"));
    assert!(!ctx_root_entry_present(&root, "bin"));
}

#[test]
fn status_tolerates_missing_agent_directory() {
    let root = clean_test_dir("ctx-status-no-agent");
    write_text_file(&root.join("status"), "ready\n");

    let processes = read_status_agent_processes(&root);
    assert_eq!(processes, Ok(Vec::new()));
}
