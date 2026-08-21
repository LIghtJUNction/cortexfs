use crate::*;

#[cfg(test)]
mod tests {
    use super::{
        ClientMode, Clients, CtxtermCommand, PtyWriter, RunConfig, env_u16_from_value,
        handle_client, open_log, parse_args, pty_command_with_env, read_client_mode,
        read_client_mode_with_timeout_duration, remove_stale_socket, start_listener, token_hash,
        tokens_equal, valid_client_token,
    };
    use std::ffi::OsString;
    use std::fs;
    use std::io::{self, Read, Write};
    use std::net::Shutdown;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

    #[test]
    fn terminal_capability_rejects_missing_short_and_mismatched_tokens() {
        let token = "0123456789abcdef0123456789abcdef";
        assert!(valid_client_token(token));
        assert!(!valid_client_token(""));
        assert!(!valid_client_token("too-short"));
        assert!(tokens_equal(token.as_bytes(), token.as_bytes()));
        assert!(!tokens_equal(
            token.as_bytes(),
            b"0123456789abcdef0123456789abcdeg"
        ));
    }

    impl Write for SharedBuffer {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            {
                let mut output = self
                    .0
                    .lock()
                    .map_err(|_error| io::Error::other("buffer lock poisoned"))?;
                output.extend_from_slice(buf);
            }
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn ctxterm_defaults_to_absolute_tsh() {
        assert_eq!(
            parse_args(Vec::new()),
            Ok(CtxtermCommand::Run {
                listen: None,
                log: None,
                stdio: true,
                program: OsString::from("/usr/bin/tsh"),
                args: Vec::new()
            })
        );
    }

    #[test]
    fn ctxterm_accepts_explicit_command_after_separator() {
        assert_eq!(
            parse_args(vec![
                OsString::from("--"),
                OsString::from("tsh"),
                OsString::from("--list"),
            ]),
            Ok(CtxtermCommand::Run {
                listen: None,
                log: None,
                stdio: true,
                program: OsString::from("tsh"),
                args: vec![OsString::from("--list")]
            })
        );
    }

    #[test]
    fn ctxterm_pty_command_uses_clean_environment() {
        let config = RunConfig {
            listen: None,
            log: None,
            stdio: false,
            program: OsString::from("/usr/bin/tsh"),
            args: Vec::new(),
        };
        let command = pty_command_with_env(
            &config,
            [
                (
                    OsString::from("CORTEXFS_SHOULD_NOT_LEAK"),
                    OsString::from("secret"),
                ),
                (OsString::from("PATH"), OsString::from("/tmp/evil")),
            ],
        );
        assert!(command.is_ok());
        let Ok(command) = command else { return };
        let mut env = command
            .iter_full_env_as_str()
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect::<Vec<_>>();
        env.sort();

        assert_eq!(
            env,
            vec![
                ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
                ("TERM".to_owned(), "xterm-256color".to_owned()),
            ]
        );
    }

    #[test]
    fn ctxterm_pty_command_preserves_agent_environment() {
        let config = RunConfig {
            listen: None,
            log: None,
            stdio: false,
            program: OsString::from("/usr/bin/tsh"),
            args: Vec::new(),
        };
        let command = pty_command_with_env(
            &config,
            [
                (OsString::from("CTX_ROOT"), OsString::from("/ctx")),
                (OsString::from("CTX_HOME"), OsString::from("/ctx/home/1000")),
                (OsString::from("CTX_AGENT"), OsString::from("coder")),
                (
                    OsString::from("CTX_AGENT_SUBJECT"),
                    OsString::from("coder_t"),
                ),
                (
                    OsString::from("CTX_PATH"),
                    OsString::from("/ctx/tool:/ctx/home/1000/tool"),
                ),
                (OsString::from("HOME"), OsString::from("/home/agent")),
                (OsString::from("USER"), OsString::from("coder")),
                (OsString::from("LOGNAME"), OsString::from("coder")),
                (OsString::from("SHELL"), OsString::from("/usr/bin/bash")),
                (OsString::from("LANG"), OsString::from("C.UTF-8")),
                (OsString::from("LD_PRELOAD"), OsString::from("evil.so")),
            ],
        );
        assert!(command.is_ok());
        let Ok(command) = command else { return };
        let env = command
            .iter_full_env_as_str()
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect::<Vec<_>>();

        assert!(env.contains(&("CTX_AGENT".to_owned(), "coder".to_owned())));
        assert!(env.contains(&("CTX_ROOT".to_owned(), "/ctx".to_owned())));
        assert!(env.contains(&("CTX_HOME".to_owned(), "/ctx/home/1000".to_owned())));
        assert!(env.contains(&("CTX_AGENT_SUBJECT".to_owned(), "coder_t".to_owned())));
        assert!(env.contains(&(
            "CTX_PATH".to_owned(),
            "/ctx/tool:/ctx/home/1000/tool".to_owned()
        )));
        assert!(env.contains(&("HOME".to_owned(), "/home/agent".to_owned())));
        assert!(!env.iter().any(|entry| entry.0 == "LD_PRELOAD"));
    }

