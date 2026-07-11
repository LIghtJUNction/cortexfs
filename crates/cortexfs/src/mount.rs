pub(crate) mod driver;
pub mod table;

/// Runs the installed mount driver.
#[doc(hidden)]
#[must_use]
pub fn main() -> std::process::ExitCode {
    driver::main()
}
