fn unique_test_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let short_name = name.chars().take(24).collect::<String>();
    std::env::temp_dir().join(format!(
        "cortexfs-ctx-{short_name}-{}-{nanos}",
        std::process::id()
    ))
}

const TEST_TEMP_PREFIX: &str = "cortexfs-ctx-";
const TEST_TEMP_MAX_BYTES: u64 = 512 * 1024 * 1024;
const TEST_TEMP_MAX_ENTRIES: usize = 256;
const TEST_TEMP_MIN_FREE_BYTES: u64 = 256 * 1024 * 1024;
const TEST_TEMP_STALE_AFTER: std::time::Duration = std::time::Duration::from_mins(30);

#[derive(Debug)]
struct TestTempEntry {
    path: PathBuf,
    modified: SystemTime,
    bytes: u64,
    owner_alive: bool,
}

struct TestDir(PathBuf);

const NEUTRAL_FIXTURE_MODEL: &str = "api.test/gpt-5.6";

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
        // These are disposable test artifacts: unlink them directly, never through a desktop
        // trash implementation that would turn a cleanup into another unbounded data store.
        let _ignored = fs::remove_dir_all(&self.0);
    }
}

fn clean_test_dir(name: &str) -> TestDir {
    remove_stale_test_dirs();
    let root = unique_test_dir(name);
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert_test_temp_budget();
    TestDir(root)
}

#[test]
fn stale_ctx_test_directory_from_dead_process_is_reclaimed() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let path = std::env::temp_dir().join(format!(
        "{TEST_TEMP_PREFIX}recovery-{}-{nonce}",
        u32::MAX
    ));
    assert!(fs::create_dir_all(&path).is_ok());

    remove_stale_test_dirs();

    assert!(!path.exists());
}

fn remove_stale_test_dirs() {
    static CLEANUP_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> =
        std::sync::OnceLock::new();
    let lock = CLEANUP_LOCK.get_or_init(|| std::sync::Mutex::new(()));
    let _guard = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let stale_before = SystemTime::now()
        .checked_sub(TEST_TEMP_STALE_AFTER)
        .unwrap_or(UNIX_EPOCH);
    let mut entries = test_temp_entries();

    // A test process that was killed never runs `Drop`; its PID is the ownership lease for the
    // directory. Reclaiming only entries whose owner is gone avoids deleting a long-running test.
    for entry in &entries {
        if !entry.owner_alive
            && (entry.modified < stale_before || test_temp_owner_pid(&entry.path).is_some())
        {
            remove_test_temp_entry(&entry.path);
        }
    }

    // Keep a second bound for malformed or very old entries that cannot be associated with a
    // live owner. This is deliberately best-effort; the final budget assertion fails closed.
    entries = test_temp_entries();
    let (mut entry_count, mut bytes) = test_temp_usage(&entries);
    let mut removable: Vec<&TestTempEntry> = entries
        .iter()
        .filter(|entry| !entry.owner_alive)
        .collect();
    removable.sort_by_key(|entry| entry.modified);
    for entry in removable {
        if entry_count <= TEST_TEMP_MAX_ENTRIES && bytes <= TEST_TEMP_MAX_BYTES {
            break;
        }
        if remove_test_temp_entry(&entry.path) {
            entry_count = entry_count.saturating_sub(1);
            bytes = bytes.saturating_sub(entry.bytes);
        }
    }
}

fn test_temp_entries() -> Vec<TestTempEntry> {
    let Ok(entries) = fs::read_dir(std::env::temp_dir()) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(TEST_TEMP_PREFIX))
        })
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).ok()?;
            Some(TestTempEntry {
                modified: metadata.modified().unwrap_or(UNIX_EPOCH),
                bytes: test_temp_size(&path),
                owner_alive: test_temp_owner_pid(&path).is_some_and(test_process_alive),
                path,
            })
        })
        .collect()
}

fn test_temp_owner_pid(path: &Path) -> Option<u32> {
    let mut parts = path.file_name()?.to_str()?.rsplit('-');
    let _nonce = parts.next()?;
    parts.next()?.parse().ok()
}

fn test_process_alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).is_dir()
}

fn test_temp_size(path: &Path) -> u64 {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    if !metadata.file_type().is_dir() {
        return metadata.len();
    }
    fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| test_temp_size(&entry.path()))
        .sum()
}

fn remove_test_temp_entry(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if metadata.file_type().is_dir() {
        fs::remove_dir_all(path).is_ok()
    } else {
        fs::remove_file(path).is_ok()
    }
}

fn test_temp_usage(entries: &[TestTempEntry]) -> (usize, u64) {
    entries.iter().fold((0, 0), |(count, bytes), entry| {
        (count.saturating_add(1), bytes.saturating_add(entry.bytes))
    })
}

fn test_temp_free_bytes() -> Option<u64> {
    nix::sys::statvfs::statvfs(&std::env::temp_dir())
        .ok()
        .map(|stats| stats.blocks_available().saturating_mul(stats.fragment_size()))
}

