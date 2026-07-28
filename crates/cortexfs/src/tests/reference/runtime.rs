use super::*;
use crate::reference::reconcile::reconcile_provider_model_tree;
use std::os::unix::fs::{PermissionsExt, symlink};

fn local_config(models: &str) -> String {
    format!(r#"{{"name":"local","base_url":"http://127.0.0.1/v1","models":{models}}}"#)
}

fn install_residues(root: &Path) -> std::io::Result<Vec<String>> {
    Ok(fs::read_dir(root.join("model"))?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with(".cortexfs-install-"))
        .collect())
}

#[test]
fn provider_projection_replaces_removed_models_and_empty_provider_dirs()
-> Result<(), Box<dyn std::error::Error>> {
    let root = clean_test_dir("reference-provider-reconcile");
    let providers = root.join("providers.d");
    let config = providers.join("local.json");
    let cache = root.join("provider-models");
    write_text_file(&config, &local_config("[\"old\"]"));

    assert!(reconcile_provider_model_tree(&root, &providers, &cache).is_ok());
    assert!(root.join("model/local/old").is_file());

    write_text_file(&config, &local_config("[\"new\"]"));
    assert!(reconcile_provider_model_tree(&root, &providers, &cache).is_ok());
    assert!(!root.join("model/local/old").exists());
    assert!(root.join("model/local/new.d/id").is_file());

    write_text_file(&config, &local_config("[]"));
    assert!(reconcile_provider_model_tree(&root, &providers, &cache).is_ok());
    assert!(root.join("model/local").is_dir());
    assert_eq!(
        fs::read_dir(root.join("model/local"))?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<std::io::Result<Vec<_>>>()?,
        [".cortexfs-provider.json"]
    );

    fs::remove_file(config)?;
    assert!(reconcile_provider_model_tree(&root, &providers, &cache).is_ok());
    assert!(!root.join("model/local").exists());
    Ok(())
}

#[test]
fn provider_projection_fails_closed_for_unknown_provider_content()
-> Result<(), Box<dyn std::error::Error>> {
    let root = clean_test_dir("reference-provider-foreign");
    let providers = root.join("providers.d");
    let config = providers.join("local.json");
    let cache = root.join("provider-models");
    write_text_file(&config, &local_config("[\"old\"]"));
    assert!(reconcile_provider_model_tree(&root, &providers, &cache).is_ok());
    write_text_file(&root.join("model/local/foreign"), "keep\n");
    write_text_file(&config, &local_config("[\"new\"]"));

    assert!(reconcile_provider_model_tree(&root, &providers, &cache).is_err());
    assert_eq!(
        fs::read_to_string(root.join("model/local/foreign"))?,
        "keep\n"
    );
    assert!(root.join("model/local/old").is_file());
    assert!(!root.join("model/local/new").exists());
    assert!(install_residues(&root)?.is_empty());
    Ok(())
}

#[test]
fn provider_projection_rejects_mode_owner_and_wrong_kind_tampering()
-> Result<(), Box<dyn std::error::Error>> {
    for case in ["mode", "owner", "kind"] {
        let root = clean_test_dir(&format!("reference-provider-tamper-{case}"));
        let providers = root.join("providers.d");
        let config = providers.join("local.json");
        let cache = root.join("provider-models");
        write_text_file(&config, &local_config("[\"old\"]"));
        assert!(reconcile_provider_model_tree(&root, &providers, &cache).is_ok());
        let provider = root.join("model/local");
        match case {
            "mode" => {
                fs::set_permissions(provider.join("old.d/id"), fs::Permissions::from_mode(0o600))?;
            }
            "owner" => {
                let manifest = provider.join(".cortexfs-provider.json");
                let mut value: serde_json::Value =
                    serde_json::from_str(&fs::read_to_string(&manifest)?)?;
                let entry = value
                    .get_mut("entries")
                    .and_then(serde_json::Value::as_array_mut)
                    .and_then(|entries| entries.first_mut())
                    .and_then(serde_json::Value::as_object_mut)
                    .ok_or_else(|| std::io::Error::other("provider receipt entry missing"))?;
                entry.insert(
                    "uid".to_owned(),
                    serde_json::json!(u64::from(nix::unistd::getuid().as_raw()) + 1),
                );
                fs::write(&manifest, serde_json::to_vec(&value)?)?;
                fs::set_permissions(&manifest, fs::Permissions::from_mode(0o600))?;
            }
            "kind" => {
                fs::remove_file(provider.join("old.d/id"))?;
                symlink("missing", provider.join("old.d/id"))?;
            }
            _ => return Err(std::io::Error::other("unknown tamper case").into()),
        }
        write_text_file(&config, &local_config("[\"new\"]"));
        assert!(reconcile_provider_model_tree(&root, &providers, &cache).is_err());
        assert!(
            provider.join("old").is_file(),
            "old wrapper lost after {case}"
        );
        assert!(!provider.join("new").exists());
        assert!(install_residues(&root)?.is_empty(), "residue after {case}");
    }
    Ok(())
}

