use super::super::*;

pub(super) fn create_test_executable(
    class: &fs::File,
    name: &str,
) -> Result<(), InstallError> {
    let fd = nix::fcntl::openat(
        class,
        name,
        nix::fcntl::OFlag::O_CREAT
            | nix::fcntl::OFlag::O_EXCL
            | nix::fcntl::OFlag::O_WRONLY
            | nix::fcntl::OFlag::O_NOFOLLOW,
        nix::sys::stat::Mode::from_bits_truncate(0o755),
    )
    .map_err(|error| {
        InstallError::unavailable(format!("cannot create test executable: {error}"))
    })?;
    let mut file = fs::File::from(fd);
    file.write_all(b"#!/bin/sh\nprintf replacement\n")
        .map_err(|error| {
            InstallError::unavailable(format!("cannot write test executable: {error}"))
        })
}

pub(super) fn replace_published_executable_for_test(
    class: &fs::File,
    name: &str,
) -> Result<(), InstallError> {
    rename_noreplace(class, name, class, ".captured-executable").map_err(|error| {
        InstallError::unavailable(format!("cannot capture test executable: {error}"))
    })?;
    create_test_executable(class, name)
}

pub(super) fn replace_published_control_for_test(
    class: &fs::File,
    name: &str,
) -> Result<(), InstallError> {
    rename_noreplace(class, name, class, ".captured-controls").map_err(|error| {
        InstallError::unavailable(format!("cannot capture test controls: {error}"))
    })?;
    nix::sys::stat::mkdirat(class, name, nix::sys::stat::Mode::from_bits_truncate(0o755)).map_err(
        |error| InstallError::unavailable(format!("cannot create test replacement: {error}")),
    )
}

pub(super) fn replace_parked_control_for_test(stage: &fs::File) -> Result<(), InstallError> {
    rename_noreplace(
        stage,
        "rolled-back-control",
        stage,
        "captured-rolled-back-control",
    )
    .map_err(|error| {
        InstallError::unavailable(format!("cannot capture parked controls: {error}"))
    })?;
    mkdirat(stage, "rolled-back-control", 0o700)
}
