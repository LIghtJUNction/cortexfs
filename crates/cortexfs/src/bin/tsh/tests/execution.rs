use super::*;
use crate::*;

#[test]
fn persistent_context_uses_only_reserved_bwrap_home_mapping() {
    let authoritative = Path::new("/var/lib/cortexfs/storage/current/home/1000/agent/coder");
    assert_eq!(
        persistent_context_visible_home(authoritative, Some(std::ffi::OsStr::new("/home/agent"))),
        PathBuf::from("/home/agent")
    );
    assert_eq!(
        persistent_context_visible_home(authoritative, Some(std::ffi::OsStr::new("/tmp/escape"))),
        authoritative
    );
    assert_eq!(
        persistent_context_visible_home(authoritative, None),
        authoritative
    );
}

#[test]
pub(crate) fn tsh_terminal_without_run_capability_uses_ephemeral_context()
-> Result<(), Box<dyn std::error::Error>> {
    const CHILD: &str = "CORTEXFS_TSH_TERMINAL_EPHEMERAL_CHILD";
    if std::env::var_os(CHILD).is_some() {
        assert_eq!(persistent_context_path(Path::new("/ctx")), Ok(None));
        return Ok(());
    }
    let output = std::process::Command::new(std::env::current_exe()?)
        .arg("--exact")
        .arg("tests::execution::tsh_terminal_without_run_capability_uses_ephemeral_context")
        .arg("--nocapture")
        .env_clear()
        .env(CHILD, "1")
        .env("CTX_AGENT", "coder")
        .env_remove("CTX_SESSION")
        .env_remove("CTX_RUN_ID")
        .env_remove("CTX_SOURCE")
        .env_remove("CTX_CONTROL_SOCKET")
        .output()?;
    assert!(output.status.success(), "{output:?}");
    Ok(())
}

