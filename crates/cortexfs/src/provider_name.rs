use std::fs;
use std::fs::File;
use std::io::Read as _;
use std::net::IpAddr;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use nix::fcntl::{FcntlArg, FdFlag, fcntl};

const PROVIDER_SYSTEM_SECRET_ROOT: &str = "/var/lib/cortexfs/secrets/provider";
const MAX_PROVIDER_SYSTEM_SECRET_BYTES: u64 = 64 * 1024;

/// Error returned when a provider config cannot produce a stable provider name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderNameError {
    /// The provider base URL has no usable host.
    MissingHost,
    /// A local/IP endpoint needs an explicit stable provider name.
    MissingNameForAddress,
    /// The configured provider name is not a `CortexFS` object name.
    InvalidName,
}

/// Returns the stable `CortexFS` provider name for a provider config.
///
/// Official providers use short canonical names when `name` is omitted.
/// Non-official domain providers keep their domain name by default. IP and
/// localhost endpoints must set `name` so `/ctx/model/<provider>` remains a
/// stable object path rather than an address literal.
pub fn provider_name_from_config(
    base_url: &str,
    name: Option<&str>,
) -> Result<String, ProviderNameError> {
    if let Some(name) = name.map(str::trim).filter(|value| !value.is_empty()) {
        return if crate::is_object_name(name) {
            Ok(name.to_owned())
        } else {
            Err(ProviderNameError::InvalidName)
        };
    }

    let host = provider_host_from_base_url(base_url).ok_or(ProviderNameError::MissingHost)?;
    if provider_host_requires_name(&host) {
        return Err(ProviderNameError::MissingNameForAddress);
    }

    Ok(canonical_provider_name_from_host(&host).to_owned())
}

/// Returns the stable `CortexFS` provider name for a provider base URL.
///
/// Prefer `provider_name_from_config` for provider JSON. This lower-level
/// helper is kept for callers that only need host canonicalization.
#[must_use]
pub fn provider_name_from_base_url(base_url: &str) -> Option<String> {
    let host = provider_host_from_base_url(base_url)?;
    Some(canonical_provider_name_from_host(&host).to_owned())
}

/// Returns the lowercase host from a provider base URL.
#[must_use]
pub fn provider_host_from_base_url(base_url: &str) -> Option<String> {
    let mut rest = base_url.trim();
    if rest.bytes().any(|byte| byte.is_ascii_control()) {
        return None;
    }
    if let Some(value) = rest.strip_prefix("https://") {
        rest = value;
    } else if let Some(value) = rest.strip_prefix("http://") {
        rest = value;
    } else {
        return None;
    }
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default();
    let host = authority
        .split(':')
        .next()
        .unwrap_or_default()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// Returns the generated OAuth access-token environment variable for a provider.
#[must_use]
pub fn provider_oauth_access_token_env_name(provider: &str) -> String {
    format!("CTX_{}_OAUTH_ACCESS_TOKEN", provider_env_label(provider))
}

/// Returns the generated OAuth refresh-token environment variable for a provider.
#[must_use]
pub fn provider_oauth_refresh_token_env_name(provider: &str) -> String {
    format!("CTX_{}_OAUTH_REFRESH_TOKEN", provider_env_label(provider))
}

/// Returns the system keychain service name for a provider.
#[must_use]
pub fn provider_keychain_service(provider: &str) -> String {
    format!("cortexfs:{provider}")
}

/// Stores a provider API secret in the root-owned `CortexFS` system secret store.
pub fn store_provider_system_secret(
    provider: &str,
    account: &str,
    secret: &str,
) -> Result<(), ProviderSystemSecretError> {
    let path = provider_system_secret_path(provider, account)?;
    let Some(parent) = path.parent() else {
        return Err(ProviderSystemSecretError::InvalidName);
    };
    create_private_provider_secret_dir(parent)?;
    set_private_dir_permissions(Path::new("/var/lib/cortexfs/secrets"))?;
    set_private_dir_permissions(Path::new(PROVIDER_SYSTEM_SECRET_ROOT))?;
    set_private_dir_permissions(parent)?;
    super::atomic_replace_text_with_mode(&path, &format!("{secret}\n"), 0o600)
        .map_err(|_error| ProviderSystemSecretError::CannotWrite)?;
    sync_provider_secret_dir(parent)
}

/// Reads a provider API secret from the root-owned `CortexFS` system secret store.
pub fn read_provider_system_secret(
    provider: &str,
    account: &str,
) -> Result<Option<String>, ProviderSystemSecretError> {
    let path = provider_system_secret_path(provider, account)?;
    let content = match read_provider_secret_file(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_error) => return Err(ProviderSystemSecretError::CannotRead),
    };
    let secret = content.trim_end_matches(['\r', '\n']);
    if secret.is_empty() {
        Ok(None)
    } else {
        Ok(Some(secret.to_owned()))
    }
}

