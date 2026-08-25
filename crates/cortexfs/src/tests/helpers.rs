pub(super) fn unique_test_dir(name: &str) -> PathBuf {
    use std::hash::{DefaultHasher, Hash, Hasher};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    // The owner pid segment must stay decimal: `test_temp_owner_pid` parses it back to decide
    // whether a directory still has a live owner, and a hex pid would resolve to a different
    // process whose absence makes the cleanup delete the running test's own fixtures.
    std::env::temp_dir().join(format!(
        "cortexfs-{:016x}-{}-{nanos:x}",
        hasher.finish(),
        std::process::id()
    ))
}

const TEST_TEMP_PREFIX: &str = "cortexfs-";
const TEST_TEMP_MAX_BYTES: u64 = 512 * 1024 * 1024;
const TEST_TEMP_MAX_ENTRIES: usize = 256;
const TEST_TEMP_MIN_FREE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug)]
struct TestTempEntry {
    path: PathBuf,
    bytes: u64,
    owner_alive: bool,
    modified: SystemTime,
}

#[test]
fn test_directory_name_keeps_ci_socket_paths_short() {
    use std::os::unix::ffi::OsStrExt;

    let directory = unique_test_dir(&"long-test-name-".repeat(32));
    let name = directory.file_name().unwrap_or_default();
    let socket = Path::new("/tmp").join(name).join("agent/architect.sock");
    assert!(socket.as_os_str().as_bytes().len() < 108);
}

pub(super) struct TestDir(PathBuf);

impl TestDir {
    pub(super) fn as_path(&self) -> &Path {
        &self.0
    }
}

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

macro_rules! ok {
    ($result:expr) => {{
        let result = $result;
        assert!(result.is_ok());
        let Ok(value) = result else { return };
        value
    }};
}

pub(super) fn clean_test_dir(name: &str) -> TestDir {
    remove_stale_test_dirs();
    let root = unique_test_dir(name);
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert_test_temp_budget();
    TestDir(root)
}

#[test]
fn live_library_test_directory_is_kept_for_its_own_owner() {
    let root = clean_test_dir("live-owner-directory-survives-cleanup");
    assert!(fs::create_dir_all(root.join("child")).is_ok());
    assert_eq!(
        test_temp_owner_pid(root.as_path()),
        Some(std::process::id())
    );

    remove_stale_test_dirs();

    assert!(root.join("child").exists());
}

#[test]
fn stale_library_test_directory_from_dead_process_is_reclaimed() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let path =
        std::env::temp_dir().join(format!("{TEST_TEMP_PREFIX}recovery-{}-{nonce:x}", u32::MAX));
    assert!(fs::create_dir_all(&path).is_ok());

    remove_stale_test_dirs();

    assert!(!path.exists());
}

