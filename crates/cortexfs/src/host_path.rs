#![allow(
    clippy::redundant_pub_crate,
    reason = "host_path is a private module shared by sibling modules without becoming public API"
)]

use std::path::Path;

pub(crate) fn is_absolute_host_workspace_path(value: &str) -> bool {
    !value.bytes().any(|byte| byte.is_ascii_control())
        && Path::new(value).is_absolute()
        && Path::new(value).components().all(|component| {
            matches!(
                component,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        })
}
