use std::process::ExitStatus;
use std::thread;
use std::time::Duration;

const DEFAULT_PROVIDER_RETRY_ATTEMPTS: usize = 1;
const MAX_PROVIDER_RETRY_ATTEMPTS: usize = 3;
const PROVIDER_RETRY_BACKOFF_MS: u64 = 250;

pub(crate) fn provider_retry_attempts() -> usize {
    std::env::var("CTX_PROVIDER_RETRY_ATTEMPTS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value <= MAX_PROVIDER_RETRY_ATTEMPTS)
        .unwrap_or(DEFAULT_PROVIDER_RETRY_ATTEMPTS)
}

pub(crate) fn provider_transport_retryable(status: ExitStatus) -> bool {
    matches!(status.code(), Some(7 | 28 | 35 | 52 | 55 | 56))
}

pub(crate) fn wait_for_provider_retry(attempt: usize) {
    let multiplier = 1_u64 << attempt.min(2);
    thread::sleep(Duration::from_millis(
        PROVIDER_RETRY_BACKOFF_MS.saturating_mul(multiplier),
    ));
}
