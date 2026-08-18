use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::peer_credentials;
use crate::support::receipt::{EmptyDirReceipt, SocketReceipt};

#[cfg(test)]
use plan::plan_targets;
pub(crate) use plan::{ProviderEgressPlan, is_provider_model};
#[cfg(test)]
use secret::ProviderEgressCredential;
use target::ProviderTarget;

const ACCEPT_PAUSE: Duration = Duration::from_millis(10);

/// Fixed path where a sandboxed provider runner sees its run-scoped relay sockets.
pub use cortexfs_paths::PROVIDER_EGRESS_SANDBOX_PATH;
/// Environment variable advertising the fixed provider relay directory.
pub const PROVIDER_EGRESS_DIR_ENV: &str = "CTX_PROVIDER_EGRESS_DIR";

/// Stable failure while planning or creating a provider egress boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ProviderEgressError {
    #[error("invalid provider egress run")]
    InvalidRun,
    #[error("invalid provider egress model")]
    InvalidModel,
    #[error("missing provider egress control")]
    MissingControl,
    #[error("invalid provider egress base URL")]
    InvalidBaseUrl,
    #[error("conflicting provider egress authority")]
    AuthorityConflict,
    #[error("cannot create provider egress boundary")]
    CannotCreate,
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
    pub(crate) fn create(
        control_dir: &Path,
        plan: ProviderEgressPlan,
        uid: u32,
        gid: u32,
    ) -> Result<Self, ProviderEgressError> {
        let runtime_owner = (
            nix::unistd::geteuid().as_raw(),
            nix::unistd::getegid().as_raw(),
        );
        let directory = EmptyDirReceipt::create(
            control_dir,
            &format!("egress-{}", plan.run),
            runtime_owner.0,
            runtime_owner.1,
            0o711,
        )
        .map_err(|_error| ProviderEgressError::CannotCreate)?;
        let mut sockets = BTreeMap::new();
        let mut listeners = Vec::new();
        for target in plan.targets {
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
                .spawn(move || serve(listener, target, uid, stop))
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

#[expect(
    clippy::needless_pass_by_value,
    reason = "the host thread exclusively owns its listener, target set, and stop handle"
)]
fn serve(listener: UnixListener, target: ProviderTarget, uid: u32, shutdown: Arc<AtomicBool>) {
    if listener.set_nonblocking(true).is_err() {
        return;
    }
    while !shutdown.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _address)) => {
                if peer_credentials(&stream).is_ok_and(|peer| peer.uid() == uid) {
                    let _ignored = http::relay(stream, &target, &shutdown);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_PAUSE);
            }
            Err(_error) => return,
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

mod http;
mod plan;
mod secret;
mod target;
#[cfg(test)]
mod tests;
