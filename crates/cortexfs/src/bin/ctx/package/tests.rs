use super::*;
use crate::{Command, parse_command};
use cortexfs::object::install::InstallTier;
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[test]
fn package_command_keeps_install_surface_small() {
    let parsed = parse_command(vec![
        "install".to_owned(),
        "--tier".to_owned(),
        "system".to_owned(),
        "./kit".to_owned(),
    ]);
    assert!(matches!(parsed, Ok(Command::PackageInstall { .. })));
}

#[test]
fn package_rejects_agent_identity() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    fs::write(
        root.path().join("cortexfs.toml"),
        r#"[[agents]]
name = "agent"
run = "bin/agent"

[agents.identity]
uid = 0
gid = 0
groups = [0]
"#,
    )?;

    let error = match manifest::load_package(root.path()) {
        Ok(_) => panic!("package-controlled identity was accepted"),
        Err(error) => error,
    };
    assert!(error.message.contains("unknown field `identity`"));
    Ok(())
}

#[test]
fn package_installs_tool_agent_and_parent_edge() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let package = root.path().join("kit");
    let source = root.path().join("source");
    fs::create_dir_all(&package)?;
    fs::write(
        package.join("cortexfs.toml"),
        r#"schema = "cortexfs.package/v1"
name = "review-kit"

[[tools]]
name = "hello"
run = "bin/hello"
schema = { type = "object" }

[[agents]]
name = "kit_reviewer"
run = "bin/reviewer"
model = "debug/echo"
tools = ["hello"]
parent = "agent:architect"
"#,
    )?;
    fs::create_dir_all(package.join("bin"))?;
    for name in ["hello", "reviewer"] {
        let path = package.join("bin").join(name);
        fs::write(&path, "#!/bin/sh\nprintf ok\n")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    }

    run_package_install(&package, Some(&source), InstallTier::System)
        .map_err(|error| std::io::Error::other(error.message))?;
    assert_eq!(
        fs::read_to_string(source.join("tool/hello.d/policy"))?,
        "allow kit_reviewer_t tool:hello execute\n"
    );
    assert_eq!(
        fs::read_to_string(source.join("tool/hello.d/schema"))?,
        "{\"type\":\"object\"}\n"
    );
    assert_eq!(
        fs::read_to_string(source.join("agent/kit_reviewer.d/model"))?,
        "debug/echo\n"
    );
    assert_eq!(
        fs::read_to_string(source.join("agent/kit_reviewer.d/parent"))?,
        "agent:architect\n"
    );
    assert!(source.join("agent/kit_reviewer").is_file());
    Ok(())
}
