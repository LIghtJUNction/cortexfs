#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fmt::Write as _;
    use std::fs;
    use std::io;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::Path;
    use std::process::Command;

    use serde_json::json;
    use sha2::{Digest, Sha256};

    type State = Vec<(OsString, u8, u32, u64, u64, Vec<u8>)>;

    fn sha256(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .fold(String::with_capacity(64), |mut output, byte| {
                let _ignored = write!(output, "{byte:02x}");
                output
            })
    }

    fn tool_manifest(artifact: &Path, digest: &str) -> serde_json::Value {
        json!({
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
        })
    }

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
                    metadata.dev(),
                    metadata.ino(),
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

        let digest = sha256(artifact_bytes);
        let mut manifest = tool_manifest(&artifact, &digest);
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

    #[test]
    fn install_then_inspect_detects_tamper_without_modifying_object()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let source = root.path().join("source");
        let tool_dir = source.join("tool");
        fs::create_dir_all(&tool_dir)?;
        let artifact = root.path().join("echo-tool");
        let artifact_bytes = b"#!/bin/sh\nprintf ok\n";
        fs::write(&artifact, artifact_bytes)?;
        fs::set_permissions(&artifact, fs::Permissions::from_mode(0o755))?;
        let digest = sha256(artifact_bytes);
        let manifest = root.path().join("tool.json");
        fs::write(
            &manifest,
            serde_json::to_vec(&tool_manifest(&artifact, &digest))?,
        )?;

        let output = Command::new(env!("CARGO_BIN_EXE_ctx"))
            .args(["object", "install", "--source"])
            .arg(&source)
            .arg(&manifest)
            .args(["--tier", "system"])
            .output()?;
        assert!(
            output.status.success(),
            "ctx object install failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"installed tool/example.echo\n");

        let installed = tool_dir.join("example.echo");
        let control = tool_dir.join("example.echo.d");
        let executable_metadata = fs::metadata(&installed)?;
        let control_metadata = fs::metadata(&control)?;
        let before = (state(&tool_dir)?, state(&control)?);
        let output = Command::new(env!("CARGO_BIN_EXE_ctx"))
            .args([
                "object",
                "inspect",
                "tool",
                "example.echo",
                "--tier",
                "system",
                "--source",
            ])
            .arg(&source)
            .output()?;
        assert!(
            output.status.success(),
            "ctx object inspect failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout)?,
            format!(
                "installed tool/example.echo tier=system schema=cortexfs.object/v1 sha256={digest} executable={}:{} control={}:{}\n",
                executable_metadata.dev(),
                executable_metadata.ino(),
                control_metadata.dev(),
                control_metadata.ino(),
            )
        );
        assert_eq!((state(&tool_dir)?, state(&control)?), before);

        let receipt_path = control.join(".cortexfs-receipt.json");
        let receipt_bytes = fs::read(&receipt_path)?;
        let mut receipt: serde_json::Value = serde_json::from_slice(&receipt_bytes)?;
        let schema = receipt
            .get_mut("schema")
            .ok_or("receipt schema is missing")?;
        *schema = serde_json::Value::String("cortexfs.object-install/v2\nINJECTED".to_owned());
        fs::set_permissions(&receipt_path, fs::Permissions::from_mode(0o644))?;
        fs::write(&receipt_path, serde_json::to_vec(&receipt)?)?;
        let output = Command::new(env!("CARGO_BIN_EXE_ctx"))
            .args(["object", "inspect", "--source"])
            .arg(&source)
            .args(["tool", "example.echo", "--tier", "system"])
            .output()?;
        assert_eq!(output.status.code(), Some(69));
        let stderr = String::from_utf8(output.stderr)?;
        assert_eq!(stderr.lines().count(), 1);
        assert!(stderr.contains("v2\\nINJECTED"));
        fs::write(&receipt_path, receipt_bytes)?;
        fs::set_permissions(&receipt_path, fs::Permissions::from_mode(0o444))?;
        assert_eq!((state(&tool_dir)?, state(&control)?), before);

        fs::write(&installed, b"#!/bin/sh\nprintf tampered\n")?;
        let before = (state(&tool_dir)?, state(&control)?);
        let output = Command::new(env!("CARGO_BIN_EXE_ctx"))
            .args(["object", "inspect", "--source"])
            .arg(&source)
            .args(["tool", "example.echo", "--tier", "system"])
            .output()?;
        assert_eq!(output.status.code(), Some(69));
        assert!(String::from_utf8_lossy(&output.stderr).contains("sha256"));
        assert_eq!((state(&tool_dir)?, state(&control)?), before);
        Ok(())
    }
}
