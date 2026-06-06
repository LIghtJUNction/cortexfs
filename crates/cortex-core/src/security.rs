//! Security and policy domain types for `CortexFS`.

use core::fmt;

/// SELinux-style label attached to actors and filesystem objects.
///
/// The fields intentionally remain textual because `CortexFS` needs to represent
/// both local Linux identities and external subjects such as chat users.
#[derive(Debug, Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SecurityContext {
    identity: String,
    role: String,
    domain: String,
    range: String,
}

impl SecurityContext {
    #[must_use]
    pub fn new(identity: String, role: String, domain: String, range: String) -> Self {
        Self {
            identity,
            role,
            domain,
            range,
        }
    }

    #[must_use]
    pub fn identity(&self) -> &str {
        &self.identity
    }

    #[must_use]
    pub fn role(&self) -> &str {
        &self.role
    }

    #[must_use]
    pub fn domain(&self) -> &str {
        &self.domain
    }

    #[must_use]
    pub fn range(&self) -> &str {
        &self.range
    }
}

impl fmt::Display for SecurityContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}:{}",
            self.identity, self.role, self.domain, self.range
        )
    }
}

/// Local process identity received from the kernel for a FUSE request.
#[derive(Debug, Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct HostActor {
    uid: u32,
    primary_gid: u32,
    supplementary_gids: Vec<u32>,
    pid: u32,
}

impl HostActor {
    #[must_use]
    pub fn new(uid: u32, primary_gid: u32, supplementary_gids: Vec<u32>, pid: u32) -> Self {
        Self {
            uid,
            primary_gid,
            supplementary_gids,
            pid,
        }
    }

    #[must_use]
    pub fn uid(&self) -> u32 {
        self.uid
    }

    #[must_use]
    pub fn primary_gid(&self) -> u32 {
        self.primary_gid
    }

    #[must_use]
    pub fn supplementary_gids(&self) -> &[u32] {
        &self.supplementary_gids
    }

    #[must_use]
    pub fn pid(&self) -> u32 {
        self.pid
    }

    #[must_use]
    pub const fn is_root(&self) -> bool {
        self.uid == 0
    }

    #[must_use]
    pub fn belongs_to_group(&self, gid: u32) -> bool {
        self.primary_gid == gid || self.supplementary_gids.contains(&gid)
    }
}

/// Non-Linux user identity represented by a trusted adapter.
#[derive(Debug, Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExternalSubject {
    platform: String,
    user_id: String,
    group_id: Option<String>,
    display_name: Option<String>,
    context: SecurityContext,
}

impl ExternalSubject {
    #[must_use]
    pub fn new(
        platform: String,
        user_id: String,
        group_id: Option<String>,
        display_name: Option<String>,
        context: SecurityContext,
    ) -> Self {
        Self {
            platform,
            user_id,
            group_id,
            display_name,
            context,
        }
    }

    #[must_use]
    pub fn platform(&self) -> &str {
        &self.platform
    }

    #[must_use]
    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    #[must_use]
    pub fn group_id(&self) -> Option<&str> {
        self.group_id.as_deref()
    }

    #[must_use]
    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }

    #[must_use]
    pub fn context(&self) -> &SecurityContext {
        &self.context
    }

    #[must_use]
    pub const fn is_group_scoped(&self) -> bool {
        self.group_id.is_some()
    }
}

/// File/object classes used by the Cortex policy engine.
#[derive(Debug, Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ObjectClass {
    Space,
    Thread,
    Message,
    Request,
    Response,
    Provider,
    Model,
    SecretRef,
    CacheEntry,
    AuditLog,
    Control,
    Route,
    Policy,
    Tool,
    Skill,
    Memory,
}

/// Operations that can be authorized against an object class.
#[derive(Debug, Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Permission {
    Read,
    Write,
    Append,
    Submit,
    Cancel,
    Use,
    Configure,
    Rotate,
    Inspect,
    Export,
    Relabel,
    Delete,
    Execute,
}

