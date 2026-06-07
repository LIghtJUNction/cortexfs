use crate::text::external_subject;
use crate::{configured_provider_ids, provider_spec};
use cortex_core::{Fingerprint, ModelId};

pub fn validate_staged_name(name: &str) -> fuse3::Result<()> {
    if name.is_empty()
        || name.contains('/')
        || name == "."
        || name == ".."
        || name.ends_with(".resp.json")
        || std::path::Path::new(name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("error"))
    {
        return Err(libc::EINVAL.into());
    }
    Ok(())
}

pub fn validate_collab_claim_staged_name(name: &str) -> fuse3::Result<()> {
    if name.is_empty()
        || name.contains('/')
        || name == "."
        || name == ".."
        || !std::path::Path::new(name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("tmp"))
    {
        return Err(libc::EINVAL.into());
    }
    Ok(())
}

pub fn validate_collab_lock_staged_name(name: &str) -> fuse3::Result<()> {
    validate_collab_claim_staged_name(name)
}

pub fn validate_collab_lock_id(name: &str) -> fuse3::Result<()> {
    if name.is_empty() || name.contains('/') || name == "." || name == ".." || name.contains('.') {
        return Err(libc::EINVAL.into());
    }
    Ok(())
}

pub fn validate_control_write(offset: u64, data: &[u8]) -> fuse3::Result<()> {
    if offset != 0 {
        return Err(libc::EINVAL.into());
    }
    let command = std::str::from_utf8(data).map_err(|_error| libc::EINVAL)?;
    if command.trim() != "1" {
        return Err(libc::EINVAL.into());
    }
    Ok(())
}

pub fn normalize_collab_claim_owner(body: &str) -> fuse3::Result<String> {
    normalize_collab_actor(body)
}

pub fn normalize_collab_actor(body: &str) -> fuse3::Result<String> {
    let owner = body.trim();
    if matches!(owner, "agent/helper" | "cluster/local/worker/local-worker") {
        return Ok(owner.to_owned());
    }
    Err(libc::EINVAL.into())
}

pub fn request_fingerprint(
    format: &str,
    _request_id: &str,
    request_content: &str,
) -> fuse3::Result<Fingerprint> {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in format.bytes().chain([0]).chain(request_content.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Fingerprint::new(format!("fnv1a64:{hash:016x}")).map_err(|_error| libc::EINVAL.into())
}

pub fn normalize_export_filter_value(data: &[u8]) -> fuse3::Result<String> {
    let value = std::str::from_utf8(data).map_err(|_error| libc::EINVAL)?;
    Ok(format!("{}\n", value.trim()))
}

pub fn validate_preference_pair(body: &str) -> Result<(), String> {
    let value =
        serde_json::from_str::<serde_json::Value>(body).map_err(|error| error.to_string())?;
    let chosen = value
        .get("chosen")
        .ok_or_else(|| "missing chosen".to_owned())?;
    let rejected = value
        .get("rejected")
        .ok_or_else(|| "missing rejected".to_owned())?;
    if chosen == rejected {
        return Err("chosen and rejected must differ".to_owned());
    }
    Ok(())
}

pub fn validate_external_thread_subject(body: &str) -> fuse3::Result<()> {
    if external_subject(body).as_deref() != Some("qq:user:123456") {
        return Err(libc::EACCES.into());
    }
    Ok(())
}

pub fn request_model(body: &str) -> fuse3::Result<Option<ModelId>> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return Ok(None);
    };
    let Some(model) = value.get("model") else {
        return Ok(None);
    };
    let model = model.as_str().ok_or(libc::EINVAL)?;
    ModelId::new(model)
        .map(Some)
        .map_err(|_error| fuse3::Errno::from(libc::EINVAL))
}

pub fn normalize_allowed_providers(providers: &str) -> fuse3::Result<String> {
    let trimmed = providers.trim();
    if trimmed.is_empty() {
        return Ok(default_allowed_providers_content());
    }
    let mut normalized = String::new();
    let mut seen = Vec::new();
    for provider in providers
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if provider_spec(provider).is_none() {
            return Err(libc::EINVAL.into());
        }
        if seen.iter().any(|seen_provider| seen_provider == provider) {
            continue;
        }
        seen.push(provider.to_owned());
        normalized.push_str(provider);
        normalized.push('\n');
    }
    if normalized.is_empty() {
        Ok(default_allowed_providers_content())
    } else {
        Ok(normalized)
    }
}

pub fn default_allowed_providers_content() -> String {
    let mut content = configured_provider_ids().collect::<Vec<_>>().join("\n");
    content.push('\n');
    content
}

pub fn allowed_provider_lines(content: &str) -> impl Iterator<Item = &str> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
}
