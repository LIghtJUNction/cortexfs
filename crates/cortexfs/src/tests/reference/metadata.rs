#[test]
fn reference_tree_bootstrap_ignores_symlink_session_meta_during_migration() {
    let root = clean_test_dir("reference-tree-session-meta-symlink");
    let outside = clean_test_dir("reference-tree-session-meta-symlink-outside");
    let session = agent_session_root(&root, "coder").join("default");
    assert!(fs::create_dir_all(&session).is_ok());
    write_text_file(&outside.join("meta.json"), "{\"model\":\"legacy\"}\n");
    assert!(symlink(outside.join("meta.json"), session.join("meta.json")).is_ok());

    assert!(ensure_reference_tree(&root).is_ok());

    assert!(
        session
            .join("meta.json")
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
    );
    assert_file_text(&outside.join("meta.json"), "{\"model\":\"legacy\"}\n");
}

#[test]
fn reference_tree_bootstrap_ignores_symlink_session_dir_during_migration() {
    let root = clean_test_dir("reference-tree-session-dir-symlink");
    let outside = clean_test_dir("reference-tree-session-dir-symlink-outside");
    let agent = root
        .join("home")
        .join("1000")
        .join("agent")
        .join("attacker");
    assert!(fs::create_dir_all(&agent).is_ok());
    write_text_file(
        &outside.join("default").join("meta.json"),
        "{\"model\":\"legacy\"}\n",
    );
    assert!(symlink(&outside, agent.join("session")).is_ok());

    assert!(ensure_reference_tree(&root).is_ok());

    assert_file_text(
        &outside.join("default").join("meta.json"),
        "{\"model\":\"legacy\"}\n",
    );
}

#[test]
fn reference_tree_bootstrap_preserves_valid_provider_model_alias() {
    let root = clean_test_dir("reference-tree-valid-model-alias");
    let user_model = ctx_home(&root).join("model");
    assert!(fs::create_dir_all(&user_model).is_ok());
    assert!(symlink("/ctx/model/openai/gpt-5.6", user_model.join("coder")).is_ok());

    assert!(ensure_reference_tree(&root).is_ok());

    let model_link = fs::read_link(user_model.join("coder"));
    assert!(
        matches!(model_link, Ok(ref target) if target == Path::new("/ctx/model/openai/gpt-5.6"))
    );
}

#[test]
fn reference_tree_bootstrap_preserves_existing_canonical_model_aliases() {
    let root = clean_test_dir("reference-tree-canonical-model-alias");
    assert!(fs::create_dir_all(root.join("model")).is_ok());
    assert!(symlink("/ctx/model/local/custom", root.join("model/code")).is_ok());

    assert!(ensure_reference_tree(&root).is_ok());

    assert!(matches!(
        fs::read_link(root.join("model/code")),
        Ok(ref target) if target == Path::new("/ctx/model/local/custom")
    ));
}

