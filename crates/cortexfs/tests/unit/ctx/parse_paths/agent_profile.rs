#[test]
fn parse_cortexfs_agent_profile_v1() {
    let text = format!(
        "\
schema: {AGENT_PROFILE_SCHEMA_V1}
name: reviewer
description: review helper
instructions: |
  Be careful.
model: openai/gpt-4o
tools:
  - fs.read
parent: agent:architect
label: reviewer_t
temporary: true
mounts:
  - source: /work
    target: /work
    mode: ro
"
    );
    let profile = parse_agent_profile_text(&text);
    assert!(profile.is_ok());
    let Ok(profile) = profile else {
        return;
    };
    assert_eq!(profile.name.as_deref(), Some("reviewer"));
    assert_eq!(profile.description.as_deref(), Some("review helper"));
    assert_eq!(profile.instructions.as_deref(), Some("Be careful."));
    assert_eq!(profile.models, vec!["openai/gpt-4o".to_owned()]);
    assert_eq!(profile.tools, vec!["fs.read".to_owned()]);
    assert_eq!(profile.parent.as_deref(), Some("agent:architect"));
    assert_eq!(profile.label.as_deref(), Some("reviewer_t"));
    assert!(profile.temporary);
    assert_eq!(profile.mounts.len(), 1);
    let Some(mount) = profile.mounts.first() else {
        return;
    };
    assert_eq!(mount.source, "/work");
    assert_eq!(mount.mode, "ro");
}

#[test]
fn parse_microsoft_agentschema_subset() {
    let text = "\
name: analyst
description: financial analyst
instructions: You analyze markets.
model: openai/gpt-4o
";
    let profile = parse_agent_profile_text(text);
    assert!(profile.is_ok());
    let Ok(profile) = profile else {
        return;
    };
    assert_eq!(profile.name.as_deref(), Some("analyst"));
    assert_eq!(profile.models, vec!["openai/gpt-4o".to_owned()]);
    assert_eq!(
        profile.instructions.as_deref(),
        Some("You analyze markets.")
    );
}

#[test]
fn parse_microsoft_manifest_template_nested() {
    let text = "\
name: sample-manifest
template:
  name: nested-agent
  instructions: Nested instructions.
  model: openai/gpt-4o
";
    let profile = parse_agent_profile_text(text);
    assert!(profile.is_ok());
    let Ok(profile) = profile else {
        return;
    };
    assert_eq!(profile.name.as_deref(), Some("sample-manifest"));
    assert_eq!(
        profile.instructions.as_deref(),
        Some("Nested instructions.")
    );
    assert_eq!(profile.models, vec!["openai/gpt-4o".to_owned()]);
}

#[test]
fn parse_rejects_hosted_container_profile() {
    let text = "\
name: boxed
kind: hosted
image: example.azurecr.io/agent:1
model: openai/gpt-4o
";
    let error = parse_agent_profile_text(text);
    assert!(error.is_err());
    let Err(error) = error else {
        return;
    };
    assert_eq!(error.code, 2);
    assert!(error.message.contains("hosted/container"));
}

#[test]
fn agent_new_from_profile_materializes_controls() {
    let root = clean_test_dir("ctx-agent-new-from-profile");
    assert!(fs::create_dir_all(&root).is_ok());
    let profile_path = root.join("reviewer.yaml");
    assert!(fs::write(
        &profile_path,
        "\
schema: cortexfs.agent.profile/v1
name: reviewer
description: code review agent
instructions: Review diffs carefully.
model: openai/gpt-4o
tools:
  - fs.read
parent: agent:architect
",
    )
    .is_ok());

    let command = parse_command(vec![
        "agent".to_owned(),
        "new".to_owned(),
        "--from".to_owned(),
        profile_path.to_string_lossy().into_owned(),
    ]);
    assert!(matches!(command, Ok(Command::Agent(AgentArgs::New(_)))));
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };

    assert_eq!(agent_new(&root, &args), Ok(ExitCode::SUCCESS));
    assert_eq!(
        fs::read_to_string(root.join("agent/reviewer.d/model")).unwrap_or_default(),
        "openai/gpt-4o\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/reviewer.d/system.md")).unwrap_or_default(),
        "Review diffs carefully.\n"
    );
    let meta = fs::read_to_string(root.join("agent/reviewer.d/meta.json")).unwrap_or_default();
    assert!(meta.contains("code review agent"));
    assert!(meta.contains("\"source\":\"profile\"") || meta.contains("\"source\": \"profile\""));
    let policy = fs::read_to_string(root.join("agent/reviewer.d/policy")).unwrap_or_default();
    assert!(policy.contains("model:openai/gpt-4o"));
    assert!(policy.contains("tool:fs.read"));
}

