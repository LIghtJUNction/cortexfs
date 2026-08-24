use std::time::{SystemTime, UNIX_EPOCH};

#[must_use]
pub fn generated_sibling_name(target: &str, kind: &str, attempt: u8) -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!(".{target}.{kind}-{}-{nonce}-{attempt}", std::process::id())
}

/// Parses a generated sibling target marker and returns the base target name.
#[must_use]
pub fn generated_sibling_target<'a>(name: &'a str, kind: &str) -> Option<&'a str> {
    let rest = name.strip_prefix('.')?;
    let marker = format!(".{kind}-");
    let (target, suffix) = rest.split_once(&marker)?;
    if target.is_empty() {
        return None;
    }
    let mut suffix = suffix.split('-');
    suffix.next()?.parse::<u32>().ok()?;
    suffix.next()?.parse::<u128>().ok()?;
    suffix.next()?.parse::<u8>().ok()?;
    suffix.next().is_none().then_some(target)
}
