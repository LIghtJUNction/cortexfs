#[test]
fn ctx_path_parses_without_implicit_current_directory() {
    let path = ToolPath::parse(":/ctx/tool::/ctx/home/1000/tool:");
    assert_eq!(
        path.dirs(),
        [
            PathBuf::from("/ctx/tool"),
            PathBuf::from("/ctx/home/1000/tool")
        ]
    );
}

#[test]
fn tool_lookup_uses_first_executable_hit() {
    let root = clean_test_dir("tool-lookup");
    let global = root.join("global-tool");
    let user = root.join("user-tool");
    assert!(fs::create_dir_all(&global).is_ok());
    assert!(fs::create_dir_all(&user).is_ok());

    write_fixture_file(&global.join("fs.read"), 0o644);
    write_fixture_file(&global.join("fs.write"), 0o755);
    write_fixture_file(&user.join("fs.read"), 0o755);
    assert!(fs::create_dir_all(user.join("fs.read.d")).is_ok());

    let path = ToolPath::new([global.clone(), user.clone()]);
    let found = path.find("fs.read");
    assert!(matches!(found, Ok(Some(ref hit)) if hit.path() == user.join("fs.read")));
    assert!(matches!(found, Ok(Some(ref hit)) if hit.control_dir() == user.join("fs.read.d")));

    write_fixture_file(&global.join("fs.read"), 0o755);
    let found = path.find("fs.read");
    assert!(matches!(found, Ok(Some(ref hit)) if hit.path() == global.join("fs.read")));
}
