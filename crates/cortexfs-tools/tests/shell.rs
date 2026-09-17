#[test]
fn shell_exec_cli_reports_signal_exit_code() {
    let args = [std::ffi::OsString::from("kill -TERM $$")];
    let code = cortexfs_tools::run_shell_exec_cli(&args, &mut Vec::new());
    assert!(matches!(code, Ok(code) if code == std::process::ExitCode::from(143)));
}