#[test]
fn agent_new_from_profile_cli_overrides_model() {
    let root = clean_test_dir("ctx-agent-new-from-profile-override");
    assert!(fs::create_dir_all(&root).is_ok());
    let profile_path = root.join("coder.yaml");
    assert!(fs::write(
        &profile_path,
        "\
name: coder
model: openai/gpt-4o
instructions: from profile
",
    )
    .is_ok());

    let command = parse_command(vec![
        "agent".to_owned(),
        "new".to_owned(),
        "coder".to_owned(),
        "--from".to_owned(),
        profile_path.to_string_lossy().into_owned(),
        "--model".to_owned(),
        "openai/gpt-4o-mini".to_owned(),
    ]);
    assert!(matches!(command, Ok(Command::Agent(AgentArgs::New(_)))));
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };
    assert_eq!(args.models, vec!["openai/gpt-4o-mini".to_owned()]);
    assert_eq!(args.instructions.as_deref(), Some("from profile"));
    assert_eq!(agent_new(&root, &args), Ok(ExitCode::SUCCESS));
    assert_eq!(
        fs::read_to_string(root.join("agent/coder.d/model")).unwrap_or_default(),
        "openai/gpt-4o-mini\n"
    );
}

#[test]
fn agent_apply_updates_existing_controls() {
    let root = clean_test_dir("ctx-agent-apply-profile");
    assert!(fs::create_dir_all(&root).is_ok());
    let create = cmd!(
        "agent",
        "new",
        "coder",
        "--parent",
        "agent:architect",
        "--model",
        "openai/gpt-4o"
    );
    assert!(matches!(create, Ok(Command::Agent(AgentArgs::New(_)))));
    let Ok(Command::Agent(AgentArgs::New(args))) = create else {
        return;
    };
    assert_eq!(agent_new(&root, &args), Ok(ExitCode::SUCCESS));

    let profile_path = root.join("update.yaml");
    assert!(fs::write(
        &profile_path,
        "\
name: coder
description: updated
instructions: New persona.
model: openai/gpt-4o
tools:
  - fs.read
",
    )
    .is_ok());

    assert_eq!(
        agent_apply(&root, "coder", &profile_path),
        Ok(ExitCode::SUCCESS)
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/coder.d/system.md")).unwrap_or_default(),
        "New persona.\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("agent/coder.d/model")).unwrap_or_default(),
        "openai/gpt-4o\n"
    );
    let policy = fs::read_to_string(root.join("agent/coder.d/policy")).unwrap_or_default();
    assert!(policy.contains("model:openai/gpt-4o"));
    assert!(policy.contains("tool:fs.read"));
}

#[test]
fn agent_apply_preserves_controls_omitted_from_profile() {
    let root = clean_test_dir("ctx-agent-apply-profile-partial");
    assert!(fs::create_dir_all(&root).is_ok());
    let create = cmd!(
        "agent",
        "new",
        "coder",
        "--model",
        "openai/gpt-4o"
    );
    assert!(matches!(create, Ok(Command::Agent(AgentArgs::New(_)))));
    let Ok(Command::Agent(AgentArgs::New(args))) = create else {
        return;
    };
    assert_eq!(agent_new(&root, &args), Ok(ExitCode::SUCCESS));

    let control = root.join("agent/coder.d");
    assert!(fs::write(control.join("meta.json"), "{\"description\":\"original\"}\n").is_ok());
    let original_model = fs::read_to_string(control.join("model")).unwrap_or_default();
    let original_meta = fs::read_to_string(control.join("meta.json")).unwrap_or_default();
    let original_policy = fs::read_to_string(control.join("policy")).unwrap_or_default();
    let profile_path = root.join("partial.yaml");
    assert!(fs::write(&profile_path, "instructions: Updated persona.\n").is_ok());

    assert_eq!(
        agent_apply(&root, "coder", &profile_path),
        Ok(ExitCode::SUCCESS)
    );
    assert_eq!(
        (
            fs::read_to_string(control.join("system.md")).unwrap_or_default(),
            fs::read_to_string(control.join("model")).unwrap_or_default(),
            fs::read_to_string(control.join("meta.json")).unwrap_or_default(),
            fs::read_to_string(control.join("policy")).unwrap_or_default(),
        ),
        (
            "Updated persona.\n".to_owned(),
            original_model,
            original_meta,
            original_policy,
        )
    );
}

#[test]
fn agent_apply_rejects_symlinked_control_directory_without_external_write() {
    let root = clean_test_dir("ctx-agent-apply-profile-symlink-control");
    let external = clean_test_dir("ctx-agent-apply-profile-external");
    assert!(fs::create_dir_all(root.join("agent")).is_ok());
    assert!(fs::create_dir_all(&external).is_ok());
    assert!(fs::write(external.join("system.md"), "external\n").is_ok());
    assert!(symlink(&external, root.join("agent/coder.d")).is_ok());
    let profile_path = root.join("update.yaml");
    assert!(fs::write(&profile_path, "instructions: changed\n").is_ok());

    assert!(agent_apply(&root, "coder", &profile_path).is_err());
    assert_eq!(
        fs::read_to_string(external.join("system.md")).unwrap_or_default(),
        "external\n"
    );
}

