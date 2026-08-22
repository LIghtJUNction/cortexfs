use std::os::unix::net::UnixStream;

use crate::support::proc::{process_in_unit, read_process_cgroup, read_process_stat};

use super::BrokerProtocolError;

pub(super) struct PeerIdentity {
    pub(super) uid: u32,
    pub(super) generation: String,
    cgroup: String,
}

impl PeerIdentity {
    pub(super) fn read(stream: &UnixStream) -> Result<Self, BrokerProtocolError> {
        let credentials =
            crate::peer_credentials(stream).map_err(|_error| BrokerProtocolError::Protocol)?;
        let pid = credentials
            .pid()
            .and_then(|pid| u32::try_from(pid).ok())
            .filter(|pid| *pid != 0)
            .ok_or(BrokerProtocolError::Protocol)?;
        let start = read_process_stat(pid).ok_or(BrokerProtocolError::Protocol)?;
        Ok(Self {
            uid: credentials.uid(),
            generation: format!("{pid}:{}", start.start_time),
            cgroup: read_process_cgroup(pid).ok_or(BrokerProtocolError::Protocol)?,
        })
    }

    pub(super) fn authorize_supervisor(
        &self,
        agent: &str,
        session: &str,
        unit: &str,
    ) -> Result<(), BrokerProtocolError> {
        validate_names(agent, session)?;
        if unit != crate::agent::launch::agent_terminal_unit(agent, session)
            || !process_in_unit(&self.cgroup, unit)
        {
            return Err(rejected(
                "supervisor_identity",
                "supervisor cgroup does not match terminal unit",
            ));
        }
        Ok(())
    }

    pub(super) fn authorize_operator(&self) -> Result<(), BrokerProtocolError> {
        if self.cgroup.contains("/cortexfs-agent-") {
            return Err(rejected(
                "operator_identity",
                "terminal clients must be outside agent cgroups",
            ));
        }
        Ok(())
    }
}

pub(super) fn validate_names(agent: &str, session: &str) -> Result<(), BrokerProtocolError> {
    if crate::is_object_name(agent) && crate::is_object_name(session) {
        return Ok(());
    }
    Err(rejected(
        "invalid_name",
        "agent and session must be object names",
    ))
}

fn rejected(code: &str, message: &str) -> BrokerProtocolError {
    BrokerProtocolError::Rejected(code.into(), message.into())
}
