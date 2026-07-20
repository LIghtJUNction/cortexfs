#[cfg(test)]
use crate::client::ToolExecution;
use crate::client::{RemoteTool, TaskSupport};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt::Write as FmtWrite;
#[cfg(test)]
use std::fs;
use std::fs::File;
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static STAGE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct ProjectInput<'a> {
    out: &'a Path,
    executable: &'a Path,
    runtime: &'a Path,
    server: &'a str,
    tools: &'a [RemoteTool],
    policy_file: Option<&'a Path>,
}

struct PreparedManifest {
    file: String,
    bytes: Vec<u8>,
}

pub(crate) fn write(
    out: &Path,
    executable: &Path,
    runtime: &Path,
    server: &str,
    tools: &[RemoteTool],
    policy_file: Option<&Path>,
) -> io::Result<Vec<PathBuf>> {
    let input = ProjectInput {
        out,
        executable,
        runtime,
        server,
        tools,
        policy_file,
    };
    write_inner(&input, |_output| Ok(()))
}

fn write_inner(
    input: &ProjectInput<'_>,
    after_first: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<Vec<PathBuf>> {
    let prepared = prepare(input)?;
    let parent_path = input
        .out
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid output"))?;
    let final_name = input
        .out
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|name| !name.is_empty() && !matches!(*name, "." | ".."))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid output"))?;
    let parent = cortexfs::support::plain::open_plain_directory(parent_path)?;
    require_absent(&parent, final_name)?;
    let parent_identity = identity(&parent)?;
    let (stage_name, stage) = create_stage(&parent)?;
    let mut files = Vec::new();
    let mut published = false;
    let mut after_first = Some(after_first);
    let publish = (|| {
        for manifest in &prepared {
            cortexfs::support::plain::write_file_atomic_at(
                &stage,
                &manifest.file,
                &manifest.bytes,
                0o644,
            )?;
            files.push(manifest.file.clone());
            if files.len() == 1
                && let Some(after_first) = after_first.take()
            {
                after_first(input.out)?;
            }
        }
        stage.sync_all()?;
        require_parent(parent_path, parent_identity)?;
        require_absent(&parent, final_name)?;
        rename_noreplace(&parent, &stage_name, final_name)?;
        published = true;
        parent.sync_all()?;
        require_parent(parent_path, parent_identity)?;
        require_visible(input.out, &stage)?;
        Ok(prepared
            .iter()
            .map(|manifest| input.out.join(&manifest.file))
            .collect())
    })();
    if publish.is_err() {
        let entry = if published { final_name } else { &stage_name };
        cleanup_stage(&parent, &stage, entry, &files);
    }
    publish
}

fn prepare(input: &ProjectInput<'_>) -> io::Result<Vec<PreparedManifest>> {
    if !input.runtime.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "runtime config must be absolute",
        ));
    }
    let policy = match input.policy_file {
        Some(path) => cortexfs::support::plain::read_small_text_file(path, 64 * 1024)?,
        None => String::new(),
    };
    let digest = digest(input.executable)?;
    let mut names = BTreeSet::new();
    let mut prepared = Vec::with_capacity(input.tools.len());
    for tool in input.tools {
        let name = format!("{}.{}", input.server, tool.name);
        if name.len() > 64 || !cortexfs::is_object_name(&name) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid projected name: {name}"),
            ));
        }
        if !names.insert(name.clone()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("duplicate projected name: {name}"),
            ));
        }
        if tool.execution.task_support == TaskSupport::Required {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsupported MCP task execution required by tool: {name}"),
            ));
        }
        let locator = json!({"transport":"stdio","config":input.runtime,"server":input.server,"tool":tool.name});
        let manifest = json!({"schema":"cortexfs.object/v2","version":env!("CARGO_PKG_VERSION"),"compatibility":{"cortexfs":">=0.1.7, <0.2.0"},"class":"tool","name":name,"executable":{"path":input.executable,"sha256":digest},"controls":{"description":tool.description,"schema":serde_json::to_string(&tool.schema).map_err(io::Error::other)?,"cap":"mcp","policy":policy,"mcp":serde_json::to_string(&locator).map_err(io::Error::other)?}});
        prepared.push(PreparedManifest {
            file: format!("{name}.manifest.json"),
            bytes: serde_json::to_vec_pretty(&manifest).map_err(io::Error::other)?,
        });
    }
    Ok(prepared)
}

