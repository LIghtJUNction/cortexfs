use crate::CortexFs;

#[test]
fn structured_job_outputs_json_from_spec_and_request_files() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let job_root = fs
        .tree
        .path_inode(&["home", "1000", "job"])
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let job = runtime.create_virtual_dir(job_root, "translate.zh")?;
    let spec = runtime
        .lookup_child(job, "spec")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let req = runtime
        .lookup_child(job, "req")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;

    runtime.write(
        spec,
        0,
        b"kind=translate\nfrom=en\nto=zh\nout=json\nfields=text,from,to,input\n",
    )?;
    runtime.write(req, 0, b"hello world\n")?;

    assert_eq!(
        runtime
            .lookup_child(job, "out.json")
            .and_then(crate::Node::content),
        Some("{\"text\":\"你好，世界\",\"from\":\"en\",\"to\":\"zh\",\"input\":\"hello world\"}\n")
    );
    assert_eq!(
        runtime
            .lookup_child(job, "status")
            .and_then(crate::Node::content),
        Some("done\n")
    );
    assert_eq!(
        runtime
            .lookup_child(job_root, "list")
            .and_then(crate::Node::content),
        Some("translate.zh\n")
    );
    drop(runtime);
    Ok(())
}
