use crate::atomic::write_text_file_atomic;
use crate::input::read_text_from_stdin_limited;
use crate::read::read_small_text_file;
use crate::replace::replace_exactly_once;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

pub fn run_fs_read_cli(args: &[OsString], writer: &mut dyn Write) -> io::Result<ExitCode> {
    let Some(path) = args.first() else {
        writeln!(io::stderr(), "fs.read: missing path")?;
        return Ok(ExitCode::from(2));
    };
    writer
        .write_all(read_small_text_file(Path::new(path), crate::MAX_FS_READ_BYTES)?.as_bytes())?;
    Ok(ExitCode::SUCCESS)
}

pub fn run_fs_write_cli(args: &[OsString], writer: &mut dyn Write) -> io::Result<ExitCode> {
    let Some(path) = args.first() else {
        writeln!(io::stderr(), "fs.write: missing path")?;
        return Ok(ExitCode::from(2));
    };
    let content = if args.len() > 1 {
        args.iter()
            .skip(1)
            .map(|value| value.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        read_text_from_stdin_limited(io::stdin(), crate::MAX_FS_WRITE_BYTES)?
    };
    write_text_file_atomic(Path::new(path), &content)?;
    writeln!(writer, "written")?;
    Ok(ExitCode::SUCCESS)
}

pub fn run_fs_replace_cli(args: &[OsString], writer: &mut dyn Write) -> io::Result<ExitCode> {
    let Some(path) = args.first() else {
        writeln!(io::stderr(), "fs.replace: missing path")?;
        return Ok(ExitCode::from(2));
    };
    let Some(old) = args.get(1) else {
        writeln!(io::stderr(), "fs.replace: missing old text")?;
        return Ok(ExitCode::from(2));
    };
    let Some(new) = args.get(2) else {
        writeln!(io::stderr(), "fs.replace: missing new text")?;
        return Ok(ExitCode::from(2));
    };
    replace_exactly_once(
        Path::new(path),
        &old.to_string_lossy(),
        &new.to_string_lossy(),
    )?;
    writeln!(writer, "replaced")?;
    Ok(ExitCode::SUCCESS)
}
