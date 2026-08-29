use std::io::Read;

use reqwest::blocking::Response;

use crate::{AppError, AppResult};

const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const RESPONSE_READ_LIMIT: u64 = 4 * 1024 * 1024 + 1;

pub(crate) fn read(response: Response, label: &str) -> AppResult<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length >= RESPONSE_READ_LIMIT)
    {
        return Err(AppError::new(format!(
            "{label} exceeds the {MAX_RESPONSE_BYTES}-byte limit"
        )));
    }
    let mut bytes = Vec::new();
    response
        .take(RESPONSE_READ_LIMIT)
        .read_to_end(&mut bytes)
        .map_err(|error| AppError::new(format!("cannot read {label}: {error}")))?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(AppError::new(format!(
            "{label} exceeds the {MAX_RESPONSE_BYTES}-byte limit"
        )));
    }
    Ok(bytes)
}
