use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::object::executor::{
    MAX_RUNNER_CONTROL_BYTES, model_candidates, model_default_base_url, read_small_plain_text_file,
};
use crate::support::receipt::{EmptyDirReceipt, SocketReceipt};
use crate::{is_object_name, peer_credentials};

const ACCEPT_PAUSE: Duration = Duration::from_millis(10);
const CONNECT_BUDGET: Duration = Duration::from_millis(250);
const RELAY_TIMEOUT: Duration = Duration::from_millis(100);
const RELAY_BUFFER_BYTES: usize = 16 * 1024;

/// Stable failure while planning or creating a provider egress boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderEgressError {
    InvalidRun,
    InvalidModel,
    MissingControl,
    InvalidBaseUrl,
    CannotResolve,
    AuthorityConflict,
    CannotCreate,
}

impl fmt::Display for ProviderEgressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match *self {
            Self::InvalidRun => "invalid provider egress run",
            Self::InvalidModel => "invalid provider egress model",
            Self::MissingControl => "missing provider egress control",
            Self::InvalidBaseUrl => "invalid provider egress base URL",
            Self::CannotResolve => "cannot resolve provider egress authority",
            Self::AuthorityConflict => "conflicting provider egress authority",
            Self::CannotCreate => "cannot create provider egress boundary",
        })
    }
}

impl std::error::Error for ProviderEgressError {}

#[derive(Debug, Eq, PartialEq)]
struct ProviderTarget {
    provider: String,
    addresses: Vec<SocketAddr>,
}

/// Run-scoped Unix sockets that relay only to pre-resolved provider authorities.
pub struct ProviderEgress {
    directory: EmptyDirReceipt,
    sockets: BTreeMap<String, SocketReceipt>,
    shutdown: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl fmt::Debug for ProviderEgress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderEgress")
            .field("host_dir", &self.host_dir())
            .field("providers", &self.sockets.keys())
            .finish_non_exhaustive()
    }
}

impl ProviderEgress {
    #[expect(
        clippy::too_many_arguments,
        reason = "egress creation keeps trusted identity and run inputs explicit"
    )]
    pub fn create(
        control_dir: &Path,
        ctx_root: &Path,
        model: &str,
        uid: u32,
        gid: u32,
        run: &str,
    ) -> Result<Self, ProviderEgressError> {
        if !is_object_name(run) {
            return Err(ProviderEgressError::InvalidRun);
        }
        let targets = plan_targets(ctx_root, model)?;
        let runtime_owner = (
            nix::unistd::geteuid().as_raw(),
            nix::unistd::getegid().as_raw(),
        );
        let directory = EmptyDirReceipt::create(
            control_dir,
            &format!("egress-{run}"),
            runtime_owner.0,
            runtime_owner.1,
            0o711,
        )
        .map_err(|_error| ProviderEgressError::CannotCreate)?;
        let mut sockets = BTreeMap::new();
        let mut listeners = Vec::new();
        for target in targets {
            let name = format!("{}.sock", target.provider);
            let bound = SocketReceipt::bind(directory.path(), &name, (uid, gid));
            let (receipt, listener) = match bound {
                Ok(bound) => bound,
                Err(_error) => {
                    cleanup_receipts(&sockets, &directory);
                    return Err(ProviderEgressError::CannotCreate);
                }
            };
            listeners.push((target, listener));
            sockets.insert(name.trim_end_matches(".sock").to_owned(), receipt);
        }
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();
        for (target, listener) in listeners {
            let stop = Arc::clone(&shutdown);
            let provider = target.provider.clone();
            match thread::Builder::new()
                .name(format!("egress-{provider}"))
                .spawn(move || serve(listener, target.addresses, uid, stop))
            {
                Ok(handle) => threads.push(handle),
                Err(_error) => {
                    shutdown.store(true, Ordering::Release);
                    join_threads(&mut threads);
                    cleanup_receipts(&sockets, &directory);
                    return Err(ProviderEgressError::CannotCreate);
                }
            }
        }
        Ok(Self {
            directory,
            sockets,
            shutdown,
            threads,
        })
    }

    #[must_use]
    pub fn host_dir(&self) -> &Path {
        self.directory.path()
    }

    #[must_use]
    pub fn socket(&self, provider: &str) -> Option<&Path> {
        self.sockets.get(provider).map(SocketReceipt::path)
    }
}

impl Drop for ProviderEgress {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        join_threads(&mut self.threads);
        cleanup_receipts(&self.sockets, &self.directory);
    }
}

