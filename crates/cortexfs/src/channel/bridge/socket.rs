use std::{fs, os::unix::fs::FileTypeExt, path::Path};

use super::ChannelBridgeError;
use cortexfs_runtime_client::RuntimeClientError;

pub(super) fn check(path: &Path) -> Result<(), ChannelBridgeError> {
    let metadata = fs::metadata(path)
        .map_err(|_error| ChannelBridgeError::Runtime(RuntimeClientError::CannotConnect))?;
    if !metadata.file_type().is_socket() {
        return Err(ChannelBridgeError::Runtime(
            RuntimeClientError::CannotConnect,
        ));
    }
    Ok(())
}