#[test]
fn tsh_cache_write_uses_real_authoritative_capability() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    if std::env::var_os("CORTEXFS_TSH_RECEIPT_CLIENT").is_some() {
        let projection =
            PathBuf::from(std::env::var_os("CORTEXFS_TSH_RECEIPT_ROOT").unwrap_or_default());
        let socket = std::env::var_os("CTX_CONTROL_SOCKET").unwrap_or_default();
        let path = persistent_context_path_with_capability(&projection, "coder", &socket)
            .map_err(|error| io::Error::other(error.message))?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "agent cache path missing"))?;
        assert!(fs::create_dir_all(path.parent().unwrap_or(&path)).is_ok());
        assert!(cortexfs::write_tsh_context_state(&path, &ToolContext::new(4).to_state()).is_ok());
        let load_path = persistent_context_path_with_capability(&projection, "coder", &socket)
            .map_err(|error| io::Error::other(error.message))?
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "loaded agent cache path missing")
            })?;
        assert!(cortexfs::read_tsh_context_state(&load_path).is_ok());
        let second_path = persistent_context_path_with_capability(&projection, "coder", &socket)
            .map_err(|error| io::Error::other(error.message))?
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "second agent cache path missing")
            })?;
        assert!(
            cortexfs::write_tsh_context_state(&second_path, &ToolContext::new(4).to_state())
                .is_ok()
        );
        assert!(path.is_file());
        return Ok(());
    }
    let root = tempfile::tempdir()?;
    let projection = root.path().join("projection");
    let source = root.path().join("source");
    for tree in [&projection, &source] {
        let control = tree.join("agent/coder.d");
        assert!(fs::create_dir_all(&control).is_ok());
        for (name, value) in [
            ("abi", "sdk-envelope-v1\n"),
            ("owner", "1000\n"),
            ("uid", "1000\n"),
            ("gid", "1000\n"),
            ("groups", "1000\n"),
            ("label", "user_u:agent_r:coder_t:s0\n"),
            ("iso", "shared\n"),
            ("parent", "\n"),
            ("life", "owned\n"),
            ("root", "/\n"),
            ("cwd", "/workspace\n"),
            ("env", "\n"),
            ("path", "/ctx/tool\n"),
            ("mount", "/ctx\t/ctx\tro\trbind,nosuid,nodev\n"),
            ("model", "local/chat\n"),
            ("window", "auto\n"),
            ("policy", "allow coder_t model:local/chat use\n"),
        ] {
            assert!(fs::write(control.join(name), value).is_ok());
        }
        let model_control = tree.join("model/local/chat.d");
        assert!(fs::create_dir_all(&model_control).is_ok());
        assert!(fs::write(model_control.join("limit"), "unknown\n").is_ok());
    }
    let session = source.join("home/1000/agent/coder/session/live");
    assert!(fs::create_dir_all(&session).is_ok());
    assert!(fs::write(session.join("current_run"), "run-1\n").is_ok());
    let control = root.path().join("control");
    assert!(fs::create_dir_all(&control).is_ok());
    assert!(fs::set_permissions(&control, fs::Permissions::from_mode(0o711)).is_ok());
    let identity = fs::metadata(&control)?;
    let (capability, listener) = cortexfs::runtime::control::RunCapability::create_with_source(
        &control,
        &source,
        "coder",
        "live",
        "run-1",
        identity.uid(),
        identity.gid(),
    )?;
    capability.register_launch_root(std::process::id())?;
    let environment = cortexfs::runtime::control::RunCapability::environment(capability.socket());
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_shutdown = Arc::clone(&shutdown);
    let (startup_tx, _startup_rx) = mpsc::sync_channel(1);
    let server = std::thread::spawn(move || {
        capability.serve_run(&listener, &server_shutdown, &startup_tx, || {
            Some("run-1".to_owned())
        })
    });
    let output = std::process::Command::new(std::env::current_exe()?)
        .arg("--exact")
        .arg("tests::execution::tsh_cache_write_uses_real_authoritative_capability")
        .arg("--nocapture")
        .env("CORTEXFS_TSH_RECEIPT_CLIENT", "1")
        .env("CORTEXFS_TSH_RECEIPT_ROOT", &projection)
        .env("CTX_AGENT", "coder")
        .env("CTX_SESSION", "live")
        .env("CTX_RUN_ID", "run-1")
        .env("CTX_SOURCE", &source)
        .env("CTX_CONTROL_SOCKET", &environment[0].1)
        .output()?;
    assert!(output.status.success(), "{output:?}");
    shutdown.store(true, Ordering::Release);
    assert!(matches!(server.join(), Ok(Ok(()))));
    assert!(
        source
            .join("home/1000/agent/coder/session/live/context/tsh.json")
            .is_file()
    );
    assert!(
        !projection
            .join("home/1000/agent/coder/session/live/context/tsh.json")
            .exists()
    );
    Ok(())
}

#[test]
fn tsh_source_receipt_rejects_clone_and_inode_replacement() -> Result<(), Box<dyn std::error::Error>>
{
    use std::os::unix::fs::MetadataExt;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let clone = root.path().join("clone");
    assert!(fs::create_dir_all(&source).is_ok());
    assert!(fs::create_dir_all(&clone).is_ok());
    let metadata = fs::metadata(&source)?;
    let receipt = cortexfs_runtime_client::RuntimeSourceReceipt {
        path: source.display().to_string(),
        dev: metadata.dev(),
        ino: metadata.ino(),
        kind: cortexfs_runtime_client::RuntimeSourceKind::PlainDirectory,
    };
    assert!(validate_runtime_source_receipt(&source, &receipt).is_ok());
    let cloned = cortexfs_runtime_client::RuntimeSourceReceipt {
        path: clone.display().to_string(),
        ..receipt
    };
    assert!(validate_runtime_source_receipt(&clone, &cloned).is_err());
    assert!(fs::rename(&source, root.path().join("old")).is_ok());
    assert!(fs::create_dir_all(&source).is_ok());
    assert!(validate_runtime_source_receipt(&source, &receipt).is_err());
    Ok(())
}