fn plan_targets(ctx_root: &Path, model: &str) -> Result<Vec<ProviderTarget>, ProviderEgressError> {
    let candidates =
        model_candidates(ctx_root, model).map_err(|_error| ProviderEgressError::InvalidModel)?;
    let mut targets: BTreeMap<String, (String, u16, BTreeSet<SocketAddr>)> = BTreeMap::new();
    for (index, candidate) in candidates.into_iter().enumerate() {
        let (provider, name) = candidate
            .name
            .split_once('/')
            .ok_or(ProviderEgressError::InvalidModel)?;
        let default = candidate
            .path
            .parent()
            .map(|parent| parent.join(format!("{name}.d/default")))
            .ok_or(ProviderEgressError::MissingControl)?;
        let content = match read_small_plain_text_file(
            &default,
            MAX_RUNNER_CONTROL_BYTES,
            "provider egress control",
        ) {
            Ok(content) => content,
            Err(error) if index > 0 && error.kind() == io::ErrorKind::NotFound => continue,
            Err(_error) => return Err(ProviderEgressError::MissingControl),
        };
        let base_url =
            model_default_base_url(&content).ok_or(ProviderEgressError::InvalidBaseUrl)?;
        let url =
            reqwest::Url::parse(&base_url).map_err(|_error| ProviderEgressError::InvalidBaseUrl)?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(ProviderEgressError::InvalidBaseUrl);
        }
        let host = url
            .host_str()
            .ok_or(ProviderEgressError::InvalidBaseUrl)?
            .trim_start_matches('[')
            .trim_end_matches(']');
        let port = url
            .port_or_known_default()
            .ok_or(ProviderEgressError::InvalidBaseUrl)?;
        let addresses = (host, port)
            .to_socket_addrs()
            .map_err(|_error| ProviderEgressError::CannotResolve)?
            .collect::<BTreeSet<_>>();
        if addresses.is_empty() {
            return Err(ProviderEgressError::CannotResolve);
        }
        match targets.get_mut(provider) {
            Some(known) => {
                if known.0 != host || known.1 != port {
                    return Err(ProviderEgressError::AuthorityConflict);
                }
                known.2.extend(addresses);
            }
            None => {
                targets.insert(provider.to_owned(), (host.to_owned(), port, addresses));
            }
        }
    }
    Ok(targets
        .into_iter()
        .map(|(provider, (_host, _port, addresses))| ProviderTarget {
            provider,
            addresses: addresses.into_iter().collect(),
        })
        .collect())
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "the host thread exclusively owns its listener, target set, and stop handle"
)]
fn serve(listener: UnixListener, addresses: Vec<SocketAddr>, uid: u32, shutdown: Arc<AtomicBool>) {
    if listener.set_nonblocking(true).is_err() {
        return;
    }
    while !shutdown.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _address)) => {
                if peer_credentials(&stream).is_ok_and(|peer| peer.uid() == uid) {
                    let _ignored = relay(stream, &addresses, &shutdown);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_PAUSE);
            }
            Err(_error) => return,
        }
    }
}

fn relay(
    local: UnixStream,
    addresses: &[SocketAddr],
    shutdown: &Arc<AtomicBool>,
) -> io::Result<()> {
    let remote = connect_target(addresses, shutdown)?;
    local.set_read_timeout(Some(RELAY_TIMEOUT))?;
    local.set_write_timeout(Some(RELAY_TIMEOUT))?;
    remote.set_read_timeout(Some(RELAY_TIMEOUT))?;
    remote.set_write_timeout(Some(RELAY_TIMEOUT))?;
    let local_read = local.try_clone()?;
    let remote_write = remote.try_clone()?;
    let outbound_stop = Arc::clone(shutdown);
    let outbound = thread::spawn(move || {
        copy_bounded(local_read, remote_write, &outbound_stop, |stream| {
            stream.shutdown(Shutdown::Write)
        })
    });
    let inbound = copy_bounded(remote, local, shutdown, |stream| {
        stream.shutdown(Shutdown::Write)
    });
    let _outbound = outbound.join();
    inbound
}

fn connect_target(addresses: &[SocketAddr], shutdown: &AtomicBool) -> io::Result<TcpStream> {
    let deadline = Instant::now() + CONNECT_BUDGET;
    for address in addresses {
        if shutdown.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "provider egress connect cancelled",
            ));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        if let Ok(stream) = TcpStream::connect_timeout(address, remaining) {
            return Ok(stream);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "provider egress connect budget exhausted",
    ))
}

fn copy_bounded<R, W>(
    mut source: R,
    mut destination: W,
    shutdown: &AtomicBool,
    finish: impl FnOnce(&W) -> io::Result<()>,
) -> io::Result<()>
where
    R: Read,
    W: Write,
{
    let mut buffer = [0_u8; RELAY_BUFFER_BYTES];
    loop {
        if shutdown.load(Ordering::Acquire) {
            return Ok(());
        }
        let count = match source.read(&mut buffer) {
            Ok(0) => return finish(&destination),
            Ok(count) => count,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error),
        };
        if shutdown.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut written = 0;
        while written < count {
            if shutdown.load(Ordering::Acquire) {
                return Ok(());
            }
            let pending = buffer
                .get(written..count)
                .ok_or_else(|| io::Error::other("provider egress relay range is invalid"))?;
            match destination.write(pending) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(bytes) => written += bytes,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(error),
            }
        }
    }
}

fn join_threads(threads: &mut Vec<JoinHandle<()>>) {
    for thread in threads.drain(..) {
        let _joined = thread.join();
    }
}

fn cleanup_receipts(sockets: &BTreeMap<String, SocketReceipt>, directory: &EmptyDirReceipt) {
    for receipt in sockets.values().rev() {
        let _cleanup = receipt.cleanup();
    }
    let _cleanup = directory.cleanup();
}

#[cfg(test)]
mod tests;
