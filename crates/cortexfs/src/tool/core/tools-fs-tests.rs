#![expect(clippy::redundant_pub_crate, reason = "test functions inside module")]

use super::*;
use crate::*;

#[test]
pub(crate) fn fs_read_tool_emits_file_content() {
    let path = std::env::temp_dir().join(format!("cortexfs-fs-read-{}", std::process::id()));
    assert!(fs::write(&path, "visible").is_ok());
    let tool = FsReadTool;
    let invocation = ToolInvocation::new("r1", format!(r#"{{"path":"{}"}}"#, path.display()));
    let mut output = Vec::new();
    assert!(run_tool(&tool, &invocation, &mut output).is_ok());
    let text = String::from_utf8(output).unwrap_or_default();
    assert!(text.contains(r#""tool":"fs.read""#));
    assert!(text.contains(r#""text":"visible""#));
    let _ignored = fs::remove_file(path);
}

#[test]
pub(crate) fn fs_read_tool_refuses_symlink_targets() {
    let dir = std::env::temp_dir().join(format!("cortexfs-fs-read-symlink-{}", std::process::id()));
    let outside = std::env::temp_dir().join(format!(
        "cortexfs-fs-read-symlink-outside-{}",
        std::process::id()
    ));
    assert!(fs::create_dir_all(&dir).is_ok());
    assert!(fs::write(&outside, "outside").is_ok());
    let link = dir.join("link");
    assert!(symlink(&outside, &link).is_ok());

    let tool = FsReadTool;
    let invocation = ToolInvocation::new("r1", format!(r#"{{"path":"{}"}}"#, link.display()));
    let mut output = Vec::new();
    assert!(run_tool(&tool, &invocation, &mut output).is_ok());

    let text = String::from_utf8(output).unwrap_or_default();
    assert!(text.contains(r#""code":"EACCES""#));
    assert!(!text.contains("outside"));
    let _ignored = fs::remove_dir_all(dir);
    let _ignored = fs::remove_file(outside);
}

#[test]
pub(crate) fn fs_read_tool_refuses_symlink_intermediate_parent() {
    let dir = std::env::temp_dir().join(format!(
        "cortexfs-fs-read-symlink-intermediate-{}",
        std::process::id()
    ));
    let outside = std::env::temp_dir().join(format!(
        "cortexfs-fs-read-symlink-intermediate-outside-{}",
        std::process::id()
    ));
    let _ignored = fs::remove_dir_all(&dir);
    let _ignored = fs::remove_dir_all(&outside);
    assert!(fs::create_dir_all(&dir).is_ok());
    assert!(fs::create_dir_all(outside.join("sub")).is_ok());
    assert!(fs::write(outside.join("sub/secret.txt"), "outside").is_ok());
    assert!(symlink(&outside, dir.join("workspace")).is_ok());

    let tool = FsReadTool;
    let invocation = ToolInvocation::new(
        "r1",
        format!(
            r#"{{"path":"{}"}}"#,
            dir.join("workspace/sub/secret.txt").display()
        ),
    );
    let mut output = Vec::new();
    assert!(run_tool(&tool, &invocation, &mut output).is_ok());

    let text = String::from_utf8(output).unwrap_or_default();
    assert!(text.contains(r#""code":"EACCES""#));
    assert!(!text.contains("outside"));
    let _ignored = fs::remove_dir_all(dir);
    let _ignored = fs::remove_dir_all(outside);
}

#[test]
pub(crate) fn fs_write_tool_writes_file_content() {
    let path = std::env::temp_dir().join(format!("cortexfs-fs-write-{}", std::process::id()));
    let tool = FsWriteTool;
    let invocation = ToolInvocation::new(
        "r1",
        format!(r#"{{"path":"{}","content":"stored"}}"#, path.display()),
    );
    let mut output = Vec::new();
    assert!(run_tool(&tool, &invocation, &mut output).is_ok());
    assert_eq!(fs::read_to_string(&path).unwrap_or_default(), "stored");
    let _ignored = fs::remove_file(path);
}

#[test]
pub(crate) fn fs_write_tool_replaces_symlink_without_writing_target() {
    let dir =
        std::env::temp_dir().join(format!("cortexfs-fs-write-symlink-{}", std::process::id()));
    let outside = std::env::temp_dir().join(format!(
        "cortexfs-fs-write-symlink-outside-{}",
        std::process::id()
    ));
    assert!(fs::create_dir_all(&dir).is_ok());
    assert!(fs::write(&outside, "outside").is_ok());
    let link = dir.join("link");
    assert!(symlink(&outside, &link).is_ok());

    let tool = FsWriteTool;
    let invocation = ToolInvocation::new(
        "r1",
        format!(r#"{{"path":"{}","content":"stored"}}"#, link.display()),
    );
    let mut output = Vec::new();
    assert!(run_tool(&tool, &invocation, &mut output).is_ok());

    assert_eq!(fs::read_to_string(&outside).unwrap_or_default(), "outside");
    assert_eq!(fs::read_to_string(&link).unwrap_or_default(), "stored");
    assert!(
        link.symlink_metadata()
            .is_ok_and(|metadata| metadata.is_file())
    );
    let _ignored = fs::remove_dir_all(dir);
    let _ignored = fs::remove_file(outside);
}

#[test]
pub(crate) fn fs_write_tool_rejects_symlink_parent_without_writing_target() {
    let dir = std::env::temp_dir().join(format!(
        "cortexfs-fs-write-symlink-parent-{}",
        std::process::id()
    ));
    let outside = std::env::temp_dir().join(format!(
        "cortexfs-fs-write-symlink-parent-outside-{}",
        std::process::id()
    ));
    let _ignored = fs::remove_dir_all(&dir);
    let _ignored = fs::remove_dir_all(&outside);
    assert!(fs::create_dir_all(&dir).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    let link = dir.join("workspace");
    assert!(symlink(&outside, &link).is_ok());

    let tool = FsWriteTool;
    let invocation = ToolInvocation::new(
        "r1",
        format!(
            r#"{{"path":"{}","content":"stored"}}"#,
            link.join("result.txt").display()
        ),
    );
    let mut output = Vec::new();
    assert!(run_tool(&tool, &invocation, &mut output).is_ok());

    let text = String::from_utf8(output).unwrap_or_default();
    assert!(text.contains(r#""code":"EACCES""#));
    assert!(!outside.join("result.txt").exists());
    assert!(!fs::read_dir(&outside).map_or(true, |entries| {
        entries.filter_map(Result::ok).any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".result.txt.tmp-")
        })
    }));
    let _ignored = fs::remove_dir_all(dir);
    let _ignored = fs::remove_dir_all(outside);
}

#[test]
pub(crate) fn fs_write_tool_rejects_symlink_intermediate_parent() {
    let dir = std::env::temp_dir().join(format!(
        "cortexfs-fs-write-symlink-intermediate-{}",
        std::process::id()
    ));
    let outside = std::env::temp_dir().join(format!(
        "cortexfs-fs-write-symlink-intermediate-outside-{}",
        std::process::id()
    ));
    let _ignored = fs::remove_dir_all(&dir);
    let _ignored = fs::remove_dir_all(&outside);
    assert!(fs::create_dir_all(&dir).is_ok());
    assert!(fs::create_dir_all(outside.join("sub")).is_ok());
    assert!(symlink(&outside, dir.join("workspace")).is_ok());

    let tool = FsWriteTool;
    let invocation = ToolInvocation::new(
        "r1",
        format!(
            r#"{{"path":"{}","content":"stored"}}"#,
            dir.join("workspace/sub/result.txt").display()
        ),
    );
    let mut output = Vec::new();
    assert!(run_tool(&tool, &invocation, &mut output).is_ok());

    let text = String::from_utf8(output).unwrap_or_default();
    assert!(text.contains(r#""code":"EACCES""#));
    assert!(!outside.join("sub/result.txt").exists());
    assert!(!fs::read_dir(outside.join("sub")).map_or(true, |entries| {
        entries.filter_map(Result::ok).any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".result.txt.tmp-")
        })
    }));
    let _ignored = fs::remove_dir_all(dir);
    let _ignored = fs::remove_dir_all(outside);
}

#[test]
pub(crate) fn fs_replace_tool_handles_unique_and_ambiguous_text_spans() {
    let tool = FsReplaceTool;
    for (label, before, expected, output_text) in [
        ("unique", "before old after", "before new after", "replaced"),
        ("ambiguous", "old old", "old old", r#""code":"EINVAL""#),
    ] {
        let path = std::env::temp_dir().join(format!(
            "cortexfs-fs-replace-{label}-{}",
            std::process::id()
        ));
        assert!(fs::write(&path, before).is_ok());
        let invocation = ToolInvocation::new(
            "r1",
            format!(r#"{{"path":"{}","old":"old","new":"new"}}"#, path.display()),
        );
        let mut output = Vec::new();
        assert!(run_tool(&tool, &invocation, &mut output).is_ok());

        assert_eq!(fs::read_to_string(&path).unwrap_or_default(), expected);
        assert!(
            String::from_utf8(output)
                .unwrap_or_default()
                .contains(output_text)
        );
        let _ignored = fs::remove_file(path);
    }
}

#[test]
pub(crate) fn fs_replace_cli_replaces_unique_text_span() {
    let path = std::env::temp_dir().join(format!("cortexfs-fs-replace-cli-{}", std::process::id()));
    assert!(fs::write(&path, "alpha beta gamma").is_ok());
    let mut output = Vec::new();
    let result = run_core_tool_cli(
        "fs.replace",
        &[
            OsString::from(&path),
            OsString::from("beta"),
            OsString::from("delta"),
        ],
        &mut output,
    );

    assert!(matches!(result, Ok(Some(code)) if code == std::process::ExitCode::SUCCESS));
    assert_eq!(String::from_utf8(output).unwrap_or_default(), "replaced\n");
    assert_eq!(
        fs::read_to_string(&path).unwrap_or_default(),
        "alpha delta gamma"
    );
    let _ignored = fs::remove_file(path);
}

#[test]
pub(crate) fn fs_write_cli_uses_atomic_writer() {
    let path = std::env::temp_dir().join(format!("cortexfs-fs-write-cli-{}", std::process::id()));
    let mut output = Vec::new();
    let result = run_core_tool_cli(
        "fs.write",
        &[OsString::from(&path), OsString::from("stored")],
        &mut output,
    );
    assert!(matches!(result, Ok(Some(code)) if code == std::process::ExitCode::SUCCESS));
    assert_eq!(String::from_utf8(output).unwrap_or_default(), "written\n");
    assert_eq!(fs::read_to_string(&path).unwrap_or_default(), "stored");
    let _ignored = fs::remove_file(path);
}

#[test]
pub(crate) fn fs_write_stdin_reader_accepts_input_at_limit() {
    let content = "x".repeat(crate::MAX_FUSE_V1_SMALL_WRITE_BYTES);

    let read = read_text_from_stdin_limited(
        Cursor::new(content.as_bytes()),
        crate::MAX_FUSE_V1_SMALL_WRITE_BYTES,
    );

    assert_eq!(
        read.unwrap_or_default().len(),
        crate::MAX_FUSE_V1_SMALL_WRITE_BYTES
    );
}

#[test]
pub(crate) fn fs_write_stdin_reader_rejects_input_over_limit() {
    let content = "x".repeat(crate::MAX_FUSE_V1_SMALL_WRITE_BYTES + 1);

    let read = read_text_from_stdin_limited(
        Cursor::new(content.as_bytes()),
        crate::MAX_FUSE_V1_SMALL_WRITE_BYTES,
    );

    assert!(matches!(read, Err(ref error) if error.kind() == std::io::ErrorKind::InvalidData));
}

#[test]
pub(crate) fn fs_read_cli_outputs_plain_text() {
    let path = std::env::temp_dir().join(format!("cortexfs-fs-read-cli-{}", std::process::id()));
    assert!(fs::write(&path, "plain").is_ok());
    let mut output = Vec::new();
    let result = run_core_tool_cli("fs.read", &[OsString::from(&path)], &mut output);
    assert!(matches!(result, Ok(Some(code)) if code == std::process::ExitCode::SUCCESS));
    assert_eq!(String::from_utf8(output).unwrap_or_default(), "plain");
    let _ignored = fs::remove_file(path);
}
