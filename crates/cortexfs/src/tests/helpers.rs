pub(super) fn unique_test_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!("cortexfs-{name}-{}-{nanos}", std::process::id()))
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
        // ponytail: best-effort test cleanup; stale startup cleanup covers killed test processes.
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
    TestDir(root)
}

pub(super) fn remove_stale_test_dirs() {
    static CLEAN: std::sync::Once = std::sync::Once::new();
    CLEAN.call_once(|| {
        let Ok(entries) = fs::read_dir(std::env::temp_dir()) else {
            return;
        };
        let stale_before = SystemTime::now()
            .checked_sub(Duration::from_hours(1))
            .unwrap_or(SystemTime::UNIX_EPOCH);
        for entry in entries.flatten() {
            let path = entry.path();
            if !entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("cortexfs-"))
            {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata
                .modified()
                .is_ok_and(|modified| modified < stale_before)
            {
                let _ignored = fs::remove_dir_all(path);
            }
        }
    });
}

pub(super) fn assert_abi_class(path: &str, expected: &str) {
    assert_eq!(classify_abi_path(path), expected, "{path}");
}

pub(super) fn reference_tree(name: &str) -> TestDir {
    let root = clean_test_dir(name);
    let result = ensure_v1_reference_tree(&root);
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

pub(super) fn create_shared_queue_layout(queue: &Path) {
    for dir in SHARED_QUEUE_REQUIRED_DIRS {
        assert!(fs::create_dir_all(queue.join(dir)).is_ok());
    }
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
        } else if class == ObjectClass::Model && *file == "fallback" {
            ""
        } else if class == ObjectClass::Tool && *file == "schema" {
            "{\"type\":\"object\"}"
        } else if class == ObjectClass::Agent {
            agent_control_fixture_value(file)
        } else {
            "ok"
        };
        write_text_file(&control_dir.join(file), &format!("{value}\n"));
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
        "label" => "user_u:agent_r:coder_t:s0",
        "iso" => "shared",
        "parent" | "pid" => "",
        "life" => "owned",
        "root" => "/ctx/home/1000/agent/coder/root",
        "cwd" => "/work",
        "env" => "CTX_ROOT=/ctx",
        "path" => "/ctx/tool:/ctx/home/1000/tool",
        "mount" => "/ctx\t/ctx\tro\trbind,nosuid,nodev",
        "model" => "debug/echo",
        "policy" => "allow coder_t model:debug/echo use",
        "status" => "idle",
        "log" => "agent/coder/log",
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
