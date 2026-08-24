use crate::{mount::table::MountTable, policy::PolicyEvaluator};

/// Effective Unix identity used by authority access decisions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentUnixIdentity {
    uid: u32,
    gid: u32,
    groups: Vec<u32>,
}

impl AgentUnixIdentity {
    /// Creates an identity from uid, primary gid, and supplementary groups.
    #[must_use]
    pub fn new(uid: u32, gid: u32, groups: impl IntoIterator<Item = u32>) -> Self {
        Self {
            uid,
            gid,
            groups: groups.into_iter().collect(),
        }
    }

    /// Returns the runtime uid.
    #[must_use]
    pub const fn uid(&self) -> u32 {
        self.uid
    }

    /// Returns the runtime primary gid.
    #[must_use]
    pub const fn gid(&self) -> u32 {
        self.gid
    }

    /// Returns supplementary groups.
    #[must_use]
    pub fn groups(&self) -> &[u32] {
        &self.groups
    }

    pub(crate) fn is_in_group(&self, gid: u32) -> bool {
        self.gid == gid || self.groups.contains(&gid)
    }
}

/// Inputs that define an agent's effective authority for an access decision.
#[derive(Clone, Copy, Debug)]
pub struct AccessAuthority<'a> {
    pub(crate) identity: &'a AgentUnixIdentity,
    pub(crate) mount_table: &'a MountTable,
    pub(crate) agent_subject: &'a str,
    pub(crate) policy: &'a dyn PolicyEvaluator,
}

impl<'a> AccessAuthority<'a> {
    /// Creates an effective authority context for one access decision.
    #[must_use]
    pub const fn new(
        identity: &'a AgentUnixIdentity,
        mount_table: &'a MountTable,
        agent_subject: &'a str,
        policy: &'a dyn PolicyEvaluator,
    ) -> Self {
        Self {
            identity,
            mount_table,
            agent_subject,
            policy,
        }
    }
}

/// Effective authority context for shared-space access.
pub type SharedAccessAuthority<'a> = AccessAuthority<'a>;

/// Effective authority context for durable session access.
pub type SessionAccessAuthority<'a> = AccessAuthority<'a>;