impl Permission {
    #[must_use]
    pub const fn is_mutating(self) -> bool {
        matches!(
            self,
            Self::Write
                | Self::Append
                | Self::Submit
                | Self::Cancel
                | Self::Configure
                | Self::Rotate
                | Self::Relabel
                | Self::Delete
                | Self::Execute
        )
    }
}

/// Result of evaluating a policy rule set.
#[derive(Debug, Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AccessDecision {
    Allow,
    Deny { reason: String },
}

impl AccessDecision {
    #[must_use]
    pub const fn allow() -> Self {
        Self::Allow
    }

    #[must_use]
    pub fn deny(reason: impl Into<String>) -> Self {
        Self::Deny {
            reason: reason.into(),
        }
    }

    #[must_use]
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }

    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match *self {
            Self::Allow => None,
            Self::Deny { ref reason } => Some(reason.as_str()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AccessDecision, ExternalSubject, HostActor, Permission, SecurityContext};

    #[test]
    fn security_context_renders_stable_selinux_style_label() {
        let context = SecurityContext::new(
            "user_u".to_owned(),
            "agent_r".to_owned(),
            "thread_t".to_owned(),
            "s0:c42".to_owned(),
        );

        assert_eq!(context.identity(), "user_u");
        assert_eq!(context.role(), "agent_r");
        assert_eq!(context.domain(), "thread_t");
        assert_eq!(context.range(), "s0:c42");
        assert_eq!(context.to_string(), "user_u:agent_r:thread_t:s0:c42");
    }

    #[test]
    fn host_actor_preserves_kernel_request_identity() {
        let actor = HostActor::new(1000, 100, vec![10, 20], 4242);

        assert_eq!(actor.uid(), 1000);
        assert_eq!(actor.primary_gid(), 100);
        assert_eq!(actor.supplementary_gids(), [10, 20]);
        assert_eq!(actor.pid(), 4242);
        assert!(!actor.is_root());
        assert!(actor.belongs_to_group(100));
        assert!(actor.belongs_to_group(20));
        assert!(!actor.belongs_to_group(30));
    }

    #[test]
    fn external_subject_keeps_adapter_identity_separate_from_policy_context() {
        let context = SecurityContext::new(
            "qq_123".to_owned(),
            "member_r".to_owned(),
            "chat_user_t".to_owned(),
            "s0:c7".to_owned(),
        );
        let subject = ExternalSubject::new(
            "qq".to_owned(),
            "123".to_owned(),
            Some("group-456".to_owned()),
            Some("alice".to_owned()),
            context.clone(),
        );

        assert_eq!(subject.platform(), "qq");
        assert_eq!(subject.user_id(), "123");
        assert_eq!(subject.group_id(), Some("group-456"));
        assert_eq!(subject.display_name(), Some("alice"));
        assert_eq!(subject.context(), &context);
        assert!(subject.is_group_scoped());
    }

    #[test]
    fn access_decision_exposes_allow_and_deny_contract() {
        let allow = AccessDecision::allow();
        let deny = AccessDecision::deny("permission denied by policy");

        assert!(allow.is_allowed());
        assert_eq!(allow.reason(), None);
        assert!(!deny.is_allowed());
        assert_eq!(deny.reason(), Some("permission denied by policy"));
    }

    #[test]
    fn permission_marks_filesystem_mutations() {
        assert!(!Permission::Read.is_mutating());
        assert!(!Permission::Inspect.is_mutating());
        assert!(!Permission::Export.is_mutating());
        assert!(!Permission::Use.is_mutating());
        assert!(Permission::Write.is_mutating());
        assert!(Permission::Append.is_mutating());
        assert!(Permission::Submit.is_mutating());
        assert!(Permission::Configure.is_mutating());
        assert!(Permission::Rotate.is_mutating());
        assert!(Permission::Relabel.is_mutating());
        assert!(Permission::Delete.is_mutating());
        assert!(Permission::Execute.is_mutating());
    }
}