fn assert_test_temp_budget() {
    let entries = test_temp_entries();
    let (entry_count, bytes) = test_temp_usage(&entries);
    let max_mib = TEST_TEMP_MAX_BYTES / (1024 * 1024);
    let used_mib = bytes / (1024 * 1024);
    let min_free_mib = TEST_TEMP_MIN_FREE_BYTES / (1024 * 1024);
    assert!(
        entry_count <= TEST_TEMP_MAX_ENTRIES,
        "cortexfs test temp directory count exceeded {TEST_TEMP_MAX_ENTRIES} (found {entry_count})"
    );
    assert!(
        bytes <= TEST_TEMP_MAX_BYTES,
        "cortexfs test temp usage exceeded {max_mib} MiB (found {used_mib} MiB)"
    );
    assert!(
        test_temp_free_bytes().is_some_and(|free| free >= TEST_TEMP_MIN_FREE_BYTES),
        "refusing to create cortexfs test data: /tmp has less than {min_free_mib} MiB free"
    );
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

fn create_complete_agent_control(root: &Path, name: &str) {
    let control = fixture_path(root, &["agent", &format!("{name}.d")]);
    assert!(fs::create_dir_all(&control).is_ok());
    for file in AGENT_CONTROL_FILES {
        write_text_file(
            &control.join(file),
            &format!("{}\n", complete_agent_control_value(file, name)),
        );
    }
}

fn ensure_runtime_model_fixture(root: &Path) {
    let providers = root.join("providers.d");
    let cache = root.join("provider-models");
    assert!(fs::create_dir_all(&providers).is_ok());
    assert!(fs::create_dir_all(&cache).is_ok());
    let debug = install_executable_object_wrapper(
        root,
        ObjectClass::Model,
        "debug/echo",
        "/ctx/bin/cortexfs-object-runner",
        &[
            ("id", "debug/echo"),
            ("driver", "default=debug\nexec=debug\nagent=debug"),
            ("cap", "chat\nstream"),
            ("effort", "auto"),
            ("limit", "unknown"),
            ("default", ""),
            ("session", "none"),
            ("status", "idle"),
            ("log", ""),
        ],
    );
    assert!(debug.is_ok(), "{debug:?}");
    for alias in cortexfs::MODEL_ALIASES {
        let path = root.join("model").join(alias);
        if fs::symlink_metadata(&path).is_err() {
            assert!(std::os::unix::fs::symlink("/ctx/model/debug/echo", path).is_ok());
        }
    }
    for model in [DEFAULT_WORKER_MODEL, NEUTRAL_FIXTURE_MODEL] {
        let model = Path::new(model);
        let limit = root
            .join("model")
            .join(model.parent().unwrap_or_else(|| Path::new("")))
            .join(format!(
                "{}.d",
                model
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
            ))
            .join("limit");
        write_text_file(&limit, "unknown\n");
    }
}

fn enable_dynamic_worker_fixture(root: &Path) {
    let installed = install_executable_object_wrapper(
        root,
        ObjectClass::Agent,
        "worker",
        "/bin/false",
        &[
            ("owner", "1000"),
            ("uid", "1000"),
            ("gid", "1000"),
            ("groups", "1000"),
            ("label", "user_u:agent_r:worker_t:s0"),
            ("iso", "shared"),
            ("parent", "agent:coder"),
            ("life", "temp"),
            ("root", "/ctx/home/1000/agent/worker/root"),
            ("cwd", "/workspace"),
            ("env", "CTX_ROOT=/ctx"),
            ("path", "/ctx/tool:/ctx/home/1000/tool"),
            (
                "mount",
                "/ctx\t/ctx\tro\trbind,nosuid,nodev\n/ctx/home/1000/agent/worker\t/home/agent\trw\trbind,nosuid,nodev\n",
            ),
            ("model", DEFAULT_WORKER_MODEL),
            ("system.md", "You are CortexFS agent `worker`."),
            ("prompt.template.md", "{{agent_instructions}}\n"),
            (
                "policy",
                "allow worker_t model:openai/gpt-5.6 use\nallow worker_t tool:tsh execute\nallow worker_t tool:fs.read execute\nallow worker_t tool:fs.write execute\nallow worker_t tool:shell.exec execute\n",
            ),
            ("status", "idle"),
            ("pid", ""),
            ("log", ""),
            ("meta.json", "{}"),
        ],
    );
    assert!(installed.is_ok(), "{installed:?}");

    let coder_policy = root.join("agent/coder.d/policy");
    let mut policy = fs::read_to_string(&coder_policy).unwrap_or_default();
    policy.push_str(
        "allow coder_t agent:worker create\nallow coder_t agent:worker start\nallow coder_t agent:worker stop\nallow coder_t agent:worker read\n",
    );
    write_text_file(&coder_policy, &policy);
}

fn complete_agent_control_value(file: &str, name: &str) -> String {
    let subject = format!("{name}_t");
    match file {
        "owner" | "uid" => "1000".to_owned(),
        "gid" => "100".to_owned(),
        "groups" => "10\n20".to_owned(),
        "perm" => "rwx".to_owned(),
        "label" => format!("user_u:agent_r:{subject}:s0"),
        "iso" => "shared".to_owned(),
        "parent" | "pid" | "log" | "system.md" | "prompt.template.md" => String::new(),
        "life" => "owned".to_owned(),
        "root" => format!("/ctx/home/1000/agent/{name}/root"),
        "cwd" => "/workspace".to_owned(),
        "env" => "LD_PRELOAD=/tmp/ignored".to_owned(),
        "path" => "/ctx/tool:/ctx/home/1000/tool".to_owned(),
        "mount" => "/ctx\t/ctx\tro\trbind,nosuid,nodev".to_owned(),
        "model" => NEUTRAL_FIXTURE_MODEL.to_owned(),
        "abi" => "sdk-envelope-v1".to_owned(),
        "window" | "compact" => "auto".to_owned(),
        "policy" => format!("allow {subject} model:{NEUTRAL_FIXTURE_MODEL} use"),
        "status" => "idle".to_owned(),
        "meta.json" => "{}".to_owned(),
        _ => "ok".to_owned(),
    }
}

fn session_file_fixture_value(file: &str) -> &'static str {
    match file {
        "state" => "idle\n",
        "cwd" => "/work\n",
        "meta.json" => "{\"client\":\"ctx\",\"model\":\"openai/gpt-5.6\",\"scope\":\"private\"}\n",
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
