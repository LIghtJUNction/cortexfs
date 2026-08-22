mod check;
mod control;
mod install;
mod manifest;
mod object;
mod source;
mod write;

pub(crate) use install::{parse_package_install_command, run_package_install};

#[cfg(test)]
mod tests;
