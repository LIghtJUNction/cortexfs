use std::io;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::thread::JoinHandle;

use crate::support::process::terminate_process_group;

pub(super) struct CurlProcess {
    child: Option<Child>,
    pub(super) output: Option<JoinHandle<io::Result<u64>>>,
    pub(super) errors: Option<JoinHandle<Vec<u8>>>,
}

impl CurlProcess {
    pub(super) fn spawn() -> io::Result<Self> {
        use std::os::unix::process::CommandExt;

        let mut command = Command::new(crate::support::command::CURL);
        command
            .args(["-q", "--config", "-"])
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        Ok(Self {
            child: Some(command.spawn()?),
            output: None,
            errors: None,
        })
    }

    pub(super) fn child_mut(&mut self) -> io::Result<&mut Child> {
        require(self.child.as_mut(), "curl child is unavailable")
    }

    pub(super) fn take_stdio(&mut self) -> io::Result<(ChildStdin, ChildStdout, ChildStderr)> {
        let child = self.child_mut()?;
        let stdin = require(child.stdin.take(), "missing curl stdin")?;
        let stdout = require(child.stdout.take(), "missing curl stdout")?;
        let stderr = require(child.stderr.take(), "missing curl stderr")?;
        Ok((stdin, stdout, stderr))
    }

    pub(super) fn finish(&mut self, terminate: bool) -> io::Result<(ExitStatus, io::Result<u64>)> {
        let mut child = require(self.child.take(), "curl child is unavailable")?;
        if terminate {
            terminate_process_group(&mut child);
        }
        let status = child.wait();
        let output = self.output.take();
        let copied = join(output, Ok(0), "curl output thread panicked").and_then(|result| result);
        let errors = self.errors.take();
        let errors = join(errors, Vec::new(), "curl stderr thread panicked");
        drop(errors?);
        Ok((status?, copied))
    }
}

fn require<T>(value: Option<T>, message: &'static str) -> io::Result<T> {
    value.ok_or_else(|| io::Error::other(message))
}

fn join<T>(handle: Option<JoinHandle<T>>, default: T, message: &'static str) -> io::Result<T> {
    handle.map_or_else(
        || Ok(default),
        |handle| handle.join().map_err(|_panic| io::Error::other(message)),
    )
}

impl Drop for CurlProcess {
    fn drop(&mut self) {
        if self.child.is_some() {
            let _finished = self.finish(true);
        }
    }
}