/// Opens a provider API secret and clears close-on-exec so a runtime child can
/// inherit it without exposing the secret in environment variables.
pub fn open_provider_system_secret(
    provider: &str,
    account: &str,
) -> Result<Option<ProviderSystemSecretHandle>, ProviderSystemSecretError> {
    let path = provider_system_secret_path(provider, account)?;
    let file = match open_provider_secret_file(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_error) => return Err(ProviderSystemSecretError::CannotRead),
    };
    clear_fd_cloexec(&file)?;
    Ok(Some(ProviderSystemSecretHandle {
        provider: provider.to_owned(),
        account: account.to_owned(),
        file,
    }))
}

/// Returns whether a provider API secret exists in the system secret store.
pub fn provider_system_secret_exists(
    provider: &str,
    account: &str,
) -> Result<bool, ProviderSystemSecretError> {
    let path = provider_system_secret_path(provider, account)?;
    provider_secret_file_exists(&path)
}

fn provider_secret_file_exists(path: &Path) -> Result<bool, ProviderSystemSecretError> {
    match open_provider_secret_file(path) {
        Ok(_file) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_error) => Err(ProviderSystemSecretError::CannotRead),
    }
}

/// Error while reading or writing the `CortexFS` system provider secret store.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderSystemSecretError {
    /// Provider or account name is invalid.
    InvalidName,
    /// Secret could not be read.
    CannotRead,
    /// Secret could not be written.
    CannotWrite,
}

/// Open provider secret inherited by a runtime child via file descriptor.
#[derive(Debug)]
pub struct ProviderSystemSecretHandle {
    provider: String,
    account: String,
    file: File,
}

/// Provider secret material read before entering a reduced-privilege runtime.
#[derive(Debug)]
pub struct ProviderSystemSecret {
    provider: String,
    account: String,
    secret: String,
}

impl ProviderSystemSecret {
    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }

    #[must_use]
    pub fn account(&self) -> &str {
        &self.account
    }

    #[must_use]
    pub fn secret(&self) -> &str {
        &self.secret
    }
}

impl ProviderSystemSecretHandle {
    /// Environment metadata for passing this already-open secret fd.
    ///
    /// These variables contain no secret material; they identify only an fd and
    /// the provider slot it belongs to.
    #[must_use]
    pub fn env(&self) -> [(String, String); 3] {
        [
            (
                "CTX_PROVIDER_SECRET_FD".to_owned(),
                self.file.as_raw_fd().to_string(),
            ),
            (
                "CTX_PROVIDER_SECRET_PROVIDER".to_owned(),
                self.provider.clone(),
            ),
            ("CTX_PROVIDER_SECRET_SLOT".to_owned(), self.account.clone()),
        ]
    }
}

fn canonical_provider_name_from_host(host: &str) -> &str {
    match host {
        "api.openai.com" => "openai",
        "api.anthropic.com" => "anthropic",
        "generativelanguage.googleapis.com" => "google",
        _ => host,
    }
}

