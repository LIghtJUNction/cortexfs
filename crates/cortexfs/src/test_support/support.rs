use fuse3::Inode;

use crate::CortexFs;

pub fn assert_format_route(
    runtime: &crate::RuntimeState,
    routes: Inode,
    format: &str,
    provider: &str,
    model: &str,
    reason: &str,
) -> fuse3::Result<()> {
    let route = runtime
        .lookup_child(routes, format)
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    assert_eq!(
        runtime
            .lookup_child(route, "provider")
            .and_then(crate::Node::content),
        Some(provider)
    );
    assert_eq!(
        runtime
            .lookup_child(route, "model")
            .and_then(crate::Node::content),
        Some(model)
    );
    assert_eq!(
        runtime
            .lookup_child(route, "reason")
            .and_then(crate::Node::content),
        Some(reason)
    );
    Ok(())
}

pub fn assert_openai_chat_route(
    runtime: &crate::RuntimeState,
    routes: Inode,
    provider: &str,
    model: &str,
    reason: &str,
) -> fuse3::Result<()> {
    assert_format_route(runtime, routes, "openai.chat", provider, model, reason)
}

pub fn provider_model_text(provider: &str) -> String {
    format!(
        "{}\n",
        crate::default_model_for_provider(provider).unwrap_or_default()
    )
}

pub fn user_models_file_content(fs: &CortexFs, name: &str) -> fuse3::Result<String> {
    let inode = fs.user_models_file_inode(name)?;
    fs.node_content(inode)
}

pub fn assert_model_metadata(
    fs: &CortexFs,
    path: &[String],
    provider: &crate::ProviderRuntimeSpec,
) {
    let mut context_window = path.to_vec();
    context_window.push("context_window".to_owned());
    assert_eq!(
        fs.lookup_path_owned(&context_window)
            .and_then(crate::Node::content),
        Some(provider.context_window)
    );

    let mut max_output_tokens = path.to_vec();
    max_output_tokens.push("max_output_tokens".to_owned());
    assert_eq!(
        fs.lookup_path_owned(&max_output_tokens)
            .and_then(crate::Node::content),
        Some(provider.max_output_tokens)
    );

    let mut cap = path.to_vec();
    cap.push("cap".to_owned());
    assert_eq!(
        fs.lookup_path_owned(&cap).and_then(crate::Node::content),
        Some(provider.model_capabilities)
    );
}

pub fn set_default_provider(
    fs: &CortexFs,
    provider: &crate::ProviderRuntimeSpec,
) -> fuse3::Result<()> {
    let routes = fs
        .tree
        .path_inode(crate::USER_ROUTES_DIR_PATH)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let mut runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    let default_provider = runtime
        .lookup_child(routes, "default_provider")
        .map(crate::Node::inode)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    runtime.write(default_provider, 0, format!("{}\n", provider.id).as_bytes())?;
    drop(runtime);
    Ok(())
}