fn create_stage(parent: &File) -> io::Result<(String, File)> {
    for _attempt in 0..64 {
        let sequence = STAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = format!(".ctxmcp-project-{}-{sequence}", std::process::id());
        match nix::sys::stat::mkdirat(
            parent,
            name.as_str(),
            nix::sys::stat::Mode::from_bits_truncate(0o755),
        ) {
            Ok(()) => {
                let opened = nix::fcntl::openat(
                    parent,
                    name.as_str(),
                    nix::fcntl::OFlag::O_DIRECTORY
                        | nix::fcntl::OFlag::O_RDONLY
                        | nix::fcntl::OFlag::O_NOFOLLOW
                        | nix::fcntl::OFlag::O_CLOEXEC,
                    nix::sys::stat::Mode::empty(),
                )
                .map(File::from)
                .map_err(io::Error::from);
                return match opened {
                    Ok(stage) => Ok((name, stage)),
                    Err(error) => {
                        let _removed = nix::unistd::unlinkat(
                            parent,
                            name.as_str(),
                            nix::unistd::UnlinkatFlags::RemoveDir,
                        );
                        Err(error)
                    }
                };
            }
            Err(nix::errno::Errno::EEXIST) => {}
            Err(error) => return Err(io::Error::from(error)),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "cannot allocate project staging directory",
    ))
}

fn require_absent(parent: &File, name: &str) -> io::Result<()> {
    match nix::sys::stat::fstatat(parent, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(_metadata) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "project output exists",
        )),
        Err(nix::errno::Errno::ENOENT) => Ok(()),
        Err(error) => Err(io::Error::from(error)),
    }
}

fn rename_noreplace(parent: &File, from: &str, to: &str) -> io::Result<()> {
    nix::fcntl::renameat2(
        parent,
        from,
        parent,
        to,
        nix::fcntl::RenameFlags::RENAME_NOREPLACE,
    )
    .map_err(|error| {
        if error == nix::errno::Errno::EEXIST {
            io::Error::new(io::ErrorKind::AlreadyExists, "project output exists")
        } else {
            io::Error::from(error)
        }
    })
}

fn identity(directory: &File) -> io::Result<(u64, u64)> {
    let metadata = directory.metadata()?;
    Ok((metadata.dev(), metadata.ino()))
}

fn require_parent(path: &Path, expected: (u64, u64)) -> io::Result<()> {
    let visible = cortexfs::support::plain::open_plain_directory(path)?;
    if identity(&visible)? == expected {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "project output parent changed during publication",
        ))
    }
}

fn require_visible(path: &Path, held: &File) -> io::Result<()> {
    let visible = cortexfs::support::plain::open_plain_directory(path)?;
    if identity(&visible)? == identity(held)? {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "project output directory changed during publication",
        ))
    }
}

