use super::*;
use crate::reference::bootstrap::ensure_reference_model_aliases;
use crate::reference::reconcile::reconcile_provider_model_tree;
use std::os::unix::fs::symlink;

#[test]
fn reference_alias_replaces_stale_target_without_clobbering_a_concurrent_alias()
-> Result<(), Box<dyn std::error::Error>> {
    let root = clean_test_dir("reference-alias-reconcile");
    let providers = root.join("providers.d");
    let cache = root.join("provider-models");
    write_text_file(
        &providers.join("local.json"),
        r#"{"name":"local","base_url":"http://127.0.0.1/v1","models":["current"]}"#,
    );
    let models = reconcile_provider_model_tree(&root, &providers, &cache)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let main = root.join("model/main");
    symlink("/ctx/model/removed/old", &main)?;

    ensure_reference_model_aliases(&root, &models)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    assert_eq!(
        fs::read_link(&main)?,
        PathBuf::from("/ctx/model/local/current")
    );
    assert!(
        fs::read_dir(root.join("model"))?
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .all(|name| !name.starts_with(".cortexfs-alias-"))
    );
    fs::remove_file(&main)?;
    symlink("/ctx/model/removed/old", &main)?;
    let previous = crate::support::receipt::set_park_hook(Some(Box::new(|directory, name| {
        nix::unistd::symlinkat("/ctx/model/debug/echo", directory, name)
            .map_err(std::io::Error::from)?;
        Ok(())
    })));
    let result = ensure_reference_model_aliases(&root, &models);
    let _previous = crate::support::receipt::set_park_hook(previous);

    assert_eq!(result, Err(ReferenceTreeError::CannotLink));
    assert_eq!(fs::read_link(main)?, PathBuf::from("/ctx/model/debug/echo"));
    Ok(())
}