    #[test]
    fn ctxterm_env_u16_rejects_zero_and_invalid_values() {
        assert_eq!(env_u16_from_value(Some("24")), Some(24));
        assert_eq!(env_u16_from_value(Some("0")), None);
        assert_eq!(env_u16_from_value(Some("bad")), None);
        assert_eq!(env_u16_from_value(None), None);
    }

    #[test]
    fn ctxterm_parses_listen_and_clients() {
        assert_eq!(
            parse_args(vec![
                OsString::from("--listen"),
                OsString::from("/tmp/main.sock"),
                OsString::from("--no-stdio"),
                OsString::from("--"),
                OsString::from("tsh"),
            ]),
            Ok(CtxtermCommand::Run {
                listen: Some(PathBuf::from("/tmp/main.sock")),
                log: None,
                stdio: false,
                program: OsString::from("tsh"),
                args: Vec::new()
            })
        );
        assert_eq!(
            parse_args(vec![
                OsString::from("watch"),
                OsString::from("/tmp/main.sock"),
            ]),
            Ok(CtxtermCommand::Client {
                socket: PathBuf::from("/tmp/main.sock"),
                write: false,
            })
        );
        assert_eq!(
            parse_args(vec![
                OsString::from("attach"),
                OsString::from("/tmp/main.sock"),
            ]),
            Ok(CtxtermCommand::Client {
                socket: PathBuf::from("/tmp/main.sock"),
                write: true,
            })
        );
    }

    #[test]
    fn client_mode_requires_newline() -> Result<(), Box<dyn std::error::Error>> {
        let (mut client, mut server) = UnixStream::pair()?;
        client.write_all(b"watch")?;
        client.shutdown(Shutdown::Write)?;

        let Err(error) = read_client_mode(&mut server) else {
            return Err("unterminated mode must fail".into());
        };

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("must end with newline"));
        Ok(())
    }

    #[test]
    fn client_mode_keeps_attach_payload_after_newline() -> Result<(), Box<dyn std::error::Error>> {
        let (mut client, mut server) = UnixStream::pair()?;
        client.write_all(b"attach\n0123456789abcdef0123456789abcdef\npayload")?;
        client.shutdown(Shutdown::Write)?;

        let (mode, token) = read_client_mode(&mut server)?;
        let mut payload = String::new();
        server.read_to_string(&mut payload)?;

        assert_eq!(mode, ClientMode::Attach);
        assert_eq!(token, "0123456789abcdef0123456789abcdef");
        assert_eq!(payload, "payload");
        Ok(())
    }

    #[test]
    fn client_mode_accepts_emit_payload_after_newline() -> Result<(), Box<dyn std::error::Error>> {
        let (mut client, mut server) = UnixStream::pair()?;
        client.write_all(b"emit\n0123456789abcdef0123456789abcdef\npayload")?;
        client.shutdown(Shutdown::Write)?;

        let (mode, token) = read_client_mode(&mut server)?;
        let mut payload = String::new();
        server.read_to_string(&mut payload)?;

        assert_eq!(mode, ClientMode::Emit);
        assert_eq!(token, "0123456789abcdef0123456789abcdef");
        assert_eq!(payload, "payload");
        Ok(())
    }

    #[test]
    fn emit_client_broadcasts_without_writing_to_pty() -> Result<(), Box<dyn std::error::Error>> {
        let (mut emit_client, emit_server) = UnixStream::pair()?;
        let (mut watch_reader, watch_writer) = UnixStream::pair()?;
        watch_reader.set_read_timeout(Some(Duration::from_secs(1)))?;
        let pty_output = Arc::new(Mutex::new(Vec::new()));
        let writer: PtyWriter =
            Arc::new(Mutex::new(Box::new(SharedBuffer(Arc::clone(&pty_output)))));
        let clients: Clients = Arc::new(Mutex::new(vec![Arc::new(Mutex::new(watch_writer))]));

        emit_client.write_all(
            b"emit\n0123456789abcdef0123456789abcdef\n\r\ntool bash running shell.exec 'date'\r\n",
        )?;
        emit_client.shutdown(Shutdown::Write)?;
        handle_client(
            emit_server,
            writer,
            &clients,
            &token_hash("0123456789abcdef0123456789abcdef"),
        );

        let expected = b"\r\ntool bash running shell.exec 'date'\r\n";
        let mut payload = vec![0; expected.len()];
        watch_reader.read_exact(&mut payload)?;
        assert_eq!(payload, expected);
        assert!(pty_output.lock().is_ok_and(|buffer| buffer.is_empty()));
        Ok(())
    }

