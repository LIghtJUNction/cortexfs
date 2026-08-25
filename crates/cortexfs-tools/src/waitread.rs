use std::io::Read;
use std::thread::{self, JoinHandle};

pub(crate) fn read_capped(mut reader: impl Read, limit: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(limit.min(8 * 1024));
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let kept = read.min(limit.saturating_add(1).saturating_sub(output.len()));
        output.extend(buffer.iter().take(kept).copied());
        if output.len() >= limit.saturating_add(1) {
            break;
        }
    }
    output
}

pub(crate) fn take_finished(
    reader: &mut Option<JoinHandle<Vec<u8>>>,
    buffer: &mut Option<Vec<u8>>,
    limit: usize,
) -> bool {
    if buffer.is_some() || !reader.as_ref().is_some_and(JoinHandle::is_finished) {
        return false;
    }
    *buffer = reader.take().and_then(|handle| handle.join().ok());
    buffer.as_ref().is_some_and(|value| value.len() > limit)
}

pub(crate) fn join_reader(reader: Option<JoinHandle<Vec<u8>>>, buffer: Option<Vec<u8>>) -> Vec<u8> {
    buffer
        .or_else(|| reader.and_then(|handle| handle.join().ok()))
        .unwrap_or_default()
}

pub(crate) fn sleep_tick() {
    thread::sleep(std::time::Duration::from_millis(50));
}