pub(super) fn remove_stale_test_dirs() {
    static CLEANUP_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    let lock = CLEANUP_LOCK.get_or_init(|| std::sync::Mutex::new(()));
    let _guard = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let entries = test_temp_entries();
    // A test process that was killed never runs `Drop`; its PID is the ownership lease for the
    // directory. Reclaiming only entries whose owner is gone avoids deleting a long-running test.
    for entry in &entries {
        if !entry.owner_alive {
            remove_test_temp_entry(&entry.path);
        }
    }

    // Keep a second bound for any stale entries that could not be removed immediately. The final
    // budget assertion fails closed instead of allowing another test to keep writing indefinitely.
    let entries = test_temp_entries();
    let (mut entry_count, mut bytes) = test_temp_usage(&entries);
    let mut removable: Vec<&TestTempEntry> =
        entries.iter().filter(|entry| !entry.owner_alive).collect();
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
        .filter_map(|entry| {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_str()?;
            if !name.starts_with(TEST_TEMP_PREFIX) || test_temp_owner_pid(&path).is_none() {
                return None;
            }
            let metadata = fs::symlink_metadata(&path).ok()?;
            Some(TestTempEntry {
                modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
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
        .map(|stats| {
            stats
                .blocks_available()
                .saturating_mul(stats.fragment_size())
        })
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

pub(super) fn assert_abi_class(path: &str, expected: &str) {
    assert_eq!(classify_abi_path(path), expected, "{path}");
}

pub(super) fn reference_tree(name: &str) -> TestDir {
    let root = clean_test_dir(name);
    let result = ensure_reference_tree(&root);
    assert!(result.is_ok(), "{result:?}");
    root
}

pub(super) fn write_fixture_file(path: &Path, mode: u32) {
    if let Some(parent) = path.parent() {
        assert!(fs::create_dir_all(parent).is_ok());
    }
    assert!(fs::write(path, "#!/bin/sh\n").is_ok());
    set_file_mode(path, mode);
}

pub(super) fn set_file_mode(path: &Path, mode: u32) {
    let permissions = fs::metadata(path).map(|metadata| metadata.permissions());
    let mut permissions = ok!(permissions);
    permissions.set_mode(mode);
    assert!(fs::set_permissions(path, permissions).is_ok());
}

pub(super) fn unix_identity_for(path: &Path) -> std::io::Result<AgentUnixIdentity> {
    fs::metadata(path).map(|metadata| AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []))
}

pub(super) fn create_complete_session_layout(session: &Path) {
    let context = session.join("context");
    assert!(fs::create_dir_all(context.join("pinned")).is_ok());
    assert!(fs::create_dir_all(context.join("swap")).is_ok());
    assert!(fs::create_dir_all(context.join("dedup")).is_ok());
    assert!(fs::create_dir_all(context.join("child").join("rev-1").join("artifact")).is_ok());

    for file in SESSION_REQUIRED_FILES {
        write_text_file(&session.join(file), session_file_fixture_value(file));
    }
    for file in super::CONTEXT_REQUIRED_FILES {
        write_text_file(&context.join(file), "ok\n");
    }
    for file in super::CHILD_RESULT_REQUIRED_FILES {
        write_text_file(&context.join("child").join("rev-1").join(file), "ok\n");
    }
}

pub(super) fn session_file_fixture_value(file: &str) -> &'static str {
    match file {
        "state" => "idle\n",
        "cwd" => "/work\n",
        "meta.json" => "{\"client\":\"ctx\",\"model\":\"debug/echo\",\"scope\":\"private\"}\n",
        "messages.jsonl" | "events.jsonl" => "",
        _ => "ok\n",
    }
}

pub(super) fn write_text_file(path: &Path, content: &str) {
    let Some(parent) = path.parent() else {
        return;
    };
    assert!(fs::create_dir_all(parent).is_ok());
    assert!(fs::write(path, content).is_ok());
}

pub(super) fn assert_file_text(path: &Path, expected: &str) {
    assert!(
        fs::read_to_string(path)
            .as_ref()
            .is_ok_and(|content| content == expected),
        "{}",
        path.display()
    );
}

pub(super) fn assert_child_test_ran(output: &std::process::Output) {
    assert!(output.status.success(), "child failed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("1 passed; 0 failed"),
        "child did not run exactly one test: {stdout}"
    );
}

pub(super) fn contains_arg_pair(args: &[String], first: &str, second: &str) -> bool {
    args.windows(2).any(|window| {
        window.first().is_some_and(|value| value == first)
            && window.get(1).is_some_and(|value| value == second)
    })
}

pub(super) fn contains_arg_triplet(
    args: &[String],
    first: &str,
    second: &str,
    third: &str,
) -> bool {
    args.windows(3).any(|window| {
        window.first().is_some_and(|value| value == first)
            && window.get(1).is_some_and(|value| value == second)
            && window.get(2).is_some_and(|value| value == third)
    })
}

pub(super) fn fixture_path(root: &Path, parts: &[&str]) -> PathBuf {
    let mut path = root.to_path_buf();
    path.extend(parts.iter().copied());
    path
}

pub(super) fn ctx_home(root: &Path) -> PathBuf {
    fixture_path(root, &["home", "1000"])
}

pub(super) fn agent_home(root: &Path, agent: &str) -> PathBuf {
    fixture_path(root, &["home", "1000", "agent", agent])
}

pub(super) fn agent_session_root(root: &Path, agent: &str) -> PathBuf {
    agent_home(root, agent).join("session")
}

pub(super) fn mount_table_for_target(target: &Path, mode: &str, options: &str) -> MountTable {
    mount_table_for_source_target(&target.display().to_string(), target, mode, options)
}

pub(super) fn mount_table_for_source_target(
    source: &str,
    target: &Path,
    mode: &str,
    options: &str,
) -> MountTable {
    let line = format!(
        "{source}\t{target}\t{mode}\t{options}\n",
        target = target.display()
    );
    let parsed = MountTable::parse(&line);
    assert!(parsed.is_ok());
    parsed.unwrap_or_default()
}

pub(super) fn allow_tool_policy(subject: &str, tool: &str) -> PolicyV0 {
    let parsed = PolicyV0::parse(&format!("allow {subject} tool:{tool} execute\n"));
    assert!(parsed.is_ok());
    parsed.unwrap_or_default()
}

pub(super) fn allow_shared_policy(subject: &str, shared: &str, access: SharedAccess) -> PolicyV0 {
    let permission = match access {
        SharedAccess::Read => "read",
        SharedAccess::Write => "write",
    };
    let parsed = PolicyV0::parse(&format!("allow {subject} shared:{shared} {permission}\n"));
    assert!(parsed.is_ok());
    parsed.unwrap_or_default()
}

pub(super) fn policy_with_rules(rules: impl IntoIterator<Item = &'static str>) -> PolicyV0 {
    let content = rules.into_iter().collect::<Vec<_>>().join("\n") + "\n";
    let parsed = PolicyV0::parse(&content);
    assert!(parsed.is_ok());
    parsed.unwrap_or_default()
}

pub(super) fn env_value<'a>(env: &'a [(String, String)], key: &str) -> Option<&'a str> {
    env.iter()
        .find_map(|entry| (entry.0 == key).then_some(entry.1.as_str()))
}

pub(super) fn create_complete_object_layout(
    root: &Path,
    class: ObjectClass,
    name: &str,
    model_session: &str,
) {
    let class_dir = root.join(class.as_str());
    assert!(fs::create_dir_all(&class_dir).is_ok());
    write_fixture_file(&class_dir.join(name), 0o755);
    let control_dir = class_dir.join(format!("{name}.d"));
    assert!(fs::create_dir_all(&control_dir).is_ok());
    for file in object_control_files(class) {
        let value = if class == ObjectClass::Model && *file == "session" {
            model_session
        } else if class == ObjectClass::Model && *file == "cap" {
            "chat"
        } else if class == ObjectClass::Model && *file == "effort" {
            "auto"
        } else if class == ObjectClass::Model
            && matches!(*file, "limit" | "recommended" | "compact")
        {
            "unknown"
        } else if class == ObjectClass::Model && *file == "driver" {
            "default=openai-chat"
        } else if class == ObjectClass::Tool && *file == "schema" {
            "{\"type\":\"object\"}"
        } else if class == ObjectClass::Agent {
            agent_control_fixture_value(file)
        } else {
            "ok"
        };
        write_text_file(&control_dir.join(file), &format!("{value}\n"));
    }
    if class == ObjectClass::Agent {
        let model_control = root.join("model/debug/echo.d");
        assert!(fs::create_dir_all(&model_control).is_ok());
        write_text_file(&model_control.join("limit"), "unknown\n");
        let worker_control = root.join("model/api.test/gpt-5.6.d");
        assert!(fs::create_dir_all(&worker_control).is_ok());
        write_text_file(&worker_control.join("limit"), "unknown\n");
        assert!(fs::create_dir_all(root.join("model")).is_ok());
        for alias in MODEL_ALIASES {
            let path = root.join("model").join(alias);
            if !path.exists() {
                assert!(std::os::unix::fs::symlink("/ctx/model/debug/echo", path).is_ok());
            }
        }
    }
    if class != ObjectClass::Model {
        let hook_dir = control_dir.join(OBJECT_HOOK_DIR);
        assert!(fs::create_dir_all(&hook_dir).is_ok());
        for phase in OBJECT_HOOK_PHASE_DIRS {
            assert!(fs::create_dir_all(hook_dir.join(phase)).is_ok());
        }
    }
}

pub(super) fn agent_control_fixture_value(file: &str) -> &'static str {
    match file {
        "owner" | "uid" => "1000",
        "gid" => "100",
        "groups" => "10\n20",
        "perm" => "rwx",
        "label" => "user_u:agent_r:executor_t:s0",
        "iso" => "shared",
        "parent" | "pid" => "",
        "life" => "owned",
        "root" => "/ctx/home/1000/agent/executor/root",
        "cwd" => "/work",
        "env" => "CTX_ROOT=/ctx",
        "path" => "/ctx/tool:/ctx/home/1000/tool",
        "mount" => "/ctx\t/ctx\tro\trbind,nosuid,nodev",
        "model" => "debug/echo",
        "abi" => "sdk-envelope-v1",
        "window" | "compact" => "auto",
        "policy" => "allow executor_t model:debug/echo use",
        "status" => "idle",
        "log" => "agent/executor/log",
        "meta.json" => "{}",
        _ => "ok",
    }
}

pub(super) fn object_control_files(class: ObjectClass) -> &'static [&'static str] {
    match class {
        ObjectClass::Model => MODEL_CONTROL_FILES,
        ObjectClass::Agent => AGENT_CONTROL_FILES,
        ObjectClass::Tool => TOOL_CONTROL_FILES,
    }
}

pub(super) fn bind_socket(path: &Path) -> Option<UnixListener> {
    let parent = path.parent()?;
    assert!(fs::create_dir_all(parent).is_ok());
    UnixListener::bind(path).ok()
}
use super::*;
use crate::MODEL_ALIASES;
