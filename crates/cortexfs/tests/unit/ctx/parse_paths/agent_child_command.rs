#[test]
fn agent_start_parses_exact_child_argv_after_delimiter() {
    let start = cmd!(
        "agent",
        "start",
        "reviewer",
        "--session",
        "test",
        "--",
        "/usr/bin/fake-agent",
        "--json",
        "turn one",
    );
    assert!(matches!(
        start,
        Ok(Command::Agent(AgentArgs::Start(ref args)))
            if args.name == "reviewer"
                && args.session == "test"
                && args.command == ["/usr/bin/fake-agent", "--json", "turn one"]
    ));
}

#[test]
fn agent_start_rejects_empty_child_delimiter() {
    assert!(cmd!("agent", "start", "reviewer", "--").is_err());
}
