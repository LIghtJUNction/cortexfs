use super::*;
use crate::*;

use crate::plain_fs::{
    open_plain_directory as open_tool_io_plain_directory,
    plain_file_name as tool_io_plain_file_name,
    read_small_text_file as read_tool_io_small_text_file,
};

impl Tool for FsReadTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fs.read",
            description: "Read a UTF-8 text file from the visible filesystem.",
            input_schema: FS_READ_SCHEMA,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let path = invocation
            .string_field("path")
            .unwrap_or_else(|| invocation.input().trim().to_owned());
        if path.is_empty() {
            return Err(ToolError::invalid("missing path"));
        }
        match read_regular_utf8_file(Path::new(&path), MAX_FS_READ_BYTES) {
            Ok(content) => output
                .message(&content)
                .map_err(|error| ToolError::new("EIO", error.to_string())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Err(ToolError::not_found("file not found"))
            }
            Err(_error) => Err(ToolError::denied("read failed")),
        }
    }
}

impl Tool for FsWriteTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fs.write",
            description: "Write UTF-8 text to a path in the visible filesystem.",
            input_schema: FS_WRITE_SCHEMA,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let path = invocation.string_field("path").unwrap_or_default();
        let content = invocation.string_field("content").unwrap_or_default();
        if path.is_empty() {
            return Err(ToolError::invalid("missing path"));
        }
        write_text_file_atomic(Path::new(&path), &content)
            .map_err(|_error| ToolError::denied("write failed"))?;
        output
            .message("written")
            .map_err(|error| ToolError::new("EIO", error.to_string()))
    }
}

impl Tool for FsReplaceTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fs.replace",
            description: "Replace exactly one UTF-8 text span in a visible file.",
            input_schema: FS_REPLACE_SCHEMA,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let path = invocation.string_field("path").unwrap_or_default();
        let old = invocation.string_field("old").unwrap_or_default();
        let new = invocation.string_field("new").unwrap_or_default();
        if path.is_empty() {
            return Err(ToolError::invalid("missing path"));
        }
        replace_exactly_once(Path::new(&path), &old, &new)
            .map_err(|error| fs_replace_tool_error(&error))?;
        output
            .message("replaced")
            .map_err(|error| ToolError::new("EIO", error.to_string()))
    }
}

pub(crate) fn run_fs_read_cli(args: &[OsString], writer: &mut dyn Write) -> io::Result<ExitCode> {
    let Some(path) = args.first() else {
        writeln!(io::stderr(), "fs.read: missing path")?;
        return Ok(ExitCode::from(2));
    };
    let content = read_regular_utf8_file(&PathBuf::from(path), MAX_FS_READ_BYTES)?;
    writer.write_all(content.as_bytes())?;
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn read_regular_utf8_file(path: &Path, max_bytes: u64) -> io::Result<String> {
    read_tool_io_small_text_file(path, max_bytes)
}

pub(crate) fn run_fs_write_cli(args: &[OsString], writer: &mut dyn Write) -> io::Result<ExitCode> {
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
        read_text_from_stdin_limited(io::stdin(), MAX_FUSE_V1_SMALL_WRITE_BYTES)?
    };
    write_text_file_atomic(Path::new(path), &content)?;
    writeln!(writer, "written")?;
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn run_fs_replace_cli(
    args: &[OsString],
    writer: &mut dyn Write,
) -> io::Result<ExitCode> {
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
        old.to_string_lossy().as_ref(),
        new.to_string_lossy().as_ref(),
    )?;
    writeln!(writer, "replaced")?;
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn read_text_from_stdin_limited(
    reader: impl Read,
    max_bytes: usize,
) -> io::Result<String> {
    let limit = u64::try_from(max_bytes.saturating_add(1)).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("stdin read limit is invalid: {error}"),
        )
    })?;
    let mut content = String::new();
    reader.take(limit).read_to_string(&mut content)?;
    if content.len() > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "stdin exceeds fs.write input limit",
        ));
    }
    Ok(content)
}

pub(crate) fn replace_exactly_once(path: &Path, old: &str, new: &str) -> io::Result<()> {
    if old.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "old text must not be empty",
        ));
    }
    let content = read_regular_utf8_file(path, MAX_FS_READ_BYTES)?;
    let mut matches = content.match_indices(old);
    let Some((_start, _matched)) = matches.next() else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "old text not found",
        ));
    };
    if matches.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "old text occurs more than once",
        ));
    }
    let updated = content.replacen(old, new, 1);
    write_text_file_atomic(path, &updated)
}

pub(crate) fn fs_replace_tool_error(error: &io::Error) -> ToolError {
    match error.kind() {
        io::ErrorKind::NotFound => ToolError::not_found(error.to_string()),
        io::ErrorKind::InvalidInput => ToolError::invalid(error.to_string()),
        _ => ToolError::denied("replace failed"),
    }
}

pub(crate) fn write_text_file_atomic(path: &Path, content: &str) -> io::Result<()> {
    let Some(parent) = path.parent() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path must have a parent directory",
        ));
    };
    let file_name = tool_io_plain_file_name(path)?;
    let parent_dir = open_tool_io_plain_directory(parent)?;
    for attempt in 0..16 {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let tmp_name = format!(".{file_name}.tmp-{}-{nonce}-{attempt}", std::process::id());
        match nix::fcntl::openat(
            &parent_dir,
            tmp_name.as_str(),
            nix::fcntl::OFlag::O_CREAT
                | nix::fcntl::OFlag::O_EXCL
                | nix::fcntl::OFlag::O_WRONLY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::from_bits_truncate(0o644),
        ) {
            Ok(fd) => {
                let mut file = fs::File::from(fd);
                if let Err(error) = file.write_all(content.as_bytes()) {
                    let _ignored = nix::unistd::unlinkat(
                        &parent_dir,
                        tmp_name.as_str(),
                        nix::unistd::UnlinkatFlags::NoRemoveDir,
                    );
                    return Err(error);
                }
                if let Err(error) = file.sync_all() {
                    let _ignored = nix::unistd::unlinkat(
                        &parent_dir,
                        tmp_name.as_str(),
                        nix::unistd::UnlinkatFlags::NoRemoveDir,
                    );
                    return Err(error);
                }
                drop(file);
                if let Err(error) =
                    nix::fcntl::renameat(&parent_dir, tmp_name.as_str(), &parent_dir, file_name)
                {
                    let _ignored = nix::unistd::unlinkat(
                        &parent_dir,
                        tmp_name.as_str(),
                        nix::unistd::UnlinkatFlags::NoRemoveDir,
                    );
                    return Err(io::Error::from(error));
                }
                return parent_dir.sync_all();
            }
            Err(nix::errno::Errno::EEXIST) => {}
            Err(error) => return Err(io::Error::from(error)),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "cannot create unique temp file",
    ))
}
