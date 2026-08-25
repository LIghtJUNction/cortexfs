use super::*;

#[test]
fn bare_ctx_starts_a_new_session() {
    assert!(matches!(parse_command(Vec::new()), Ok(Command::NewSession)));
}

#[test]
fn resume_accepts_default_and_explicit_forms() {
    assert!(matches!(
        parse_command(vec!["resume".to_owned()]),
        Ok(Command::Resume {
            agent: None,
            session: None
        })
    ));
    assert!(matches!(
        parse_command(vec![
            "resume".to_owned(),
            "executor".to_owned(),
            "--session".to_owned(),
            "work".to_owned()
        ]),
        Ok(Command::Resume {
            agent: Some(agent),
            session: Some(session)
        }) if agent == "executor" && session == "work"
    ));
}

#[test]
fn status_and_help_are_explicit_commands() {
    assert!(matches!(
        parse_command(vec!["status".to_owned()]),
        Ok(Command::Status)
    ));
    assert!(matches!(
        parse_command(vec!["--help".to_owned()]),
        Ok(Command::Help)
    ));
    assert!(matches!(
        parse_command(vec!["help".to_owned()]),
        Ok(Command::Help)
    ));
}
