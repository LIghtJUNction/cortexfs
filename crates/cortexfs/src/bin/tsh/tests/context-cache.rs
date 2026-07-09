use super::*;
use crate::*;

#[test]
pub(crate) fn tool_context_evicts_oldest_unpinned_tool() {
    let mut context = ToolContext::new(1);
    assert!(context.insert(test_loaded_tool("a", false)).is_empty());
    let evicted = context.insert(test_loaded_tool("b", false));
    assert_eq!(
        evicted
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        vec!["a"]
    );
    assert!(context.tools.contains_key("b"));
}

#[test]
pub(crate) fn tool_context_touch_preserves_recently_used_tool() {
    let mut context = ToolContext::new(2);
    assert!(context.insert(test_loaded_tool("a", false)).is_empty());
    assert!(context.insert(test_loaded_tool("b", false)).is_empty());
    context.touch("a");

    let evicted = context.insert(test_loaded_tool("c", false));

    assert_eq!(
        evicted
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        vec!["b"]
    );
    assert!(context.tools.contains_key("a"));
    assert!(context.tools.contains_key("c"));
}

#[test]
pub(crate) fn tool_context_keeps_pinned_tools_over_limit() {
    let mut context = ToolContext::new(1);
    assert!(context.insert(test_loaded_tool("a", true)).is_empty());
    assert!(context.insert(test_loaded_tool("b", false)).is_empty());
    assert!(context.tools.contains_key("a"));
    assert!(context.tools.contains_key("b"));
}

#[test]
pub(crate) fn tool_context_reload_preserves_existing_pin() {
    let mut context = ToolContext::new(1);
    assert!(context.insert(test_loaded_tool("a", true)).is_empty());
    assert!(context.insert(test_loaded_tool("a", false)).is_empty());

    let evicted = context.insert(test_loaded_tool("b", false));

    assert!(evicted.is_empty());
    assert!(context.tools.get("a").is_some_and(|tool| tool.pinned));
    assert!(context.tools.contains_key("b"));
}

#[test]
pub(crate) fn tool_context_unload_removes_only_unpinned_tools() {
    let mut context = ToolContext::new(2);
    assert!(context.insert(test_loaded_tool("a", true)).is_empty());
    assert!(context.insert(test_loaded_tool("b", false)).is_empty());

    assert!(context.remove_unpinned("a").is_err());
    assert!(context.tools.contains_key("a"));
    let removed = context.remove_unpinned("b");
    assert!(matches!(removed, Ok(Some(ref tool)) if tool.name == "b"));
    assert!(!context.tools.contains_key("b"));
}

#[test]
pub(crate) fn tsh_cache_victim_prefers_lowest_frequency_then_oldest_use() {
    assert_eq!(
        wtinylfu_victim_path([
            (Path::new("/ctx/tool/a"), 4, 10),
            (Path::new("/ctx/tool/b"), 1, 20),
            (Path::new("/ctx/tool/c"), 1, 5),
        ]),
        Some(PathBuf::from("/ctx/tool/c"))
    );
    assert_eq!(wtinylfu_victim_path([]), None);
}

#[test]
pub(crate) fn tsh_cache_admission_keeps_frequent_main_entries() {
    assert!(tiny_lfu_admits(5, 2, 1, 10));
    assert!(!tiny_lfu_admits(1, 3, 10, 1));
    assert!(tiny_lfu_admits(2, 2, 20, 10));
    assert!(!tiny_lfu_admits(2, 2, 10, 20));
}

#[test]
pub(crate) fn tsh_cache_keeps_current_load_and_respects_pins() {
    let a = PathBuf::from("/ctx/tool/a");
    let b = PathBuf::from("/ctx/tool/b");
    let c = PathBuf::from("/ctx/tool/c");
    let mut cache = DynamicToolCache::with_window_percent(1, 1);

    cache.pin_path(&a);
    cache.load_path(&b);
    cache.load_path(&c);

    assert!(cache.contains_path(&a));
    assert!(cache.is_pinned_path(&a));
    assert!(cache.contains_path(&c));
    assert!(cache.unpinned_len() <= 1);
    assert!(cache.unpin_path(&a));
}