fn provider_host_requires_name(host: &str) -> bool {
    host == "localhost" || host.parse::<IpAddr>().is_ok()
}

/// Opens the default provider system secret for a selected model alias/name.
pub fn open_provider_system_secret_for_model(
    ctx_root: &Path,
    model: &str,
) -> Result<Option<ProviderSystemSecretHandle>, ProviderSystemSecretError> {
    let Some(provider) = selected_model_provider(ctx_root, model) else {
        return Ok(None);
    };
    open_provider_system_secret(&provider, "default")
}

/// Reads the default provider system secret for a selected model alias/name.
pub fn read_provider_system_secret_for_model(
    ctx_root: &Path,
    model: &str,
) -> Result<Option<ProviderSystemSecret>, ProviderSystemSecretError> {
    let Some(provider) = selected_model_provider(ctx_root, model) else {
        return Ok(None);
    };
    let account = "default";
    let Some(secret) = read_provider_system_secret(&provider, account)? else {
        return Ok(None);
    };
    Ok(Some(ProviderSystemSecret {
        provider,
        account: account.to_owned(),
        secret,
    }))
}

fn selected_model_provider(ctx_root: &Path, model: &str) -> Option<String> {
    let model = model.trim();
    if model.contains('/') {
        return model.split_once('/').and_then(|(provider, model)| {
            (!provider.is_empty() && !model.is_empty()).then_some(provider.to_owned())
        });
    }
    if !matches!(model, "main" | "helper") {
        return None;
    }
    let target = read_model_alias_target(ctx_root, model).ok()?;
    let target = target
        .strip_prefix("/ctx/model/")
        .or_else(|| target.strip_prefix("model/"))
        .unwrap_or(&target);
    target.split_once('/').and_then(|(provider, model)| {
        (!provider.is_empty() && !model.is_empty()).then_some(provider.to_owned())
    })
}

fn read_model_alias_target(ctx_root: &Path, alias: &str) -> std::io::Result<String> {
    let model_dir = open_plain_directory_no_follow(&ctx_root.join("model"))?;
    let target = nix::fcntl::readlinkat(&model_dir, alias).map_err(std::io::Error::from)?;
    Ok(target.to_string_lossy().into_owned())
}

fn provider_system_secret_path(
    provider: &str,
    account: &str,
) -> Result<std::path::PathBuf, ProviderSystemSecretError> {
    if !crate::is_object_name(provider) || !is_secret_account_name(account) {
        return Err(ProviderSystemSecretError::InvalidName);
    }
    Ok(Path::new(PROVIDER_SYSTEM_SECRET_ROOT)
        .join(provider)
        .join(account))
}

fn is_secret_account_name(value: &str) -> bool {
    crate::is_object_name(value)
}

fn read_provider_secret_file(path: &Path) -> std::io::Result<String> {
    let mut file = open_provider_secret_file(path)?;
    let len = file.metadata()?.len();
    let len = usize::try_from(len)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
    let mut content = vec![0; len];
    file.read_exact(&mut content)?;
    String::from_utf8(content)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.utf8_error()))
}

fn open_provider_secret_file(path: &Path) -> std::io::Result<File> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "provider secret has no parent",
        )
    })?;
    let parent_dir = open_plain_directory_no_follow(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid provider secret name",
            )
        })?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )?;
    let file = File::from(file_fd);
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_PROVIDER_SYSTEM_SECRET_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "provider secret file is invalid",
        ));
    }
    Ok(file)
}

fn set_private_dir_permissions(path: &Path) -> Result<(), ProviderSystemSecretError> {
    let dir = match open_plain_directory_no_follow(path) {
        Ok(dir) => dir,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_error) => return Err(ProviderSystemSecretError::CannotWrite),
    };
    if !dir
        .metadata()
        .map_err(|_error| ProviderSystemSecretError::CannotWrite)?
        .is_dir()
    {
        return Err(ProviderSystemSecretError::CannotWrite);
    }
    dir.set_permissions(fs::Permissions::from_mode(0o700))
        .and_then(|()| dir.sync_all())
        .map_err(|_error| ProviderSystemSecretError::CannotWrite)
}

