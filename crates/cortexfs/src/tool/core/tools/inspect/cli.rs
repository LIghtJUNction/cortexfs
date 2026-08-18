use super::{MAX_FS_LIST_ENTRIES, metadata_value};
use crate::support::plain::{open_plain_directory, path_metadata_no_follow, proc_fd_path};
use serde_json::to_writer;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub(crate) fn run_fs_list_cli(args: &[OsString], writer: &mut dyn Write) -> io::Result<ExitCode> {
    let path = args
        .first()
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    let directory = open_plain_directory(&path)?;
    let entries = fs::read_dir(proc_fd_path(&directory))?;
    let mut values = entries
        .take(MAX_FS_LIST_ENTRIES)
        .map(|entry| {
            let entry = entry?;
            let metadata = entry.path().symlink_metadata()?;
            Ok(metadata_value(
                entry.file_name().to_string_lossy().as_ref(),
                &metadata,
            ))
        })
        .collect::<io::Result<Vec<_>>>()?;
    values.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
    to_writer(&mut *writer, &values)?;
    writeln!(writer)?;
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn run_fs_stat_cli(args: &[OsString], writer: &mut dyn Write) -> io::Result<ExitCode> {
    let Some(path) = args.first() else {
        return Ok(ExitCode::from(2));
    };
    let path = Path::new(path);
    let metadata = path_metadata_no_follow(path)?;
    to_writer(
        &mut *writer,
        &metadata_value(path.to_string_lossy().as_ref(), &metadata),
    )?;
    writeln!(writer)?;
    Ok(ExitCode::SUCCESS)
}
