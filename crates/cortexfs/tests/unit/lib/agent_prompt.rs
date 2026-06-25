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

    let shortened = format_skill_metadata_with_budget(skills.clone(), 520);
    assert!(shortened.contains("WARNING: skill descriptions were shortened"));
    assert!(shortened.contains("name: alpha"));
    assert!(shortened.contains("name: beta"));

    let omitted = format_skill_metadata_with_budget(skills, 340);
    assert!(omitted.contains("WARNING: skill metadata exceeded"));
    assert!(omitted.contains("name: alpha"));
    assert!(!omitted.contains("name: beta"));
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