fn cleanup_stage(parent: &File, stage: &File, entry: &str, files: &[String]) {
    for file in files.iter().rev() {
        let _removed = nix::unistd::unlinkat(
            stage,
            file.as_str(),
            nix::unistd::UnlinkatFlags::NoRemoveDir,
        );
    }
    let _synced = stage.sync_all();
    let stage_identity = identity(stage).ok();
    let entry_identity =
        nix::sys::stat::fstatat(parent, entry, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
            .ok()
            .map(|metadata| (metadata.st_dev, metadata.st_ino));
    if stage_identity.is_some() && stage_identity == entry_identity {
        let _removed = nix::unistd::unlinkat(parent, entry, nix::unistd::UnlinkatFlags::RemoveDir);
    }
    let _synced = parent.sync_all();
}

fn digest(path: &Path) -> io::Result<String> {
    let mut file = cortexfs::support::plain::open_plain_file(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "executable is not plain",
        ));
    }
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hash.update(buffer.get(..read).unwrap_or_default());
    }
    let mut text = String::with_capacity(64);
    for byte in hash.finalize() {
        write!(text, "{byte:02x}").map_err(io::Error::other)?;
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;

    fn executable(root: &Path) -> io::Result<PathBuf> {
        let path = root.join("fixture");
        fs::write(&path, b"#!/bin/sh\nexit 0\n")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
        Ok(path)
    }

    #[test]
    fn project_writes_strict_manifest_and_refuses_overwrite() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let out = root.path().join("out");
        let tools = [RemoteTool {
            name: "echo".to_owned(),
            description: "Echo".to_owned(),
            schema: json!({"type":"object"}),
            execution: ToolExecution::default(),
        }];
        let executable = executable(root.path())?;
        let paths = write(
            &out,
            &executable,
            Path::new("/visible/mcp.json"),
            "demo",
            &tools,
            None,
        )?;
        let value: serde_json::Value = serde_json::from_slice(&fs::read(
            paths
                .first()
                .ok_or_else(|| io::Error::other("missing manifest"))?,
        )?)?;
        assert_eq!(
            value.get("name").and_then(serde_json::Value::as_str),
            Some("demo.echo")
        );
        assert!(
            cortexfs::object::install::check_object(
                paths
                    .first()
                    .ok_or_else(|| io::Error::other("missing manifest"))?
            )
            .is_ok()
        );
        let source = root.path().join("source");
        fs::create_dir_all(source.join("tool"))?;
        cortexfs::object::install::install_object(
            &source,
            paths
                .first()
                .ok_or_else(|| io::Error::other("missing manifest"))?,
            cortexfs::object::install::InstallTier::System,
        )
        .map_err(|error| io::Error::other(error.message()))?;
        let found = cortexfs::ToolPath::new([source.join("tool")])
            .find("demo.echo")
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
        assert!(found.is_some());
        assert!(source.join("tool/demo.echo.d/mcp").is_file());
        assert!(
            !String::from_utf8(fs::read(
                paths
                    .first()
                    .ok_or_else(|| io::Error::other("missing manifest"))?
            )?)
            .map_err(io::Error::other)?
            .contains("secret-value")
        );
        assert!(
            write(
                &out,
                &executable,
                Path::new("/visible/mcp.json"),
                "demo",
                &tools,
                None
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn project_rejects_invalid_combined_name() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let tools = [RemoteTool {
            name: "bad/name".to_owned(),
            description: String::new(),
            schema: json!({"type":"object"}),
            execution: ToolExecution::default(),
        }];
        assert!(
            write(
                &root.path().join("out"),
                &std::env::current_exe()?,
                Path::new("/visible/mcp.json"),
                "demo",
                &tools,
                None
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn project_rejects_symlink_output() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let target = root.path().join("target");
        fs::create_dir_all(&target)?;
        let out = root.path().join("out");
        std::os::unix::fs::symlink(&target, &out)?;
        let executable = executable(root.path())?;
        let tools = [RemoteTool {
            name: "echo".to_owned(),
            description: String::new(),
            schema: json!({"type":"object"}),
            execution: ToolExecution::default(),
        }];
        assert!(
            write(
                &out,
                &executable,
                Path::new("/visible/mcp.json"),
                "demo",
                &tools,
                None,
            )
            .is_err()
        );
        assert!(fs::read_dir(target)?.next().is_none());
        Ok(())
    }

    #[test]
    fn project_holds_dirfd_across_parent_swap() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let parent = root.path().join("parent");
        fs::create_dir_all(&parent)?;
        let out = parent.join("out");
        let displaced = root.path().join("displaced");
        let executable = executable(root.path())?;
        let tools = [RemoteTool {
            name: "echo".to_owned(),
            description: String::new(),
            schema: json!({"type":"object"}),
            execution: ToolExecution::default(),
        }];
        let result = write_inner(
            &ProjectInput {
                out: &out,
                executable: &executable,
                runtime: Path::new("/visible/mcp.json"),
                server: "demo",
                tools: &tools,
                policy_file: None,
            },
            |_output| {
                fs::rename(&parent, &displaced)?;
                fs::create_dir_all(&parent)
            },
        );
        assert!(result.is_err());
        assert!(fs::read_dir(&parent)?.next().is_none());
        assert!(fs::read_dir(&displaced)?.next().is_none());
        assert!(!out.exists());
        Ok(())
    }

    #[test]
    fn project_never_replaces_raced_output_symlink() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let out = root.path().join("out");
        let external = root.path().join("external");
        fs::write(&external, "unchanged")?;
        let executable = executable(root.path())?;
        let tools = [RemoteTool {
            name: "echo".to_owned(),
            description: String::new(),
            schema: json!({"type":"object"}),
            execution: ToolExecution::default(),
        }];
        let result = write_inner(
            &ProjectInput {
                out: &out,
                executable: &executable,
                runtime: Path::new("/visible/mcp.json"),
                server: "demo",
                tools: &tools,
                policy_file: None,
            },
            |output| std::os::unix::fs::symlink(&external, output),
        );
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&external)?, "unchanged");
        assert!(fs::symlink_metadata(out)?.file_type().is_symlink());
        Ok(())
    }

    #[test]
    fn staged_batch_failure_leaves_no_output_and_retry_succeeds() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let out = root.path().join("out");
        let executable = executable(root.path())?;
        let tools = [
            RemoteTool {
                name: "echo".to_owned(),
                description: String::new(),
                schema: json!({"type":"object"}),
                execution: ToolExecution::default(),
            },
            RemoteTool {
                name: "sum".to_owned(),
                description: String::new(),
                schema: json!({"type":"object"}),
                execution: ToolExecution::default(),
            },
        ];
        let input = ProjectInput {
            out: &out,
            executable: &executable,
            runtime: Path::new("/visible/mcp.json"),
            server: "demo",
            tools: &tools,
            policy_file: None,
        };
        let failed = write_inner(&input, |_output| Err(io::Error::other("staged fault")));
        assert!(failed.is_err());
        assert!(!out.exists());
        assert!(!fs::read_dir(root.path())?.any(|entry| {
            entry.is_ok_and(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".ctxmcp-project-")
            })
        }));

        let paths = write(
            &out,
            &executable,
            Path::new("/visible/mcp.json"),
            "demo",
            &tools,
            None,
        )?;
        assert_eq!(paths.len(), 2);
        assert!(paths.iter().all(|path| path.is_file()));
        Ok(())
    }

    #[test]
    fn required_task_support_fails_before_output_creation() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let out = root.path().join("out");
        let executable = executable(root.path())?;
        let mut required = RemoteTool {
            name: "required".to_owned(),
            description: String::new(),
            schema: json!({"type":"object"}),
            execution: ToolExecution::default(),
        };
        required.execution.task_support = TaskSupport::Required;
        let error = write(
            &out,
            &executable,
            Path::new("/visible/mcp.json"),
            "demo",
            &[required],
            None,
        )
        .err()
        .ok_or_else(|| io::Error::other("required task support was accepted"))?;
        assert!(error.to_string().contains("unsupported MCP task execution"));
        assert!(!out.exists());
        Ok(())
    }

    #[test]
    fn forbidden_and_optional_task_support_project_normally() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let out = root.path().join("out");
        let executable = executable(root.path())?;
        let forbidden = RemoteTool {
            name: "forbidden".to_owned(),
            description: String::new(),
            schema: json!({"type":"object"}),
            execution: ToolExecution::default(),
        };
        let mut optional = RemoteTool {
            name: "optional".to_owned(),
            description: String::new(),
            schema: json!({"type":"object"}),
            execution: ToolExecution::default(),
        };
        optional.execution.task_support = TaskSupport::Optional;

        let paths = write(
            &out,
            &executable,
            Path::new("/visible/mcp.json"),
            "demo",
            &[forbidden, optional],
            None,
        )?;
        assert_eq!(paths.len(), 2);
        assert!(paths.iter().all(|path| path.is_file()));
        Ok(())
    }
}