#[test]
pub(crate) fn tsh_persistent_cache_rejects_missing_runtime_receipt()
-> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("CORTEXFS_TSH_CACHE_CHILD").is_none() {
        let base =
            std::env::temp_dir().join(format!("cortexfs-tsh-cache-source-{}", std::process::id()));
        let projection = base.join("projection");
        let source = base.join("source");
        let output = std::process::Command::new(std::env::current_exe().unwrap_or_default())
            .arg("--exact")
            .arg("tests::execution::tsh_persistent_cache_rejects_missing_runtime_receipt")
            .arg("--nocapture")
            .env("CORTEXFS_TSH_CACHE_CHILD", "1")
            .env("CORTEXFS_TSH_CACHE_PROJECTION", &projection)
            .env("CTX_SOURCE", &source)
            .env("CTX_AGENT", "coder")
            .env("CTX_SESSION", "live")
            .env("CTX_RUN_ID", "run-1")
            .output();
        assert!(
            matches!(output, Ok(ref output) if output.status.success()),
            "{output:?}"
        );
        let _ignored = fs::remove_dir_all(base);
        return Ok(());
    }
    let projection =
        PathBuf::from(std::env::var_os("CORTEXFS_TSH_CACHE_PROJECTION").unwrap_or_default());
    let source = PathBuf::from(std::env::var_os("CTX_SOURCE").unwrap_or_default());
    for root in [&projection, &source] {
        let control = root.join("agent/coder.d");
        assert!(fs::create_dir_all(&control).is_ok());
        for (name, value) in [
            ("abi", "sdk-envelope-v1\n"),
            ("owner", "1000\n"),
            ("uid", "1000\n"),
            ("gid", "1000\n"),
            ("groups", "1000\n"),
            ("label", "user_u:agent_r:coder_t:s0\n"),
            ("iso", "shared\n"),
            ("parent", "\n"),
            ("life", "owned\n"),
            ("root", "/\n"),
            ("cwd", "/workspace\n"),
            ("env", "\n"),
            ("path", "/ctx/tool\n"),
            ("mount", "/ctx\t/ctx\tro\trbind,nosuid,nodev\n"),
            ("model", "main\n"),
            ("window", "auto\n"),
            ("policy", "allow coder_t model:main use\n"),
        ] {
            assert!(fs::write(control.join(name), value).is_ok());
        }
    }
    let session = source.join("home/1000/agent/coder/session/live");
    assert!(fs::create_dir_all(&session).is_ok());
    assert!(fs::write(session.join("current_run"), "run-1\n").is_ok());
    let Err(error) = persistent_context_path(&projection) else {
        return Err(io::Error::other("expected missing runtime receipt").into());
    };
    assert!(error.message.contains("authenticated runtime capability"));
    Ok(())
}

