use super::{manifest, run_package_install};
use cortexfs::object::install::InstallTier;
use std::fs;
use std::os::unix::fs::PermissionsExt;

const SCRIPT: &str = "#!/bin/sh\nprintf ok\n";
const SCRIPT_SHA256: &str = "cdcc04283294dbab6a333988417cbf5846a763b12c63f74edc857a55b6565eb4";

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
    let Err(error) = manifest::load_package(root.path()) else {
        return Err("package-controlled identity was accepted".into());
    };
    assert!(error.message.contains("unknown field `identity`"));
    Ok(())
}

#[test]
fn required_hashes_bind_package_executables() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let package = root.path().join("kit");
    let executable = package.join("bin/hello");
    fs::create_dir_all(executable.parent().ok_or("missing executable parent")?)?;
    fs::write(
        package.join("cortexfs.toml"),
        format!(
            r#"schema = "cortexfs.package/v1"
[[tools]]
name = "hello"
run = "bin/hello"
sha256 = "{SCRIPT_SHA256}"
schema = {{ type = "object" }}
"#,
        ),
    )?;
    fs::write(&executable, SCRIPT)?;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))?;
    run_package_install(&package, None, InstallTier::System, true, true)
        .map_err(|error| std::io::Error::other(error.message))?;

    fs::write(&executable, "#!/bin/sh\nprintf tampered\n")?;
    let Err(error) = run_package_install(&package, None, InstallTier::System, true, true) else {
        return Err("tampered package executable was accepted".into());
    };
    assert!(error.message.contains("sha256 mismatch: tool/hello"));

    fs::write(
        package.join("cortexfs.toml"),
        r#"schema = "cortexfs.package/v1"
[[tools]]
name = "hello"
run = "bin/hello"
schema = { type = "object" }
"#,
    )?;
    let Err(error) = run_package_install(&package, None, InstallTier::System, true, true) else {
        return Err("hashless package executable was accepted".into());
    };
    assert!(error.message.contains("sha256 is required: tool/hello"));
    Ok(())
}