#[test]
fn agent_apply_invalid_model_does_not_change_controls() {
    let root = clean_test_dir("ctx-agent-apply-profile-invalid-model");
    assert!(fs::create_dir_all(&root).is_ok());
    let create = cmd!("agent", "new", "coder", "--model", "openai/gpt-4o");
    let Ok(Command::Agent(AgentArgs::New(args))) = create else {
        return;
    };
    assert_eq!(agent_new(&root, &args), Ok(ExitCode::SUCCESS));
    let control = root.join("agent/coder.d");
    let before = agent_apply_control_snapshot(&control);
    let profile_path = root.join("invalid-model.yaml");
    assert!(fs::write(
        &profile_path,
        "instructions: changed\ndescription: changed\nmodel: invalid model\n"
    )
    .is_ok());

    assert!(agent_apply(&root, "coder", &profile_path).is_err());
    assert_eq!(agent_apply_control_snapshot(&control), before);
}

#[test]
fn agent_apply_invalid_mount_does_not_change_controls() {
    let root = clean_test_dir("ctx-agent-apply-profile-invalid-mount");
    assert!(fs::create_dir_all(&root).is_ok());
    let create = cmd!("agent", "new", "coder", "--model", "openai/gpt-4o");
    let Ok(Command::Agent(AgentArgs::New(args))) = create else {
        return;
    };
    assert_eq!(agent_new(&root, &args), Ok(ExitCode::SUCCESS));
    let control = root.join("agent/coder.d");
    let before = agent_apply_control_snapshot(&control);
    let profile_path = root.join("invalid-mount.yaml");
    assert!(fs::write(
        &profile_path,
        "instructions: changed\nmounts:\n  - source: relative\n    target: /work\n    mode: rw\n"
    )
    .is_ok());

    assert!(agent_apply(&root, "coder", &profile_path).is_err());
    assert_eq!(agent_apply_control_snapshot(&control), before);
}