fn create_private_provider_secret_dir(path: &Path) -> Result<(), ProviderSystemSecretError> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        return if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            set_private_dir_permissions(path)
        } else {
            Err(ProviderSystemSecretError::CannotWrite)
        };
    }

    let mut missing = Vec::new();
    let mut cursor = Some(path);
    while let Some(current) = cursor {
        match fs::symlink_metadata(current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() => {
                return Err(ProviderSystemSecretError::CannotWrite);
            }
            Ok(_metadata) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(current.to_path_buf());
                cursor = current.parent();
            }
            Err(_error) => return Err(ProviderSystemSecretError::CannotWrite),
        }
    }

    let parent = missing
        .last()
        .and_then(|path| path.parent())
        .ok_or(ProviderSystemSecretError::CannotWrite)?;
    let mut parent_dir = open_plain_directory_no_follow(parent)
        .map_err(|_error| ProviderSystemSecretError::CannotWrite)?;
    for directory in missing.iter().rev() {
        let name = plain_file_name(directory).ok_or(ProviderSystemSecretError::CannotWrite)?;
        nix::sys::stat::mkdirat(
            &parent_dir,
            name,
            nix::sys::stat::Mode::from_bits_truncate(0o700),
        )
        .map_err(|_error| ProviderSystemSecretError::CannotWrite)?;
        parent_dir
            .sync_all()
            .map_err(|_error| ProviderSystemSecretError::CannotWrite)?;
        let child = nix::fcntl::openat(
            &parent_dir,
            name,
            nix::fcntl::OFlag::O_DIRECTORY
                | nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::empty(),
        )
        .map_err(|_error| ProviderSystemSecretError::CannotWrite)?;
        parent_dir = File::from(child);
        parent_dir
            .set_permissions(fs::Permissions::from_mode(0o700))
            .and_then(|()| parent_dir.sync_all())
            .map_err(|_error| ProviderSystemSecretError::CannotWrite)?;
    }
    Ok(())
}

fn sync_provider_secret_dir(path: &Path) -> Result<(), ProviderSystemSecretError> {
    let dir = open_plain_directory_no_follow(path)
        .map_err(|_error| ProviderSystemSecretError::CannotWrite)?;
    dir.sync_all()
        .map_err(|_error| ProviderSystemSecretError::CannotWrite)
}

fn open_plain_directory_no_follow(path: &Path) -> std::io::Result<File> {
    let mut directory = if path.is_absolute() {
        open_plain_directory_no_follow_leaf(Path::new("/"))?
    } else {
        open_plain_directory_no_follow_leaf(Path::new("."))?
    };
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                let name = name.to_str().ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid directory name")
                })?;
                let next = nix::fcntl::openat(
                    &directory,
                    name,
                    nix::fcntl::OFlag::O_DIRECTORY
                        | nix::fcntl::OFlag::O_RDONLY
                        | nix::fcntl::OFlag::O_NOFOLLOW
                        | nix::fcntl::OFlag::O_CLOEXEC,
                    nix::sys::stat::Mode::empty(),
                )?;
                directory = File::from(next);
            }
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "directory path contains unsupported components",
                ));
            }
        }
    }
    Ok(directory)
}

fn open_plain_directory_no_follow_leaf(path: &Path) -> std::io::Result<File> {
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_DIRECTORY | nix::libc::O_NOFOLLOW)
        .open(path)?;
    if !directory.metadata()?.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path is not a plain directory",
        ));
    }
    Ok(directory)
}

fn plain_file_name(path: &Path) -> Option<&str> {
    path.file_name()?.to_str()
}

