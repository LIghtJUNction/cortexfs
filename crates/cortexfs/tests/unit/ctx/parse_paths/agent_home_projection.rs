#[test]
fn agent_mount_allows_readonly_subpaths_in_sandbox_home() {
    for target in [
        "/home/agent/.codex/config.toml",
        "/home/agent/.claude/settings.json",
        "/home/agent/.pi/agent/settings.json",
    ] {
        let mount = AgentMount {
            source: "/tmp/source".to_owned(),
            target: target.to_owned(),
            mode: "ro".to_owned(),
        };
        assert!(
            require_agent_mount(&mount).is_ok(),
            "readonly home projection should be accepted: {target}"
        );
    }
}

#[test]
fn agent_mount_keeps_home_root_and_writable_subpaths_protected() {
    for (target, mode) in [
        ("/home/agent", "ro"),
        ("/home/other/.config", "ro"),
        ("/home/agent/../other/.config", "ro"),
        ("/home/agent/.codex/config.toml", "rw"),
    ] {
        let mount = AgentMount {
            source: "/tmp/source".to_owned(),
            target: target.to_owned(),
            mode: mode.to_owned(),
        };
        assert!(
            require_agent_mount(&mount).is_err(),
            "protected home projection should be rejected: {target} {mode}"
        );
    }
}

#[test]
fn agent_home_projection_still_requires_exact_mount_policy() {
    let root = clean_test_dir("ctx-agent-home-config-policy");
    assert!(ensure_reference_tree(&root).is_ok());
    ensure_runtime_model_fixture(&root);
    let source = root.join("host-codex-config.toml");
    write_text_file(&source, "model = \"example\"\n");
    let mount = AgentMount {
        source: source.display().to_string(),
        target: "/home/agent/.codex/config.toml".to_owned(),
        mode: "ro".to_owned(),
    };

    let Ok(view) = derive_agent_runtime_view(&root, "executor") else {
        return;
    };
    assert!(validate_agent_start_mounts(&view, std::slice::from_ref(&mount)).is_err());

    let declared = format!(
        "/ctx\t/ctx\tro\trbind,nosuid,nodev\n{}\t/home/agent/.codex/config.toml\tro\tbind,nosuid,nodev,noexec\n",
        source.display()
    );
    write_text_file(&root.join("agent/executor.d/mount"), &declared);
    let Ok(view) = derive_agent_runtime_view(&root, "executor") else {
        return;
    };
    assert_eq!(
        validate_agent_start_mounts(&view, std::slice::from_ref(&mount)),
        Ok(())
    );
}