#[test]
pub(crate) fn tsh_refuses_tool_execution_without_agent_authority() {
    let root = std::env::temp_dir().join(format!("cortexfs-tsh-empty-argv-{}", std::process::id()));
    let tool_dir = root.join("tool");
    assert!(fs::create_dir_all(&tool_dir).is_ok());
    let tool = tool_dir.join("noop");
    assert!(fs::write(&tool, "#!/bin/sh\n[ \"$CTX_TOOL_MODE\" = cli ]\n").is_ok());
    assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());

    let result = run_tool(&root, "noop", Vec::new());
    assert!(matches!(
        result,
        Err(error)
            if error.message.contains("CTX_AGENT")
                && error.message.contains("ctx agent attach AGENT")
    ));
    let _ignored = fs::remove_dir_all(root);
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "single subprocess test exhaustively audits the sanitized tool environment"
)]
pub(crate) fn tsh_tool_execution_gets_clean_agent_environment() {
    if std::env::var_os("CORTEXFS_TSH_ENV_CHILD").is_none() {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-tsh-clean-tool-env-{}",
            std::process::id()
        ));
        let output = std::process::Command::new(std::env::current_exe().unwrap_or_default())
            .arg("--exact")
            .arg("tests::tsh_tool_execution_gets_clean_agent_environment")
            .arg("--nocapture")
            .env("CORTEXFS_TSH_ENV_CHILD", "1")
            .env("CORTEXFS_SHOULD_NOT_LEAK", "secret")
            .env("CORTEXFS_TSH_ROOT", &root)
            .env("CTX_AGENT", "coder")
            .env("CTX_SESSION", "live")
            .env("CTX_RUN_ID", "run-1")
            .env("CTX_SOURCE", &root)
            .env("CTX_CONTROL_SOCKET", "/run/cortexfs/control.sock")
            .output();
        assert!(matches!(output, Ok(ref output) if output.status.success()));
        return;
    }

    let root = std::env::var_os("CORTEXFS_TSH_ROOT")
        .map(PathBuf::from)
        .unwrap_or_default();
    let control = root.join("agent").join("coder.d");
    let tool_control = root.join("tool").join("probe.d");
    assert!(fs::create_dir_all(&control).is_ok());
    assert!(fs::create_dir_all(&tool_control).is_ok());
    assert!(fs::write(control.join("owner"), "1000\n").is_ok());
    assert!(fs::write(control.join("uid"), "1000\n").is_ok());
    assert!(fs::write(control.join("gid"), "1000\n").is_ok());
    assert!(fs::write(control.join("groups"), "1000\n").is_ok());
    assert!(fs::write(control.join("label"), "user_u:agent_r:coder_t:s0\n").is_ok());
    assert!(fs::write(control.join("iso"), "shared\n").is_ok());
    assert!(fs::write(control.join("parent"), "\n").is_ok());
    assert!(fs::write(control.join("life"), "owned\n").is_ok());
    assert!(fs::write(control.join("root"), "/ctx/home/1000/agent/coder/root\n").is_ok());
    assert!(fs::write(control.join("cwd"), "/workspace\n").is_ok());
    assert!(fs::write(control.join("env"), "\n").is_ok());
    assert!(fs::write(control.join("model"), "main\n").is_ok());
    assert!(fs::write(control.join("window"), "auto\n").is_ok());
    let model_control = root.join("model/local/chat.d");
    assert!(fs::create_dir_all(&model_control).is_ok());
    assert!(fs::write(model_control.join("limit"), "unknown\n").is_ok());
    assert!(std::os::unix::fs::symlink("/ctx/model/local/chat", root.join("model/main")).is_ok());
    assert!(fs::write(control.join("status"), "idle\n").is_ok());
    assert!(fs::write(control.join("pid"), "\n").is_ok());
    assert!(fs::write(control.join("log"), "\n").is_ok());
    assert!(fs::write(control.join("meta.json"), "{}\n").is_ok());
    let session = root.join("home/1000/agent/coder/session/live");
    assert!(fs::create_dir_all(&session).is_ok());
    assert!(fs::write(session.join("current_run"), "run-1\n").is_ok());
    assert!(
        fs::write(
            control.join("path"),
            format!("{}\n", root.join("tool").display())
        )
        .is_ok()
    );
    assert!(
        fs::write(
            control.join("mount"),
            format!(
                "{}\t{}\tro\trbind,nosuid,nodev\n",
                root.display(),
                root.display()
            ),
        )
        .is_ok()
    );
    assert!(
        fs::write(
            control.join("policy"),
            "allow coder_t model:main use\nallow coder_t tool:probe execute\n",
        )
        .is_ok()
    );
    assert!(
        fs::write(
            tool_control.join("policy"),
            "allow coder_t tool:probe execute\n"
        )
        .is_ok()
    );
    let tool = root.join("tool").join("probe");
    assert!(
        fs::write(
            &tool,
            r#"#!/bin/sh
[ -z "$CORTEXFS_SHOULD_NOT_LEAK" ] || exit 10
[ "$CTX_TOOL_MODE" = cli ] || exit 11
[ "$CTX_AGENT" = coder ] || exit 12
[ "$CTX_SESSION" = live ] || exit 16
[ "$CTX_RUN_ID" = run-1 ] || exit 17
[ "$CTX_SOURCE" = "$CTX_ROOT" ] || exit 18
[ -z "$CTX_CONTROL_SOCKET" ] || exit 19
[ "$(env | grep -c '^CTX_CONTROL_')" = 0 ] || exit 20
[ "$CTX_AUTHORIZED_OBJECT" = /ctx/tool/probe ] || exit 15
[ "$PATH" = /usr/bin:/bin ] || exit 13
[ -n "$CTX_ROOT" ] || exit 14
exit 0
"#,
        )
        .is_ok()
    );
    assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());

    let result = run_tool(&root, "probe", Vec::new());

    assert!(matches!(result, Ok(code) if code == ExitCode::SUCCESS));
    assert!(matches!(
        validate_tsh_runtime_context(&root, "coder", "live", "wrong-run", &root),
        Err(ref error) if error.message.contains("session mismatch")
    ));
    assert!(matches!(
        validate_tsh_runtime_context(&root, "coder", "../bad", "run-1", &root),
        Err(ref error) if error.message.contains("invalid agent runtime context")
    ));
    assert!(matches!(
        validate_tsh_runtime_context_values(
            &root,
            "coder",
            Some("live".to_owned()),
            None,
            Some(root.clone()),
        ),
        Err(ref error) if error.message.contains("missing CTX_RUN_ID")
    ));
    assert!(matches!(
        validate_tsh_control_environment(Some(OsString::from("/run/cortexfs/control.sock"))),
        Ok(Some(socket)) if socket == "/run/cortexfs/control.sock"
    ));
    assert!(matches!(
        validate_tsh_control_environment(Some(OsString::from(""))),
        Err(ref error) if error.message.contains("fixed runtime control path")
    ));

    let backing = root.join("backing");
    let backing_control = backing.join("agent/coder.d");
    assert!(fs::create_dir_all(&backing_control).is_ok());
    let entries = fs::read_dir(&control).ok();
    assert!(entries.is_some());
    if let Some(entries) = entries {
        for entry in entries.flatten() {
            if entry.metadata().is_ok_and(|metadata| metadata.is_file()) {
                assert!(fs::copy(entry.path(), backing_control.join(entry.file_name())).is_ok());
            }
        }
    }
    let backing_session = backing.join("home/1000/agent/coder/session/live");
    assert!(fs::create_dir_all(&backing_session).is_ok());
    assert!(fs::write(backing_session.join("current_run"), "run-1\n").is_ok());
    assert!(validate_tsh_runtime_context(&root, "coder", "live", "run-1", &backing).is_ok());
    for (file, replacement) in [
        ("env", "MISMATCH=1\n"),
        ("path", "/mismatch/tool\n"),
        ("mount", "/mismatch\t/workspace\tro\trbind,nosuid,nodev\n"),
        ("root", "/mismatch\n"),
    ] {
        let path = backing_control.join(file);
        let original = fs::read_to_string(&path).unwrap_or_default();
        assert!(fs::write(&path, replacement).is_ok());
        assert!(matches!(
            validate_tsh_runtime_context(&root, "coder", "live", "run-1", &backing),
            Err(ref error) if error.message.contains("source mismatch")
        ));
        assert!(fs::write(path, original).is_ok());
    }
    let _ignored = fs::remove_dir_all(root);
}

#[test]
pub(crate) fn repl_allows_empty_argv_for_normal_cli_tools() {
    let root = std::env::temp_dir().join(format!(
        "cortexfs-tsh-repl-empty-normal-{}",
        std::process::id()
    ));
    let tool_dir = root.join("tool");
    assert!(fs::create_dir_all(&tool_dir).is_ok());
    let tool = tool_dir.join("noop");
    assert!(fs::write(&tool, "#!/bin/sh\nexit 0\n").is_ok());
    assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());

    let mut context = ToolContext::new(4);
    let result = run_repl_tool(&root, &mut context, "noop", Vec::new());

    assert!(matches!(
        result,
        Err(error)
            if error.message.contains("CTX_AGENT")
                && error.message.contains("ctx agent attach AGENT")
    ));
    let _ignored = fs::remove_dir_all(root);
}

#[test]
pub(crate) fn repl_keeps_explicit_input_guard_for_structured_core_tools() {
    assert!(requires_explicit_repl_input("fs.read"));
    assert!(requires_explicit_repl_input("fs.write"));
    assert!(requires_explicit_repl_input("shell.exec"));
    assert!(!requires_explicit_repl_input("ls"));
    assert!(!requires_explicit_repl_input("project.test"));
}
