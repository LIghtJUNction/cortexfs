use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

pub(super) fn token() -> String {
    format!("cortexfs-{:x}", NEXT.fetch_add(1, Ordering::Relaxed))
}
