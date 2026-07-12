#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fmt::Write as _;
    use std::fs;
    use std::io;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::process::Command;

    use serde_json::json;
    use sha2::{Digest, Sha256};

    type State = Vec<(OsString, u8, u32, Vec<u8>)>;

    fn state(root: &Path) -> io::Result<State> {
        let mut state = fs::read_dir(root)?
            .map(|entry| {
                let entry = entry?;
                let metadata = fs::symlink_metadata(entry.path())?;
                let kind = if metadata.is_file() {
                    0
                } else if metadata.is_dir() {
                    1
                } else if metadata.file_type().is_symlink() {
                    2
                } else {
                    3
                };
                let content = if metadata.is_file() {
                    fs::read(entry.path())?
                } else {
                    Vec::new()
                };
                Ok((
                    entry.file_name(),
                    kind,
                    metadata.permissions().mode() & 0o7777,
                    content,
                ))
            })
            .collect::<io::Result<Vec<_>>>()?;
        state.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(state)
    }

    #[test]
    fn check_is_read_only() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let artifact = root.path().join("echo-tool");
        let artifact_bytes = b"#!/bin/sh\nprintf ok\n";
        fs::write(&artifact, artifact_bytes)?;
        fs::set_permissions(&artifact, fs::Permissions::from_mode(0o755))?;
        assert_eq!(
            fs::metadata(&artifact)?.permissions().mode() & 0o7777,
            0o755
        );

        let digest = Sha256::digest(artifact_bytes).iter().fold(
            String::with_capacity(64),
            |mut output, byte| {
                let _ignored = write!(output, "{byte:02x}");
                output
            },
        );
        let mut manifest = json!({
            "schema": "cortexfs.object/v1",
            "class": "tool",
            "name": "example.echo",
            "executable": { "path": artifact, "sha256": digest },
            "controls": {
                "description": "echo",
                "schema": r#"{"type":"object"}"#,
                "cap": "text",
                "policy": "allow example_t tool:example.echo execute"
            }
        });
        let path = root.path().join("tool.json");
        fs::write(&path, serde_json::to_vec(&manifest)?)?;

        let before = state(root.path())?;
        let output = Command::new(env!("CARGO_BIN_EXE_ctx"))
            .args(["object", "check"])
            .arg(&path)
            .output()?;
        assert!(
            output.status.success(),
            "ctx object check failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"valid tool/example.echo\n");
        assert_eq!(state(root.path())?, before);

        let manifest_digest = manifest
            .pointer_mut("/executable/sha256")
            .ok_or("manifest executable sha256 is missing")?;
        *manifest_digest = json!("0".repeat(64));
        fs::write(&path, serde_json::to_vec(&manifest)?)?;
        let before = state(root.path())?;
        let output = Command::new(env!("CARGO_BIN_EXE_ctx"))
            .args(["object", "check"])
            .arg(&path)
            .output()?;
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("sha256"));
        assert_eq!(state(root.path())?, before);
        Ok(())
    }
}