#[test]
fn malformed_former_active_provider_preserves_projection_without_residue() -> std::io::Result<()> {
    let root = clean_test_dir("reference-provider-malformed-active");
    let providers = root.join("providers.d");
    let config = providers.join("local.json");
    let cache = root.join("provider-models");
    write_text_file(&config, &local_config("[\"old\"]"));
    assert!(reconcile_provider_model_tree(&root, &providers, &cache).is_ok());
    write_text_file(&config, "{");

    assert!(reconcile_provider_model_tree(&root, &providers, &cache).is_err());
    assert!(root.join("model/local/old").is_file());
    assert!(install_residues(&root)?.is_empty());
    Ok(())
}

#[test]
fn inactive_legacy_provider_fails_closed() -> std::io::Result<()> {
    let root = clean_test_dir("reference-provider-inactive-legacy");
    let providers = root.join("providers.d");
    let config = providers.join("local.json");
    let cache = root.join("provider-models");
    write_text_file(&config, &local_config("[\"old\"]"));
    assert!(reconcile_provider_model_tree(&root, &providers, &cache).is_ok());
    fs::remove_file(root.join("model/local/.cortexfs-provider.json"))?;
    fs::remove_file(config)?;
    assert!(reconcile_provider_model_tree(&root, &providers, &cache).is_err());
    assert!(root.join("model/local/old").is_file());
    assert!(install_residues(&root)?.is_empty());
    Ok(())
}

#[test]
fn reserved_receipted_provider_is_retired() -> std::io::Result<()> {
    let root = clean_test_dir("reference-provider-reserved-retire");
    let providers = root.join("providers.d");
    let config = providers.join("local.json");
    let cache = root.join("provider-models");
    write_text_file(&config, &local_config("[\"old\"]"));
    assert!(reconcile_provider_model_tree(&root, &providers, &cache).is_ok());
    fs::rename(root.join("model/local"), root.join("model/main"))?;
    fs::remove_file(config)?;
    assert!(reconcile_provider_model_tree(&root, &providers, &cache).is_ok());
    assert!(!root.join("model/main").exists());
    assert!(install_residues(&root)?.is_empty());
    Ok(())
}

#[test]
fn reserved_entries_require_their_canonical_kind_or_provenance() -> std::io::Result<()> {
    for name in ["route", "main"] {
        let root = clean_test_dir(&format!("reference-provider-reserved-kind-{name}"));
        let providers = root.join("providers.d");
        let cache = root.join("provider-models");
        fs::create_dir_all(root.join("model").join(name))?;

        assert!(reconcile_provider_model_tree(&root, &providers, &cache).is_err());
        assert!(root.join("model").join(name).is_dir());
        assert!(install_residues(&root)?.is_empty());
    }
    Ok(())
}

#[test]
fn canonical_debug_survives_provider_reconciliation_but_foreign_debug_blocks() -> std::io::Result<()>
{
    let root = clean_test_dir("reference-provider-canonical-debug");
    let providers = root.join("providers.d");
    let cache = root.join("provider-models");
    let debug = crate::reference::bootstrap::ensure_reference_debug_model(&root);
    assert!(debug.is_ok(), "{debug:?}");
    assert!(reconcile_provider_model_tree(&root, &providers, &cache).is_ok());

    write_text_file(&root.join("model/debug/foreign"), "keep\n");
    assert!(reconcile_provider_model_tree(&root, &providers, &cache).is_err());
    assert_eq!(
        fs::read_to_string(root.join("model/debug/foreign"))?,
        "keep\n"
    );
    assert!(install_residues(&root)?.is_empty());
    Ok(())
}

#[test]
fn provider_projection_preserves_concurrent_directory_replacement() -> std::io::Result<()> {
    let root = clean_test_dir("reference-provider-concurrent");
    let providers = root.join("providers.d");
    let config = providers.join("local.json");
    let cache = root.join("provider-models");
    write_text_file(&config, &local_config("[\"old\"]"));
    assert!(reconcile_provider_model_tree(&root, &providers, &cache).is_ok());
    write_text_file(&config, &local_config("[\"new\"]"));
    let previous = crate::support::receipt::set_park_hook(Some(Box::new(|parent, name| {
        nix::sys::stat::mkdirat(
            parent,
            name,
            nix::sys::stat::Mode::from_bits_truncate(0o755),
        )
        .map_err(std::io::Error::from)?;
        fs::write(
            crate::support::plain::proc_fd_path(parent)
                .join(name)
                .join("foreign"),
            "keep",
        )?;
        Ok(())
    })));
    let result = reconcile_provider_model_tree(&root, &providers, &cache);
    let _previous = crate::support::receipt::set_park_hook(previous);
    assert!(matches!(result, Err(ReferenceTreeError::CannotCreate)));
    assert_eq!(
        fs::read_to_string(root.join("model/local/foreign"))?,
        "keep"
    );
    Ok(())
}