fn clear_fd_cloexec(file: &File) -> Result<(), ProviderSystemSecretError> {
    let flags =
        fcntl(file, FcntlArg::F_GETFD).map_err(|_error| ProviderSystemSecretError::CannotRead)?;
    let mut flags = FdFlag::from_bits_truncate(flags);
    flags.remove(FdFlag::FD_CLOEXEC);
    fcntl(file, FcntlArg::F_SETFD(flags))
        .map(|_value| ())
        .map_err(|_error| ProviderSystemSecretError::CannotRead)
}

fn provider_env_label(provider: &str) -> String {
    provider
        .bytes()
        .map(|byte| match byte {
            b'a'..=b'z' => char::from(byte.to_ascii_uppercase()),
            b'A'..=b'Z' | b'0'..=b'9' => char::from(byte),
            _ => '_',
        })
        .collect()
}

#[cfg(test)]
mod provider_secret_file_tests {
    use super::{
        create_private_provider_secret_dir, is_secret_account_name, open_provider_secret_file,
        provider_host_from_base_url, provider_secret_file_exists, read_provider_secret_file,
        selected_model_provider, set_private_dir_permissions,
    };
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn provider_secret_account_names_use_object_name_rules() {
        assert!(is_secret_account_name("default"));
        assert!(is_secret_account_name("office.prod"));
        assert!(!is_secret_account_name(""));
        assert!(!is_secret_account_name("."));
        assert!(!is_secret_account_name(".."));
        assert!(!is_secret_account_name("../default"));
        assert!(!is_secret_account_name("bad/name"));
        assert!(!is_secret_account_name("-bad"));
    }

    #[test]
    fn provider_base_url_host_requires_http_scheme_and_clean_text() {
        assert_eq!(
            provider_host_from_base_url("https://api.openai.com/v1"),
            Some("api.openai.com".to_owned())
        );
        assert_eq!(
            provider_host_from_base_url("http://127.0.0.1:8317/v1"),
            Some("127.0.0.1".to_owned())
        );
        assert_eq!(provider_host_from_base_url("api.openai.com/v1"), None);
        assert_eq!(provider_host_from_base_url("https:///v1"), None);
        assert_eq!(
            provider_host_from_base_url("https://api.openai.com\noutput=/tmp/leak"),
            None
        );
    }

    #[test]
    fn provider_secret_file_helpers_refuse_symlink_targets()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-provider-secret-symlink-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir_all(&root)?;
        let target = root.join("target");
        let link = root.join("link");
        fs::write(&target, "secret\n")?;
        symlink(&target, &link)?;

        assert!(read_provider_secret_file(&link).is_err());
        assert!(open_provider_secret_file(&link).is_err());
        assert_eq!(
            provider_secret_file_exists(&link),
            Err(super::ProviderSystemSecretError::CannotRead)
        );

        let _ignored = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn provider_secret_file_helpers_reject_symlink_intermediate_directory()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-provider-secret-symlink-parent-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        let outside = root.join("outside");
        fs::create_dir_all(outside.join("local"))?;
        fs::write(outside.join("local/default"), "secret\n")?;
        symlink(&outside, root.join("provider"))?;

        let path = root.join("provider").join("local").join("default");
        assert!(read_provider_secret_file(&path).is_err());
        assert!(open_provider_secret_file(&path).is_err());
        assert_eq!(
            provider_secret_file_exists(&path),
            Err(super::ProviderSystemSecretError::CannotRead)
        );
        assert_eq!(
            fs::read_to_string(outside.join("local/default"))?,
            "secret\n"
        );

        let _ignored = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn selected_model_provider_rejects_symlink_model_directory()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-selected-model-provider-symlink-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        let outside = root.join("outside");
        fs::create_dir_all(&outside)?;
        symlink("/ctx/model/evil/model", outside.join("main"))?;
        symlink(&outside, root.join("model"))?;

        assert_eq!(selected_model_provider(&root, "main"), None);

