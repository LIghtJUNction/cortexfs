#[cfg(unix)]
#[test]
fn prompt_discovery_skips_symlinked_agents_files_and_skill_trees()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let _guard = env_lock()
        .lock()
        .map_err(|_error| std::io::Error::other("env lock poisoned"))?;
    let original_cwd = std::env::current_dir()?;
    let root = unique_temp_dir("prompt-symlinks")?;
    let workspace = root.join("workspace");
    let outside = root.join("outside");
    fs::create_dir_all(&workspace)?;
    fs::create_dir_all(outside.join("skills").join("leak"))?;
    fs::write(outside.join("secret.txt"), "SHOULD_NOT_LEAK_RULE")?;
    fs::write(
        outside.join("skills").join("leak").join("SKILL.md"),
        "---\nname: leak\ndescription: SHOULD_NOT_LEAK_SKILL\n---\nbody",
    )?;
    symlink(outside.join("secret.txt"), workspace.join("AGENTS.md"))?;
    fs::create_dir_all(workspace.join(".agents"))?;
    symlink(outside.join("skills"), workspace.join(".agents").join("skills"))?;

    std::env::set_current_dir(&workspace)?;
    let rules = collect_agent_rules();
    let skills = collect_skill_metadata(8_000);

    std::env::set_current_dir(original_cwd)?;
    let _ignored = fs::remove_dir_all(root);

    assert!(!rules.contains("SHOULD_NOT_LEAK_RULE"));
    assert!(!skills.contains("SHOULD_NOT_LEAK_SKILL"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn prompt_discovery_enforces_file_size_limits() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = env_lock()
        .lock()
        .map_err(|_error| std::io::Error::other("env lock poisoned"))?;
    let original_cwd = std::env::current_dir()?;
    let root = unique_temp_dir("prompt-limits")?;
    let workspace = root.join("workspace");
    fs::create_dir_all(workspace.join(".agents").join("skills").join("large"))?;
    fs::write(workspace.join("AGENTS.md"), "A".repeat(70 * 1024))?;
    fs::write(
        workspace
            .join(".agents")
            .join("skills")
            .join("large")
            .join("SKILL.md"),
        format!("---\nname: large\ndescription: {}\n---\n", "B".repeat(20 * 1024)),
    )?;

    std::env::set_current_dir(&workspace)?;
    let rules = collect_agent_rules();
    let skills = collect_skill_metadata(8_000);

    std::env::set_current_dir(original_cwd)?;
    let _ignored = fs::remove_dir_all(root);

    assert!(!rules.contains(&"A".repeat(1024)));
    assert!(!skills.contains("large"));
    Ok(())
}
