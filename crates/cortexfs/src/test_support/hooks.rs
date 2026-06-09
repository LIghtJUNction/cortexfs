use std::path::PathBuf;

use crate::CortexFs;

#[test]
fn hook_runs_structured_job_when_request_is_written() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let hook_root = fs
        .tree
        .path_inode(crate::HOOK_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let hook = runtime.create_virtual_dir(hook_root, "daily-translate")?;
    let spec = runtime
        .lookup_child(hook, "spec")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let req = runtime
        .lookup_child(hook, "req")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;

    runtime.write(
        spec,
        0,
        b"kind=job\njob=translate.zh\nfrom=en\nto=zh\nfields=text,from,to,input\n",
    )?;
    runtime.write(req, 0, b"good morning\n")?;

    assert_eq!(
        runtime
            .lookup_child(hook, "out.json")
            .and_then(crate::Node::content),
        Some("{\"text\":\"早上好\",\"from\":\"en\",\"to\":\"zh\",\"input\":\"good morning\"}\n")
    );
    assert_eq!(
        runtime
            .lookup_child(hook, "status")
            .and_then(crate::Node::content),
        Some("done\n")
    );
    assert_eq!(
        runtime
            .lookup_child(hook, "last")
            .and_then(crate::Node::content),
        Some("done\n")
    );
    assert!(
        runtime
            .lookup_child(hook, "log.jsonl")
            .and_then(crate::Node::content)
            .is_some_and(|log| log.contains("\"event\":\"drained\""))
    );
    drop(runtime);
    Ok(())
}

#[test]
fn hook_config_is_written_through_and_reloaded() -> fuse3::Result<()> {
    let config_dir = temp_chan_config_dir("persisted-hook");
    let hook_dir = config_dir.parent().ok_or(libc::EINVAL)?.join("hook.d");
    let fs = CortexFs::new_with_chan_config_dir(config_dir.clone());
    let hook_root = fs
        .tree
        .path_inode(crate::HOOK_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    {
        let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let hook = runtime.create_virtual_dir(hook_root, "daily-translate")?;
        let trigger = runtime
            .lookup_child(hook, "trigger")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let spec = runtime
            .lookup_child(hook, "spec")
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        runtime.write(trigger, 0, b"systemd.timer\n")?;
        runtime.write(
            spec,
            0,
            b"kind=job\njob=translate.zh\nfrom=en\nto=zh\nfields=text,from,to,input\n",
        )?;
    }

    let config_path = hook_dir.join("daily-translate.conf");
    let persisted = std::fs::read_to_string(&config_path).map_err(|_error| libc::EIO)?;
    assert!(persisted.contains("trigger=systemd.timer\n"));
    assert!(persisted.contains("spec.kind=job\n"));
    assert!(persisted.contains("spec.job=translate.zh\n"));

    let fs = CortexFs::new_with_chan_config_dir(config_dir.clone());
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let hook = runtime
        .lookup_child(hook_root, "daily-translate")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert_eq!(
        runtime
            .lookup_child(hook, "trigger")
            .and_then(crate::Node::content),
        Some("systemd.timer\n")
    );
    assert_eq!(
        runtime
            .lookup_child(hook_root, "list")
            .and_then(crate::Node::content),
        Some("daily-translate\n")
    );
    drop(runtime);
    let _result = std::fs::remove_dir_all(
        config_dir
            .parent()
            .ok_or(libc::EINVAL)?
            .parent()
            .ok_or(libc::EINVAL)?,
    );
    Ok(())
}

fn temp_chan_config_dir(name: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir()
        .join("cortexfs-tests")
        .join(format!("{name}-{}-{stamp}", std::process::id()))
        .join("chan.d")
}