    #[test]
    fn client_mode_timeout_rejects_idle_client_and_restores_blocking()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_client, mut server) = UnixStream::pair()?;

        let Err(error) =
            read_client_mode_with_timeout_duration(&mut server, Duration::from_millis(1))
        else {
            return Err("idle client mode must time out".into());
        };

        assert!(matches!(
            error.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
        ));
        assert_eq!(server.read_timeout()?, None);
        Ok(())
    }

    #[test]
    fn remove_stale_socket_refuses_symlink_without_touching_target()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("target.txt");
        let link = dir.path().join("session.sock");
        fs::write(&target, "keep me")?;
        symlink(&target, &link)?;

        let Err(error) = remove_stale_socket(&link) else {
            return Err("symlinks are refused".into());
        };

        assert!(matches!(
            error.kind(),
            io::ErrorKind::AlreadyExists | io::ErrorKind::NotADirectory
        ));
        assert_eq!(fs::read_to_string(&target)?, "keep me");
        assert!(link.is_symlink());
        Ok(())
    }

    #[test]
    fn remove_stale_socket_rejects_symlink_parent_without_removing_target_socket()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        let outside_socket = outside.path().join("session.sock");
        let listener = UnixListener::bind(&outside_socket)?;
        let link = dir.path().join("runtime");
        symlink(outside.path(), &link)?;

        let Err(error) = remove_stale_socket(&link.join("session.sock")) else {
            return Err("symlink parent must fail".into());
        };

        assert!(matches!(
            error.kind(),
            io::ErrorKind::AlreadyExists | io::ErrorKind::NotADirectory
        ));
        assert!(outside_socket.exists());
        drop(listener);
        Ok(())
    }

    #[test]
    fn remove_stale_socket_only_removes_socket_inodes() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let socket = dir.path().join("session.sock");
        let listener = UnixListener::bind(&socket)?;
        drop(listener);

        remove_stale_socket(&socket)?;

        assert!(!socket.exists());
        Ok(())
    }

    #[test]
    fn open_log_refuses_symlink_without_touching_target() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("target.log");
        let link = dir.path().join("session.log");
        fs::write(&target, "keep me")?;
        symlink(&target, &link)?;

        let Err(error) = open_log(&link) else {
            return Err("symlink log path must fail".into());
        };

        assert_eq!(error.code, 69);
        assert_eq!(fs::read_to_string(&target)?, "keep me");
        assert!(link.is_symlink());
        Ok(())
    }

    #[test]
    fn open_log_appends_plain_files() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("session.log");
        let Ok(mut file) = open_log(&path) else {
            return Err("plain log path should open".into());
        };
        file.write_all(b"hello")?;
        drop(file);

        let Ok(mut file) = open_log(&path) else {
            return Err("existing plain log path should open".into());
        };
        file.write_all(b" world")?;
        drop(file);

        assert_eq!(fs::read_to_string(&path)?, "hello world");
        assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
        Ok(())
    }

    #[test]
    fn open_log_rejects_symlink_parent_directory() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        let parent = dir.path().join("logs");
        symlink(outside.path(), &parent)?;
        let path = parent.join("session.log");

        let Err(error) = open_log(&path) else {
            return Err("symlink parent must fail".into());
        };

        assert_eq!(error.code, 69);
        assert!(!outside.path().join("session.log").exists());
        assert!(parent.is_symlink());
        Ok(())
    }

    #[test]
    fn open_log_rejects_symlink_intermediate_parent_with_existing_target_dirs()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        let parent = dir.path().join("runtime");
        fs::create_dir_all(outside.path().join("session"))?;
        symlink(outside.path(), &parent)?;
        let path = parent.join("session").join("session.log");

        let Err(error) = open_log(&path) else {
            return Err("symlink intermediate parent must fail".into());
        };

        assert_eq!(error.code, 69);
        assert!(!outside.path().join("session").join("session.log").exists());
        Ok(())
    }

    #[test]
    fn start_listener_refuses_symlink_listen_path() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("target.txt");
        let link = dir.path().join("session.sock");
        fs::write(&target, "keep me")?;
        symlink(&target, &link)?;
        let writer: PtyWriter = Arc::new(Mutex::new(Box::new(Vec::<u8>::new())));
        let clients: Clients = Arc::new(Mutex::new(Vec::new()));

        let Err(error) = start_listener(&link, writer, clients) else {
            return Err("symlinks are refused".into());
        };

        assert_eq!(error.code, 69);
        assert!(
            error
                .message
                .contains("refusing to replace non-socket path")
        );
        assert_eq!(fs::read_to_string(&target)?, "keep me");
        assert!(link.is_symlink());
        Ok(())
    }

    #[test]
    fn start_listener_rejects_symlink_parent_directory() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        let parent = dir.path().join("sockets");
        symlink(outside.path(), &parent)?;
        let socket = parent.join("session.sock");
        let writer: PtyWriter = Arc::new(Mutex::new(Box::new(Vec::<u8>::new())));
        let clients: Clients = Arc::new(Mutex::new(Vec::new()));

        let Err(error) = start_listener(&socket, writer, clients) else {
            return Err("symlink parent must fail".into());
        };

        assert_eq!(error.code, 69);
        assert!(!outside.path().join("session.sock").exists());
        assert!(parent.is_symlink());
        Ok(())
    }
}
