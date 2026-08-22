mod integrity;
use super::*;
use crate::{Command, parse_command};
use cortexfs::object::install::InstallTier;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

#[test]
fn package_command_keeps_install_surface_small() {
    let parsed = parse_command(vec![
        "install".to_owned(),
        "--tier".to_owned(),
        "system".to_owned(),
        "./kit".to_owned(),
    ]);
    assert!(matches!(
        parsed,
        Ok(Command::PackageInstall {
            check: false,
            require_hashes: false,
            ..
        })
    ));
    assert!(matches!(
        parse_command(
            ["install", "--check", "--require-hashes", "./kit"]
                .map(str::to_owned)
                .to_vec()
        ),
        Ok(Command::PackageInstall {
            check: true,
            require_hashes: true,
            ..
        })
    ));
    assert!(
        parse_command(
            ["install", "--check", "--source", "/tree", "./kit"]
                .map(str::to_owned)
                .to_vec()
        )
        .is_err()
    );
}

#[test]
fn package_check_validates_without_writing_source() -> Result<(), Box<dyn std::error::Error>> {
    let (root, package) = package_fixture()?;
    let source = root.path().join("source");
    run_package_install(&package, Some(&source), InstallTier::System, true, false)
        .map_err(|error| std::io::Error::other(error.message))?;
    assert!(!source.exists());
    Ok(())
}

#[test]
fn package_installs_tool_agent_and_parent_edge() -> Result<(), Box<dyn std::error::Error>> {
    let (root, package) = package_fixture()?;
    let source = root.path().join("source");
    run_package_install(&package, Some(&source), InstallTier::System, false, false)
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

fn package_fixture() -> Result<(tempfile::TempDir, PathBuf), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let package = root.path().join("kit");
    fs::create_dir_all(package.join("bin"))?;
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
    for name in ["hello", "reviewer"] {
        let path = package.join("bin").join(name);
        fs::write(&path, "#!/bin/sh\nprintf ok\n")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    }
    Ok((root, package))
}
