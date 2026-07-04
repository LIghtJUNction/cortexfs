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
    crate::atomic_replace_text_with_mode(&path, &format!("{secret}\n"), 0o600)
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
        path,
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
    path: PathBuf,
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
    /// These variables contain no secret material; they identify only an fd/path
    /// and the provider slot it belongs to.
    #[must_use]
    pub fn env(&self) -> [(String, String); 4] {
        [
            (
                "CTX_PROVIDER_SECRET_FD".to_owned(),
                self.file.as_raw_fd().to_string(),
            ),
            (
                "CTX_PROVIDER_SECRET_PATH".to_owned(),
                self.path.display().to_string(),
            ),
            (
                "CTX_PROVIDER_SECRET_PROVIDER".to_owned(),
                self.provider.clone(),
            ),
            ("CTX_PROVIDER_SECRET_SLOT".to_owned(), self.account.clone()),
        ]
    }
}
