use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::object::executor::{
    MAX_RUNNER_CONTROL_BYTES, model_candidates, model_default_base_url, read_small_plain_text_file,
};
use crate::support::receipt::{EmptyDirReceipt, SocketReceipt};
use crate::{is_object_name, peer_credentials};

const ACCEPT_PAUSE: Duration = Duration::from_millis(10);

/// Fixed path where a sandboxed provider runner sees its run-scoped relay sockets.
pub const PROVIDER_EGRESS_SANDBOX_PATH: &str = "/run/cortexfs/provider-egress";
/// Environment variable advertising the fixed provider relay directory.
pub const PROVIDER_EGRESS_DIR_ENV: &str = "CTX_PROVIDER_EGRESS_DIR";

pub(crate) fn is_provider_model(ctx_root: &Path, model: &str) -> Result<bool, ProviderEgressError> {
    let candidates =
        model_candidates(ctx_root, model).map_err(|_error| ProviderEgressError::InvalidModel)?;
    Ok(candidates
        .first()
        .is_some_and(|candidate| candidate.name != "debug/echo"))
}

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

#[derive(Debug, Eq, PartialEq)]
struct ProviderTarget {
    provider: String,
    base_url: String,
    authority: String,
    base_path: String,
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

fn plan_targets(ctx_root: &Path, model: &str) -> Result<Vec<ProviderTarget>, ProviderEgressError> {
    let candidates =
        model_candidates(ctx_root, model).map_err(|_error| ProviderEgressError::InvalidModel)?;
    let mut targets: BTreeMap<String, ProviderTarget> = BTreeMap::new();
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
        if url.cannot_be_a_base()
            || url.username() != ""
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(ProviderEgressError::InvalidBaseUrl);
        }
        let authority = url.origin().ascii_serialization();
        let source_path = url.path().trim_end_matches('/');
        let base_path = crate::provider::effective_base_url(source_path);
        if base_path.contains(['%', '\\']) {
            return Err(ProviderEgressError::InvalidBaseUrl);
        }
        let mut canonical = url;
        canonical.set_path(&base_path);
        match targets.get_mut(provider) {
            Some(known) => {
                if known.authority != authority || known.base_path != base_path {
                    return Err(ProviderEgressError::AuthorityConflict);
                }
            }
            None => {
                targets.insert(
                    provider.to_owned(),
                    ProviderTarget {
                        provider: provider.to_owned(),
                        base_url: canonical.to_string().trim_end_matches('/').to_owned(),
                        authority,
                        base_path,
                    },
                );
            }
        }
    }
    Ok(targets.into_values().collect())
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
#[cfg(test)]
mod tests;
