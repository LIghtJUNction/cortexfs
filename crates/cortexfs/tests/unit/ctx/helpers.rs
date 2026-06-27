fn unique_test_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!(
        "cortexfs-ctx-{name}-{}-{nanos}",
        std::process::id()
    ))
}

struct TestDir(PathBuf);

impl std::ops::Deref for TestDir {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<Path> for TestDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<std::ffi::OsStr> for TestDir {
    fn as_ref(&self) -> &std::ffi::OsStr {
        self.0.as_os_str()
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        // ponytail: best-effort test cleanup; stale startup cleanup covers killed test processes.
        let _ignored = fs::remove_dir_all(&self.0);
    }
}

fn clean_test_dir(name: &str) -> TestDir {
    remove_stale_test_dirs();
    let root = unique_test_dir(name);
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    TestDir(root)
}

fn remove_stale_test_dirs() {
    static CLEAN: std::sync::Once = std::sync::Once::new();
    CLEAN.call_once(|| {
        let Ok(entries) = fs::read_dir(std::env::temp_dir()) else {
            return;
        };
        let stale_before = SystemTime::now()
            .checked_sub(std::time::Duration::from_hours(1))
            .unwrap_or(UNIX_EPOCH);
        for entry in entries.flatten() {
            let path = entry.path();
            if !entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("cortexfs-ctx-"))
            {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.modified().is_ok_and(|modified| modified < stale_before) {
                let _ignored = fs::remove_dir_all(path);
            }
        }
    });
}

fn assert_path_matches(paths: &[&str], predicate: fn(&str) -> bool, expected: bool) {
    for path in paths {
        assert_eq!(predicate(path), expected, "{path}");
    }
}

macro_rules! assert_path_kind {
    ($path:expr, $classifier:expr, $expected:expr) => {
        assert_eq!($classifier($path), $expected, "{}", $path);
    };
}

fn assert_file_check_error_contains(root: &Path, path: &str, fragments: &[&str]) {
    let checked = file_check(root, path);
    assert!(checked.as_ref().is_err_and(|error| {
        error.code == 2 && fragments.iter().all(|fragment| error.message.contains(fragment))
    }), "{path}");
}

fn is_model_capability_path(path: &str) -> bool {
    parse_abi_path(path).model_control_file() == Some("cap")
}

fn is_model_driver_path(path: &str) -> bool {
    parse_abi_path(path).model_control_file() == Some("driver")
}

fn is_tool_schema_path(path: &str) -> bool {
    matches!(
        parse_abi_path(path),
        AbiPathKind::ObjectControl {
            class: ObjectClass::Tool,
            file: "schema",
            ..
        }
    )
}

fn is_shared_tool_schema_path(path: &str) -> bool {
    matches!(
        parse_abi_path(path),
        AbiPathKind::SharedToolControl { file: "schema", .. }
    )
}

fn is_shared_queue_root_path(path: &str) -> bool {
    matches!(parse_abi_path(path), AbiPathKind::SharedQueueRoot { .. })
}

fn agent_control_path_kind(path: &str) -> Option<AgentControlKind> {
    parse_abi_path(path).agent_control_kind()
}

fn is_durable_session_instance_path(path: &str) -> bool {
    parse_abi_path(path).is_session_instance()
}

fn session_index_path_kind(path: &str) -> Option<SessionIndexKind> {
    parse_abi_path(path).session_index_kind()
}

fn is_session_events_path(path: &str) -> bool {
    matches!(
        parse_abi_path(path),
        AbiPathKind::SessionFile {
            file: "events.jsonl",
            ..
        }
    )
}

fn is_session_messages_path(path: &str) -> bool {
    matches!(
        parse_abi_path(path),
        AbiPathKind::SessionFile {
            file: "messages.jsonl",
            ..
        }
    )
}

fn session_control_path_kind(path: &str) -> Option<SessionControlKind> {
    parse_abi_path(path).session_control_kind()
}

fn is_context_pack_path(path: &str) -> bool {
    parse_abi_path(path).is_context_pack()
}

fn context_jsonl_path_kind(path: &str) -> Option<ContextJsonlKind> {
    parse_abi_path(path).context_jsonl_kind()
}

fn executable_object_path(path: &str) -> Option<(ObjectClass, String)> {
    parse_abi_path(path)
        .executable_object()
        .map(|(class, name)| (class, name.into_owned()))
}

fn fixture_path(root: &Path, parts: &[&str]) -> PathBuf {
    let mut path = root.to_path_buf();
    path.extend(parts.iter().copied());
    path
}

macro_rules! cmd {
    ($($arg:literal),* $(,)?) => {
        parse_command(vec![$($arg.to_owned()),*])
    };
}

fn create_complete_session_layout(session: &Path) {
    let context = session.join("context");
    assert!(fs::create_dir_all(&context).is_ok());
    for file in SESSION_REQUIRED_FILES {
        write_text_file(&session.join(file), session_file_fixture_value(file));
    }
    for file in CONTEXT_REQUIRED_FILES {
        write_text_file(&context.join(file), "ok\n");
    }
    for dir in CONTEXT_REQUIRED_DIRS {
        assert!(fs::create_dir_all(context.join(dir)).is_ok());
    }
    let child = context.join("child").join("rev-1");
    assert!(fs::create_dir_all(child.join("artifact")).is_ok());
    for file in CHILD_RESULT_REQUIRED_FILES {
        write_text_file(&child.join(file), "ok\n");
    }
}

fn session_file_fixture_value(file: &str) -> &'static str {
    match file {
        "state" => "idle\n",
        "cwd" => "/work\n",
        "meta.json" => "{\"client\":\"ctx\",\"model\":\"openai/gpt-4o\",\"scope\":\"private\"}\n",
        _ => "ok\n",
    }
}

fn write_text_file(path: &Path, content: &str) {
    let Some(parent) = path.parent() else {
        return;
    };
    assert!(fs::create_dir_all(parent).is_ok());
    assert!(fs::write(path, content).is_ok());
}
