pub mod bootstrap;
pub(crate) mod executor;
pub mod install;
pub mod layout;
pub mod metadata;
pub mod receipt;
pub mod residue;
pub(crate) mod runner;
pub mod uninstall;

/// Runs the installed object executor.
#[doc(hidden)]
#[must_use]
pub fn runner_main() -> std::process::ExitCode {
    executor::main()
}
