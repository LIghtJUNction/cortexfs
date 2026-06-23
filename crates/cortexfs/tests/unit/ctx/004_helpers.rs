fn unique_test_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!(
        "cortexfs-ctx-{name}-{}-{nanos}",
        std::process::id()
    ))
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