#[test]
fn agent_apply_merges_meta_and_preserves_unknown_keys() {
    let root = clean_test_dir("ctx-agent-apply-profile-meta-merge");
    assert!(fs::create_dir_all(&root).is_ok());
    let create = cmd!("agent", "new", "coder", "--model", "openai/gpt-4o");
    let Ok(Command::Agent(AgentArgs::New(args))) = create else {
        return;
    };
    assert_eq!(agent_new(&root, &args), Ok(ExitCode::SUCCESS));
    let meta_path = root.join("agent/coder.d/meta.json");
    assert!(fs::write(
        &meta_path,
        r#"{"description":"old","source":"manual","unknown":{"keep":true}}"#
    )
    .is_ok());
    assert!(fs::set_permissions(&meta_path, fs::Permissions::from_mode(0o644)).is_ok());
    let before_meta = fs::symlink_metadata(&meta_path)
        .map(|metadata| (metadata.ino(), metadata.uid(), metadata.gid()))
        .ok();
    let profile_path = root.join("description.yaml");
    assert!(fs::write(&profile_path, "description: updated\n").is_ok());

    assert_eq!(
        agent_apply(&root, "coder", &profile_path),
        Ok(ExitCode::SUCCESS)
    );
    let meta: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&meta_path).unwrap_or_default())
            .unwrap_or_default();
    assert_eq!(meta.get("description").and_then(serde_json::Value::as_str), Some("updated"));
    assert_eq!(meta.get("source").and_then(serde_json::Value::as_str), Some("profile"));
    assert_eq!(
        meta.get("unknown")
            .and_then(|value| value.get("keep"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert!(matches!(
        (before_meta, fs::symlink_metadata(meta_path)),
        (Some((inode, uid, gid)), Ok(metadata))
            if metadata.ino() != inode
                && metadata.permissions().mode() & 0o7777 == 0o644
                && metadata.uid() == uid
                && metadata.gid() == gid
    ));
}

#[test]
fn agent_apply_invalid_meta_does_not_change_controls() {
    let root = clean_test_dir("ctx-agent-apply-profile-invalid-meta");
    assert!(fs::create_dir_all(&root).is_ok());
    let create = cmd!("agent", "new", "coder", "--model", "openai/gpt-4o");
    let Ok(Command::Agent(AgentArgs::New(args))) = create else {
        return;
    };
    assert_eq!(agent_new(&root, &args), Ok(ExitCode::SUCCESS));
    let control = root.join("agent/coder.d");
    assert!(fs::write(control.join("meta.json"), "not-json\n").is_ok());
    let before = agent_apply_control_snapshot(&control);
    let profile_path = root.join("invalid-meta.yaml");
    assert!(fs::write(
        &profile_path,
        "instructions: changed\ndescription: changed\n"
    )
    .is_ok());

    assert!(agent_apply(&root, "coder", &profile_path).is_err());
    assert_eq!(agent_apply_control_snapshot(&control), before);
}

fn agent_apply_control_snapshot(control: &Path) -> Vec<(String, String)> {
    ["system.md", "meta.json", "model", "policy", "mount"]
        .iter()
        .map(|file| {
            (
                (*file).to_owned(),
                fs::read_to_string(control.join(file)).unwrap_or_default(),
            )
        })
        .collect()
}

#[test]
fn agent_new_args_from_profile_requires_name() {
    let profile = AgentProfile::default();
    let args = AgentNewArgs {
        name: String::new(),
        temporary: false,
        parent: None,
        label: None,
        models: Vec::new(),
        tools: Vec::new(),
        shared: Vec::new(),
        mounts: Vec::new(),
        instructions: None,
        description: None,
    };
    let error = agent_new_args_from_profile(profile, args);
    assert!(error.is_err());
    let Err(error) = error else {
        return;
    };
    assert_eq!(error.code, 2);
    assert!(error.message.contains("missing name"));
}

#[test]
fn load_agent_profile_missing_file() {
    let error = load_agent_profile(Path::new("/no/such/agent-profile.yaml"));
    assert!(error.is_err());
    let Err(error) = error else {
        return;
    };
    assert_eq!(error.code, 2);
    assert!(error.message.contains("cannot read agent profile"));
}

#[test]
fn resolve_agent_profile_prefers_agent_yaml_in_directory() {
    let root = clean_test_dir("ctx-agent-yaml-in-dir");
    let dir = root.join("reviewer");
    assert!(fs::create_dir_all(&dir).is_ok());
    let profile = dir.join("agent.yaml");
    assert!(fs::write(
        &profile,
        "\
name: reviewer
model: openai/gpt-4o
instructions: from agent.yaml
",
    )
    .is_ok());

    let resolved = resolve_agent_profile_path(&dir);
    assert!(resolved.is_ok());
    let Ok(resolved) = resolved else {
        return;
    };
    assert_eq!(resolved, profile);

    let loaded = load_agent_profile(&dir);
    assert!(loaded.is_ok());
    let Ok(loaded) = loaded else {
        return;
    };
    assert_eq!(loaded.name.as_deref(), Some("reviewer"));
    assert_eq!(loaded.instructions.as_deref(), Some("from agent.yaml"));
}

#[test]
fn resolve_agent_profile_accepts_explicit_agent_yaml_file() {
    let root = clean_test_dir("ctx-agent-yaml-file");
    assert!(fs::create_dir_all(&root).is_ok());
    let profile = root.join("agent.yaml");
    assert!(fs::write(
        &profile,
        "\
name: solo
model: openai/gpt-4o
",
    )
    .is_ok());

    let resolved = resolve_agent_profile_path(&profile);
    assert!(resolved.is_ok());
    let Ok(resolved) = resolved else {
        return;
    };
    assert_eq!(resolved, profile);
}

#[test]
fn agent_new_from_directory_agent_yaml() {
    let root = clean_test_dir("ctx-agent-new-from-agent-yaml-dir");
    let dir = root.join("pack");
    assert!(fs::create_dir_all(&dir).is_ok());
    assert!(fs::write(
        dir.join("agent.yaml"),
        "\
schema: cortexfs.agent.profile/v1
name: packbot
description: packed agent
instructions: Hello from agent.yaml
model: openai/gpt-4o
parent: agent:architect
",
    )
    .is_ok());

    let command = parse_command(vec![
        "agent".to_owned(),
        "new".to_owned(),
        "--from".to_owned(),
        dir.to_string_lossy().into_owned(),
    ]);
    assert!(matches!(command, Ok(Command::Agent(AgentArgs::New(_)))));
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };
    assert_eq!(args.name, "packbot");
    assert_eq!(agent_new(&root, &args), Ok(ExitCode::SUCCESS));
    assert_eq!(
        fs::read_to_string(root.join("agent/packbot.d/system.md")).unwrap_or_default(),
        "Hello from agent.yaml\n"
    );
}

#[test]
fn resolve_agent_profile_directory_without_agent_yaml_errors() {
    let root = clean_test_dir("ctx-agent-yaml-missing-in-dir");
    assert!(fs::create_dir_all(&root).is_ok());
    let error = resolve_agent_profile_path(&root);
    assert!(error.is_err());
    let Err(error) = error else {
        return;
    };
    assert_eq!(error.code, 2);
    assert!(error.message.contains("no agent.yaml"));
}
