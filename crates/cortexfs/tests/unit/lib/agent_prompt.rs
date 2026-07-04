#[test]
fn agent_prompt_rules_collect_existing_files_once_in_order() {
    let root = clean_test_dir("agent-prompt-rules");
    let first = root.join("AGENTS.md");
    let second = root.join("nested").join("AGENTS.md");
    assert!(fs::create_dir_all(second.parent().unwrap_or(&root)).is_ok());
    write_text_file(&first, "root rule\n");
    write_text_file(&second, "nested rule\n");

    let rules = collect_agent_rules_from_paths([first.clone(), second, first]);

    assert!(rules.contains("root rule"));
    assert!(rules.contains("nested rule"));
    assert_eq!(rules.matches("root rule").count(), 1);
    assert!(rules.find("root rule") < rules.find("nested rule"));
}

#[test]
fn agent_prompt_rules_refuse_symlinked_files() {
    let root = clean_test_dir("agent-prompt-rules-symlink");
    let outside = root.join("outside").join("AGENTS.md");
    write_text_file(&outside, "outside rule\n");
    let link = root.join("AGENTS.md");
    assert!(symlink(&outside, &link).is_ok());

    let rules = collect_agent_rules_from_paths([link]);

    assert_eq!(rules, "(no AGENTS.md rules discovered)");
}

#[test]
fn agent_prompt_rules_refuse_symlinked_parent_dirs() {
    let root = clean_test_dir("agent-prompt-rules-symlink-parent");
    let outside = root.join("outside");
    let link_parent = root.join("project");
    assert!(fs::create_dir_all(&outside).is_ok());
    write_text_file(&outside.join("AGENTS.md"), "outside rule\n");
    assert!(symlink(&outside, &link_parent).is_ok());

    let rules = collect_agent_rules_from_paths([link_parent.join("AGENTS.md")]);

    assert_eq!(rules, "(no AGENTS.md rules discovered)");
}

#[test]
fn agent_prompt_rules_refuse_symlinked_intermediate_dirs() {
    let root = clean_test_dir("agent-prompt-rules-symlink-intermediate");
    let outside = root.join("outside").join("nested");
    let link_parent = root.join("project");
    assert!(fs::create_dir_all(&outside).is_ok());
    write_text_file(&outside.join("AGENTS.md"), "outside rule\n");
    assert!(symlink(root.join("outside"), &link_parent).is_ok());

    let rules = collect_agent_rules_from_paths([link_parent.join("nested").join("AGENTS.md")]);

    assert_eq!(rules, "(no AGENTS.md rules discovered)");
}

#[test]
fn agent_prompt_rules_refuse_oversized_files() {
    let root = clean_test_dir("agent-prompt-rules-oversized");
    let rules_path = root.join("AGENTS.md");
    write_text_file(&rules_path, &"x".repeat(64 * 1024 + 1));

    let rules = collect_agent_rules_from_paths([rules_path]);

    assert_eq!(rules, "(no AGENTS.md rules discovered)");
}

#[test]
fn agent_prompt_skill_discovery_rejects_symlinked_skill_root_parent() {
    let root = clean_test_dir("agent-prompt-skills-symlink-parent");
    let outside = root.join("outside").join(".agents");
    let skill = outside.join("skills").join("escape").join("SKILL.md");
    write_text_file(
        &skill,
        "---\nname: escape-skill\nsummary: outside skill\n---\n",
    );
    assert!(symlink(&outside, root.join(".agents")).is_ok());

    let skills = crate::agent::prompt::discover_skill_metadata_from_roots([
        root.join(".agents").join("skills"),
        root.join(".codex").join("skills"),
    ]);
    let skills = format_skill_metadata_with_budget(&skills, 8_000);

    assert!(!skills.contains("escape-skill"));
}

#[test]
fn agent_prompt_skill_metadata_discovers_project_ancestor_skills() {
    let root = clean_test_dir("agent-prompt-skills-project-ancestor");
    let nested = root.join("workspace").join("crates").join("cortexfs");
    write_text_file(
        &root
            .join(".agents")
            .join("skills")
            .join("project-skill")
            .join("SKILL.md"),
        "---\nname: project-skill\ndescription: project skill from ancestor\n---\n",
    );
    assert!(fs::create_dir_all(&nested).is_ok());
    let mut roots = Vec::new();
    crate::agent::prompt::push_project_skill_roots(&mut roots, &nested);
    let skills = crate::agent::prompt::discover_skill_metadata_from_roots(roots);
    let skills = format_skill_metadata_with_budget(&skills, 8_000);

    assert!(skills.contains("name: project-skill"));
    assert!(skills.contains("project skill from ancestor"));
    assert!(skills.contains(".agents/skills/project-skill/SKILL.md"));
}

#[test]
fn agent_prompt_skill_metadata_shortens_then_omits_over_budget() {
    let skills = vec![
        SkillMetadata {
            name: "alpha".to_owned(),
            description: "A ".repeat(300),
            path: PathBuf::from("/skills/alpha/SKILL.md"),
        },
        SkillMetadata {
            name: "beta".to_owned(),
            description: "B ".repeat(300),
            path: PathBuf::from("/skills/beta/SKILL.md"),
        },
    ];

    let shortened = format_skill_metadata_with_budget(&skills, 520);
    assert!(shortened.contains("WARNING: skill descriptions were shortened"));
    assert!(shortened.contains("name: alpha"));
    assert!(shortened.contains("name: beta"));

    let omitted = format_skill_metadata_with_budget(&skills, 340);
    assert!(omitted.contains("WARNING: skill metadata exceeded"));
    assert!(omitted.contains("name: alpha"));
    assert!(!omitted.contains("name: beta"));
}

