use crate::{PolicyEvaluator, PolicyObjectClass, PolicyPermission, is_object_name};

/// Policy input for one named network connection class.
#[derive(Clone, Copy, Debug)]
pub struct NetworkConnectAuthority<'a> {
    subject: &'a str,
    policy: &'a dyn PolicyEvaluator,
}

impl<'a> NetworkConnectAuthority<'a> {
    #[must_use]
    pub const fn new(subject: &'a str, policy: &'a dyn PolicyEvaluator) -> Self {
        Self { subject, policy }
    }
}

/// Stable network policy refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkConnectDenial {
    /// The named network class is malformed.
    InvalidNetwork,
    /// Host-loaded policy refused the connection class.
    Policy,
}

/// Authorizes a named network class before sandbox/relay construction.
pub fn authorize_network_connect(
    network: &str,
    authority: NetworkConnectAuthority<'_>,
) -> Result<(), NetworkConnectDenial> {
    if !is_object_name(network) {
        return Err(NetworkConnectDenial::InvalidNetwork);
    }
    if !authority.policy.evaluate(
        authority.subject,
        PolicyObjectClass::Network,
        network,
        PolicyPermission::Connect,
    ) {
        return Err(NetworkConnectDenial::Policy);
    }
    Ok(())
}
