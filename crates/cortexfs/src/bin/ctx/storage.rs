use super::*;

pub(crate) fn update_storage(path: Option<&Path>) -> Result<(), CliError> {
    let storage = path.unwrap_or_else(|| Path::new(cortexfs::SYSTEM_STORAGE_DIR));
    let generation = update_storage_generation(storage)
        .map_err(|error| CliError::unavailable(format!("cannot update storage: {error}")))?;
    print_line(&generation.display().to_string())
}
