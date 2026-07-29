use super::{PolicyObjectClass, PolicyPermission, PolicyV0};

/// Supplies policy decisions to authority-enforcement mechanisms.
///
/// Implementations may use the built-in v0 allowlist, a compiled policy, or
/// another host-owned source. Callers must still enforce Linux identity,
/// mount, path, and principal constraints independently.
pub trait PolicyEvaluator: std::fmt::Debug {
    /// Returns whether one concrete policy request is allowed.
    fn evaluate(
        &self,
        subject_type: &str,
        object_class: PolicyObjectClass,
        object_name: &str,
        permission: PolicyPermission,
    ) -> bool;
}

impl PolicyEvaluator for PolicyV0 {
    fn evaluate(
        &self,
        subject_type: &str,
        object_class: PolicyObjectClass,
        object_name: &str,
        permission: PolicyPermission,
    ) -> bool {
        Self::allows(self, subject_type, object_class, object_name, permission)
    }
}