#[test]
pub(crate) fn load_tool_context_reads_metadata_without_dynamic_load() {
    let root =
        std::env::temp_dir().join(format!("cortexfs-tsh-load-context-{}", std::process::id()));
    let tool_dir = root.join("tool");
    let control_dir = root.join("tool").join("meta.d");
    assert!(fs::create_dir_all(&control_dir).is_ok());
    let tool = tool_dir.join("meta");
    assert!(fs::write(&tool, "#!/bin/sh\nexit 0\n").is_ok());
    assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());
    assert!(fs::write(control_dir.join("description"), "metadata only\n").is_ok());

    let loaded = load_tool_context(&root, "meta", true);
    assert!(loaded.is_ok(), "load metadata: {loaded:?}");
    let Ok(loaded) = loaded else {
        return;
    };

    assert_eq!(loaded.name, "meta");
    assert_eq!(loaded.description, "metadata only");
    assert!(loaded.pinned);
    assert!(!loaded.dynamic_resident);

    let _ignored = fs::remove_dir_all(root);
}

#[test]
pub(crate) fn load_tool_context_ignores_symlink_metadata() {
    let root = std::env::temp_dir().join(format!(
        "cortexfs-tsh-load-context-symlink-{}",
        std::process::id()
    ));
    let tool_dir = root.join("tool");
    let control_dir = root.join("tool").join("meta.d");
    assert!(fs::create_dir_all(&control_dir).is_ok());
    let tool = tool_dir.join("meta");
    assert!(fs::write(&tool, "#!/bin/sh\nexit 0\n").is_ok());
    assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());
    let outside = root.join("outside-description");
    assert!(fs::write(&outside, "attacker metadata\n").is_ok());
    assert!(symlink(&outside, control_dir.join("description")).is_ok());

    let loaded = load_tool_context(&root, "meta", true);
    assert!(loaded.is_ok(), "load metadata: {loaded:?}");
    let Ok(loaded) = loaded else {
        return;
    };

    assert_eq!(loaded.description, "");
    let _ignored = fs::remove_dir_all(root);
}

#[test]
pub(crate) fn load_tool_context_ignores_symlink_intermediate_metadata_dir() {
    let root = std::env::temp_dir().join(format!(
        "cortexfs-tsh-load-context-symlink-intermediate-{}",
        std::process::id()
    ));
    let outside = root.join("outside");
    let tool_dir = root.join("tool");
    assert!(fs::create_dir_all(&tool_dir).is_ok());
    assert!(fs::create_dir_all(outside.join("meta.d")).is_ok());
    assert!(fs::write(outside.join("meta.d/description"), "attacker metadata\n").is_ok());
    let tool = tool_dir.join("meta");
    assert!(fs::write(&tool, "#!/bin/sh\nexit 0\n").is_ok());
    assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());
    assert!(symlink(&outside, tool_dir.join("meta.d")).is_ok());

    let loaded = load_tool_context(&root, "meta", true);
    assert!(loaded.is_ok(), "load metadata: {loaded:?}");
    let Ok(loaded) = loaded else {
        return;
    };

    assert_eq!(loaded.description, "");
    let _ignored = fs::remove_dir_all(root);
}

#[test]
pub(crate) fn terminal_safe_text_escapes_control_sequences() {
    assert_eq!(
        terminal_safe_text("desc-prefix-\u{1b}[31mRED\u{1b}[0m\tend"),
        "desc-prefix-\\u{1b}[31mRED\\u{1b}[0m\\tend"
    );
}

#[test]
pub(crate) fn schema_help_escapes_decoded_control_sequences() {
    let mut text = String::new();
    append_schema_help(
        &mut text,
        r#"{
                "title":"schema-title-\u001b[35mMAGENTA\u001b[0m",
                "description":"schema-description-\u001b]52;c;AAAA\u0007",
                "required":["safe","bad\u001b[0m"]
            }"#,
    );

    assert!(!text.contains('\u{1b}'));
    assert!(!text.contains('\u{7}'));
    assert!(text.contains(r"schema-title-\u{1b}[35mMAGENTA\u{1b}[0m"));
    assert!(text.contains(r"schema-description-\u{1b}]52;c;AAAA\u{7}"));
    assert!(text.contains(r"required: safe bad\u{1b}[0m"));
}

pub(crate) fn test_loaded_tool(name: &str, pinned: bool) -> LoadedTool {
    LoadedTool {
        name: name.to_owned(),
        path: PathBuf::from(format!("/ctx/tool/{name}")),
        description: String::new(),
        schema: None,
        dynamic_resident: false,
        pinned,
        last_used: 0,
    }
}