#[test]
fn agent_prompt_skill_metadata_respects_tiny_budget() {
    let skills = vec![SkillMetadata {
        name: "alpha".to_owned(),
        description: "A ".repeat(300),
        path: PathBuf::from("/skills/alpha/SKILL.md"),
    }];

    let tiny = format_skill_metadata_with_budget(&skills, 1);

    assert!(tiny.len() <= 1);
}

#[test]
fn agent_prompt_history_messages_are_bounded_and_recent() {
    let messages = concat!(
        "{\"role\":\"user\",\"content\":\"old question old question old question old question old question old question old question old question old question old question\"}\n",
        "{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"old answer\"}]}\n",
        "{\"role\":\"user\",\"content\":\"new question\"}\n",
    );

    let full = format_history_messages_jsonl(messages, 10_000);
    assert!(full.contains("- user: old question old question"));
    assert!(full.contains("- assistant: old answer"));
    assert!(full.contains("- user: new question"));

    let bounded = format_history_messages_jsonl(messages, 120);
    assert!(bounded.contains("WARNING: historical messages exceeded"));
    assert!(!bounded.contains("old question"));
    assert!(bounded.contains("new question"));
}

#[test]
fn agent_prompt_history_messages_respect_tiny_budget() {
    let messages = serde_json::json!({
        "role": "user",
        "content": "large message"
    })
    .to_string();

    let tiny = format_history_messages_jsonl(&messages, 1);

    assert!(tiny.len() <= 1);
}

#[test]
fn agent_prompt_history_session_reads_only_bounded_recent_tail() {
    let root = clean_test_dir("agent-prompt-history-bounded-tail");
    let session = root.join("session").join("default");
    assert!(fs::create_dir_all(&session).is_ok());
    let old = serde_json::json!({
        "role": "user",
        "content": "old message".repeat(70_000)
    })
    .to_string();
    let recent = serde_json::json!({
        "role": "assistant",
        "content": "recent answer"
    })
    .to_string();
    write_text_file(
        &session.join("messages.jsonl"),
        &format!("{old}\n{recent}\n"),
    );

    let history = collect_history_messages_from_session(&session, 10_000);

    assert!(!history.contains("old message"));
    assert_eq!(history, "- assistant: recent answer");
}

#[test]
fn agent_prompt_history_skips_oversized_lines_before_rendering() {
    let messages = format!(
        "{}\n{}\n",
        serde_json::json!({"role": "user", "content": "x".repeat(20_000)}),
        serde_json::json!({"role": "assistant", "content": "small"})
    );

    let history = format_history_messages_jsonl(&messages, 10_000);

    assert_eq!(history, "- assistant: small");
}

#[test]
fn agent_prompt_history_messages_read_session_file() {
    let root = clean_test_dir("agent-prompt-history-session");
    let session = root.join("session").join("default");
    assert!(fs::create_dir_all(&session).is_ok());
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"hello\"}\n",
    );

    let history = collect_history_messages_from_session(&session, 10_000);

    assert_eq!(history, "- user: hello");
}

#[test]
fn agent_prompt_history_refuses_symlinked_messages_file() {
    let root = clean_test_dir("agent-prompt-history-symlink");
    let session = root.join("session").join("default");
    assert!(fs::create_dir_all(&session).is_ok());
    let outside = root.join("outside-messages.jsonl");
    write_text_file(
        &outside,
        "{\"role\":\"user\",\"content\":\"external secret\"}\n",
    );
    assert!(symlink(&outside, session.join("messages.jsonl")).is_ok());

    let history = collect_history_messages_from_session(&session, 10_000);

    assert_eq!(history, "(no historical messages injected)");
}

#[test]
fn agent_prompt_history_refuses_symlinked_session_dir() {
    let root = clean_test_dir("agent-prompt-history-symlink-session");
    let outside = root.join("outside-session");
    let link = root.join("session-link");
    assert!(fs::create_dir_all(&outside).is_ok());
    write_text_file(
        &outside.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"external secret\"}\n",
    );
    assert!(symlink(&outside, &link).is_ok());

    let history = collect_history_messages_from_session(&link, 10_000);

    assert_eq!(history, "(no historical messages injected)");
}

#[test]
fn agent_prompt_history_refuses_symlinked_intermediate_session_dir() {
    let root = clean_test_dir("agent-prompt-history-symlink-intermediate");
    let outside = root.join("outside").join("default");
    let link = root.join("sessions");
    assert!(fs::create_dir_all(&outside).is_ok());
    write_text_file(
        &outside.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"external secret\"}\n",
    );
    assert!(symlink(root.join("outside"), &link).is_ok());

    let history = collect_history_messages_from_session(&link.join("default"), 10_000);

    assert_eq!(history, "(no historical messages injected)");
}
