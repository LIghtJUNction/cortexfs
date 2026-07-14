pub mod catalog;
pub mod discovery;
pub mod model;
pub mod name;
pub mod oauth;

pub(crate) fn effective_base_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.rsplit('/').next() == Some("v1") {
        base.to_owned()
    } else {
        format!("{base}/v1")
    }
}