#[test]
fn reference_tree_bootstrap_rejects_symlink_model_alias_parent_without_writing_target() {
    let root = clean_test_dir("reference-tree-model-alias-parent-symlink");
    let outside = clean_test_dir("reference-tree-model-alias-parent-symlink-outside");
    let user = root.join("home").join("1000");
    assert!(fs::create_dir_all(&user).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(symlink(&outside, user.join("model")).is_ok());

    assert!(ensure_reference_tree(&root).is_err());
    assert!(!outside.join("coder").exists());
}

#[test]
fn model_exec_metadata_exposes_driver_route_table() {
    let root = clean_test_dir("model-driver-metadata");
    let control = root.join("model").join("openai").join("gpt-5.6.d");

    write_text_file(&control.join("id"), "openai/gpt-5.6\n");
    write_text_file(
        &control.join("driver"),
        "default=openai-chat\nexec=openai-chat\nagent=openai-responses,openai-chat\n",
    );
    write_text_file(&control.join("cap"), "chat\nstream\ntool_call_syntax\n");
    write_text_file(&control.join("limit"), "32768\n");
    write_text_file(&control.join("session"), "socket\n");
    write_text_file(&control.join("status"), "idle\n");

    let metadata = model_exec_metadata("openai/gpt-5.6", &control);
    let metadata = ok!(metadata);
    assert!(metadata.contains("# cortexfs.driver=openai-chat\n"));
    assert!(metadata.contains("# cortexfs.driver.default=openai-chat\n"));
    assert!(metadata.contains("# cortexfs.driver.exec=openai-chat\n"));
    assert!(metadata.contains("# cortexfs.driver.socket=\n"));
    assert!(metadata.contains("# cortexfs.driver.agent=openai-responses,openai-chat\n"));
    assert!(metadata.contains("# cortexfs.context_length=32768\n"));
}

#[test]
fn model_exec_metadata_rejects_extra_limit_line() {
    let root = clean_test_dir("model-limit-metadata-extra-line");
    let control = root.join("model").join("openai").join("gpt-5.6.d");

    write_text_file(&control.join("id"), "openai/gpt-5.6\n");
    write_text_file(&control.join("driver"), "default=openai-chat\n");
    write_text_file(&control.join("cap"), "chat\n");
    write_text_file(&control.join("limit"), "32768\n\n");
    write_text_file(&control.join("session"), "none\n");
    write_text_file(&control.join("status"), "idle\n");

    assert_eq!(
        model_exec_metadata("openai/gpt-5.6", &control),
        Err(FuseError::InvalidContent)
    );
}

#[test]
fn model_exec_metadata_refuses_symlink_control_files() {
    let root = clean_test_dir("model-driver-metadata-symlink");
    let control = root.join("model").join("openai").join("gpt-5.6.d");
    let outside = root.join("outside-driver");

    write_text_file(&control.join("id"), "openai/gpt-5.6\n");
    write_text_file(&outside, "default=openai-chat\n");
    assert!(symlink(&outside, control.join("driver")).is_ok());
    write_text_file(&control.join("cap"), "chat\n");
    write_text_file(&control.join("session"), "socket\n");
    write_text_file(&control.join("status"), "idle\n");

    assert_eq!(
        model_exec_metadata("openai/gpt-5.6", &control),
        Err(FuseError::InvalidContent)
    );
}

#[test]
fn model_exec_metadata_refuses_symlink_control_directory() {
    let root = clean_test_dir("model-driver-metadata-symlink-dir");
    let outside = clean_test_dir("model-driver-metadata-symlink-dir-outside");
    let control = root.join("model").join("openai").join("gpt-5.6.d");
    let outside_control = outside.join("gpt-5.6.d");

    write_text_file(&outside_control.join("id"), "openai/gpt-5.6\n");
    write_text_file(&outside_control.join("driver"), "default=openai-chat\n");
    write_text_file(&outside_control.join("cap"), "chat\n");
    write_text_file(&outside_control.join("session"), "socket\n");
    write_text_file(&outside_control.join("status"), "idle\n");
    assert!(fs::create_dir_all(root.join("model").join("openai")).is_ok());
    assert!(symlink(&outside_control, &control).is_ok());

    assert_eq!(
        model_exec_metadata("openai/gpt-5.6", &control),
        Err(FuseError::Io)
    );
}

#[test]
fn model_exec_metadata_refuses_oversized_control_files() {
    let root = clean_test_dir("model-driver-metadata-oversized");
    let control = root.join("model").join("openai").join("gpt-5.6.d");

    write_text_file(&control.join("id"), "openai/gpt-5.6\n");
    write_text_file(&control.join("driver"), &"x".repeat((64 * 1024) + 1));
    write_text_file(&control.join("cap"), "chat\n");
    write_text_file(&control.join("session"), "socket\n");
    write_text_file(&control.join("status"), "idle\n");

    assert_eq!(
        model_exec_metadata("openai/gpt-5.6", &control),
        Err(FuseError::InvalidContent)
    );
}

#[test]
fn echo_model_runner_emits_one_shot_jsonl() {
    let mut stdout = Vec::new();
    let result = run_echo_model(["fix tests"], &mut stdout);
    assert!(result.is_ok());
    let stdout = String::from_utf8(stdout);
    let stdout = ok!(stdout);
    assert!(stdout.contains(r#"{"type":"start","run":"r1","model":"debug/echo"}"#));
    assert!(stdout.contains(r#"{"type":"delta","run":"r1","text":"fix tests"}"#));
    assert!(stdout.contains(r#"{"type":"done","run":"r1","status":"ok"}"#));
    assert!(inspect_event_stream_jsonl(&stdout).is_ok());
}

#[test]
fn echo_model_stdin_reader_accepts_input_at_limit() {
    let input = "x".repeat(MAX_ECHO_MODEL_STDIN_BYTES);

    let read = read_echo_model_stdin_limited(
        std::io::Cursor::new(input.as_bytes()),
        MAX_ECHO_MODEL_STDIN_BYTES,
    );

    assert_eq!(read.unwrap_or_default().len(), MAX_ECHO_MODEL_STDIN_BYTES);
}

#[test]
fn echo_model_stdin_reader_rejects_input_over_limit() {
    let input = "x".repeat(MAX_ECHO_MODEL_STDIN_BYTES + 1);

    let read = read_echo_model_stdin_limited(
        std::io::Cursor::new(input.as_bytes()),
        MAX_ECHO_MODEL_STDIN_BYTES,
    );

    assert!(matches!(read, Err(ref error) if error.kind() == std::io::ErrorKind::InvalidData));
}

#[test]
fn reference_tree_bootstrap_installs_tsh_tools() {
    let root = reference_tree("reference-tree-tool-exec");

    assert!(root.join("tool").join("tsh").is_file());
    assert!(root.join("tool").join("tsh.d").is_dir());
    assert!(root.join("tool").join("tsh.d").join("config").is_file());
    assert!(root.join("tool").join("tsh.config").is_file());
    assert!(root.join("tool").join("tsh.config.d").is_dir());
    for tool in ["fs.read", "fs.write", "shell.exec", "agent.create"] {
        assert!(root.join("tool").join(tool).is_file());
        assert!(root.join("tool").join(format!("{tool}.d")).is_dir());
        assert!(root.join("tool").join(format!("{tool}.d/schema")).is_file());
        assert!(root.join("tool").join(format!("{tool}.d/policy")).is_file());
    }
    for tool in ["bash", "tmux", "zellij"] {
        assert!(!root.join("tool").join(tool).exists());
        assert!(!root.join("tool").join(format!("{tool}.d")).exists());
    }
}

#[test]
fn root_bootstrap_chowns_reference_home_symlinks_without_following_targets() {
    if fs::metadata("/proc/self").map_or(1, |metadata| metadata.uid()) != 0 {
        return;
    }

    let root = clean_test_dir("reference-tree-home-symlink-ownership");
    let target_dir = clean_test_dir("reference-tree-home-symlink-target");
    let target = target_dir.join("root-owned-target");
    assert!(fs::create_dir_all(&target_dir).is_ok());
    assert!(fs::write(&target, "keep root owner\n").is_ok());
    assert!(ensure_reference_tree(&root).is_ok());

    let link = root
        .join("home")
        .join("1000")
        .join("agent")
        .join("coder")
        .join("session")
        .join("pwn");
    assert!(symlink(&target, &link).is_ok());

    let target_before = ok!(fs::symlink_metadata(&target));
    assert_eq!(target_before.uid(), 0);
    assert_eq!(target_before.gid(), 0);

    assert!(ensure_reference_tree(&root).is_ok());

    let link_metadata = ok!(fs::symlink_metadata(&link));
    assert!(link_metadata.file_type().is_symlink());
    assert_eq!(link_metadata.uid(), 1000);
    assert_eq!(link_metadata.gid(), 1000);

    let target_after = ok!(fs::symlink_metadata(&target));
    assert_eq!(target_after.uid(), 0);
    assert_eq!(target_after.gid(), 0);
}

#[test]
fn root_bootstrap_assigns_reference_home_to_agent_identity() {
    if fs::metadata("/proc/self").map_or(1, |metadata| metadata.uid()) != 0 {
        return;
    }

    let root = clean_test_dir("reference-tree-home-ownership");
    assert!(ensure_reference_tree(&root).is_ok());

    for path in [
        root.join("home").join("1000"),
        root.join("home").join("1000").join("agent").join("coder"),
        root.join("home")
            .join("1000")
            .join("agent")
            .join("coder")
            .join("session"),
        root.join("home")
            .join("1000")
            .join("agent")
            .join("coder")
            .join("session")
            .join("index"),
    ] {
        let metadata = ok!(fs::symlink_metadata(path));
        assert_eq!(metadata.uid(), 1000);
        assert_eq!(metadata.gid(), 1000);
    }
}
use super::*;
