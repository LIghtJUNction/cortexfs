use super::super::*;

fn sample() -> AgentLaunchReceipt {
    AgentLaunchReceipt {
        unit: "unit".to_owned(),
        pid: 42,
        identity: AgentUnixIdentity::new(1000, 1000, []),
        invocation: "expected".to_owned(),
        socket: PathBuf::new(),
    }
}

#[test]
fn launch_receipt_rejects_invocation_and_live_pid_reuse_without_mutation() {
    let receipt = sample();
    assert_eq!(verify_launch_state(&receipt, None), Ok(false));
    assert_eq!(
        verify_launch_state(
            &receipt,
            Some(UnitState {
                pid: 42,
                invocation: "replaced".to_owned(),
                active: "active".to_owned(),
            })
        ),
        Err(AgentLaunchError::StopConflict)
    );
    assert_eq!(
        verify_launch_state(
            &receipt,
            Some(UnitState {
                pid: 99,
                invocation: "expected".to_owned(),
                active: "active".to_owned(),
            })
        ),
        Err(AgentLaunchError::StopConflict)
    );
    assert_eq!(
        verify_launch_state(
            &receipt,
            Some(UnitState {
                pid: 0,
                invocation: "expected".to_owned(),
                active: "inactive".to_owned(),
            })
        ),
        Ok(false)
    );
}
