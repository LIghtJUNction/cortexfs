//! Host-enforced cgroup quotas for CortexFS agent units.
//!
//! Transient `systemd-run --user` terminals and packaged agent runtimes share
//! these ceilings so one agent cannot exhaust host memory, CPU, or tasks.

/// Hard memory ceiling for one interactive agent terminal cgroup.
pub const AGENT_TERMINAL_MEMORY_MAX: &str = "1G";

/// Soft memory pressure threshold for one interactive agent terminal.
pub const AGENT_TERMINAL_MEMORY_HIGH: &str = "768M";

/// CPU quota for one interactive agent terminal (`200%` = two cores).
pub const AGENT_TERMINAL_CPU_QUOTA: &str = "200%";

/// Max tasks (threads/processes) inside one agent terminal cgroup.
pub const AGENT_TERMINAL_TASKS_MAX: &str = "256";

/// Open-file ceiling for one agent terminal cgroup.
pub const AGENT_TERMINAL_LIMIT_NOFILE: &str = "1024";

/// Hard memory ceiling for one socket-activated agent runtime.
pub const AGENT_RUNTIME_MEMORY_MAX: &str = "512M";

/// CPU quota for one socket-activated agent runtime.
pub const AGENT_RUNTIME_CPU_QUOTA: &str = "100%";

/// Max tasks inside one socket-activated agent runtime cgroup.
pub const AGENT_RUNTIME_TASKS_MAX: &str = "128";

/// Open-file ceiling for one socket-activated agent runtime.
pub const AGENT_RUNTIME_LIMIT_NOFILE: &str = "1024";

/// Max concurrently running user agent terminal units per operator.
pub const MAX_USER_AGENT_TERMINALS: u32 = 8;

/// `systemd-run --property=` args for an interactive agent terminal.
#[must_use]
pub fn agent_terminal_properties() -> Vec<String> {
    vec![
        format!("--property=MemoryMax={AGENT_TERMINAL_MEMORY_MAX}"),
        format!("--property=MemoryHigh={AGENT_TERMINAL_MEMORY_HIGH}"),
        format!("--property=CPUQuota={AGENT_TERMINAL_CPU_QUOTA}"),
        format!("--property=TasksMax={AGENT_TERMINAL_TASKS_MAX}"),
        format!("--property=LimitNOFILE={AGENT_TERMINAL_LIMIT_NOFILE}"),
        "--property=OOMPolicy=stop".to_owned(),
        "--property=MemoryAccounting=yes".to_owned(),
        "--property=CPUAccounting=yes".to_owned(),
        "--property=TasksAccounting=yes".to_owned(),
    ]
}

/// `systemd-run --property=` args for a socket-activated agent runtime.
#[must_use]
pub fn agent_runtime_properties() -> Vec<String> {
    vec![
        format!("--property=MemoryMax={AGENT_RUNTIME_MEMORY_MAX}"),
        format!("--property=CPUQuota={AGENT_RUNTIME_CPU_QUOTA}"),
        format!("--property=TasksMax={AGENT_RUNTIME_TASKS_MAX}"),
        format!("--property=LimitNOFILE={AGENT_RUNTIME_LIMIT_NOFILE}"),
        "--property=OOMPolicy=stop".to_owned(),
        "--property=MemoryAccounting=yes".to_owned(),
        "--property=CPUAccounting=yes".to_owned(),
        "--property=TasksAccounting=yes".to_owned(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_properties_pin_memory_cpu_and_tasks() {
        let args = agent_terminal_properties();
        assert!(args.iter().any(|a| a.contains("MemoryMax=1G")));
        assert!(args.iter().any(|a| a.contains("CPUQuota=200%")));
        assert!(args.iter().any(|a| a.contains("TasksMax=256")));
        assert!(args.iter().any(|a| a == "--property=OOMPolicy=stop"));
    }

    #[test]
    fn runtime_properties_are_stricter_than_terminal() {
        let args = agent_runtime_properties();
        assert!(args.iter().any(|a| a.contains("MemoryMax=512M")));
        assert!(args.iter().any(|a| a.contains("CPUQuota=100%")));
        assert!(args.iter().any(|a| a.contains("TasksMax=128")));
    }
}
