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
