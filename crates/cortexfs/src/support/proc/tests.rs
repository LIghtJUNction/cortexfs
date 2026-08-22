use super::*;

#[test]
fn process_stat_handles_spaces_and_rejects_zombies() {
    let stat = "42 (worker ) with spaces) S 7 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 1234 0";
    assert_eq!(
        parse_process_stat(stat),
        Some(ProcessStat {
            parent: 7,
            start_time: 1234,
        })
    );
    assert_eq!(parse_process_stat("42 (worker) Z 7 0"), None);
}

#[test]
fn procfs_reader_handles_zero_length_metadata_files() {
    let pid = std::process::id();
    assert!(read_process_stat(pid).is_some_and(|stat| stat.start_time > 0));
    assert!(read_process_cgroup(pid).is_some_and(|cgroup| !cgroup.is_empty()));
}

#[test]
fn cgroup_unit_match_requires_a_service_path_component() {
    let valid = "0::/user.slice/user-1000.slice/app.slice/cortexfs-agent-a-s-terminal.service\n";
    assert!(process_in_unit(valid, "cortexfs-agent-a-s-terminal"));
    assert!(!process_in_unit(valid, "agent-a"));
    assert!(!process_in_unit(
        "0::/x/cortexfs-agent-a-s-terminal.service.fake",
        "cortexfs-agent-a-s-terminal"
    ));
}