        let _ignored = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn provider_secret_file_helpers_read_plain_files() -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-provider-secret-plain-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir_all(&root)?;
        let path = root.join("default");
        fs::write(&path, "secret\n")?;

        assert_eq!(read_provider_secret_file(&path)?, "secret\n");
        assert!(open_provider_secret_file(&path).is_ok());
        assert_eq!(provider_secret_file_exists(&path), Ok(true));
        assert_eq!(
            provider_secret_file_exists(&root.join("missing")),
            Ok(false)
        );

        let _ignored = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn provider_secret_file_helpers_reject_non_regular_and_oversized_files()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-provider-secret-invalid-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir_all(&root)?;
        let dir = root.join("dir");
        fs::create_dir_all(&dir)?;
        let oversized = root.join("oversized");
        fs::write(&oversized, "x".repeat((64 * 1024) + 1))?;

        assert!(open_provider_secret_file(&dir).is_err());
        assert!(read_provider_secret_file(&dir).is_err());
        assert!(open_provider_secret_file(&oversized).is_err());
        assert!(read_provider_secret_file(&oversized).is_err());

        let _ignored = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn provider_secret_private_dir_permissions_repair_plain_directories()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-provider-secret-dir-plain-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir_all(&root)?;
        let target = root.join("target");
        fs::create_dir_all(&target)?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755))?;

        assert_eq!(set_private_dir_permissions(&target), Ok(()));
        assert_eq!(fs::metadata(&target)?.permissions().mode() & 0o777, 0o700);

        let _ignored = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn provider_secret_private_dir_permissions_refuse_symlink_directories()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-provider-secret-dir-symlink-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir_all(&root)?;
        let target = root.join("target");
        let link = root.join("link");
        fs::create_dir_all(&target)?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755))?;
        symlink(&target, &link)?;

        assert_eq!(
            set_private_dir_permissions(&link),
            Err(super::ProviderSystemSecretError::CannotWrite)
        );
        assert_eq!(fs::metadata(&target)?.permissions().mode() & 0o777, 0o755);

        let _ignored = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn provider_secret_private_dir_permissions_reject_symlink_intermediate_directory()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-provider-secret-dir-symlink-parent-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        let outside = root.join("outside");
        fs::create_dir_all(outside.join("local"))?;
        fs::set_permissions(outside.join("local"), fs::Permissions::from_mode(0o755))?;
        symlink(&outside, root.join("provider"))?;

        assert_eq!(
            set_private_dir_permissions(&root.join("provider").join("local")),
            Err(super::ProviderSystemSecretError::CannotWrite)
        );
        assert_eq!(
            fs::metadata(outside.join("local"))?.permissions().mode() & 0o777,
            0o755
        );

        let _ignored = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn provider_secret_dir_creation_sets_private_modes() -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-provider-secret-create-private-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        let path = root.join("secrets").join("provider").join("local");

        assert_eq!(create_private_provider_secret_dir(&path), Ok(()));
        assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o700);
        assert_eq!(
            fs::metadata(root.join("secrets"))?.permissions().mode() & 0o777,
            0o700
        );

        let _ignored = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn provider_secret_dir_creation_refuses_symlink_parent()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-provider-secret-create-symlink-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        let outside = std::env::temp_dir().join(format!(
            "cortexfs-provider-secret-create-symlink-outside-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir_all(root.join("secrets"))?;
        fs::create_dir_all(&outside)?;
        symlink(&outside, root.join("secrets").join("provider"))?;
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o755))?;

        assert_eq!(
            create_private_provider_secret_dir(
                &root.join("secrets").join("provider").join("local")
            ),
            Err(super::ProviderSystemSecretError::CannotWrite)
        );
        assert!(!outside.join("local").exists());
        assert_eq!(fs::metadata(&outside)?.permissions().mode() & 0o777, 0o755);

        let _ignored = fs::remove_dir_all(root);
        let _ignored = fs::remove_dir_all(outside);
        Ok(())
    }
}
