use std::collections::{HashMap, VecDeque};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const MAX_STATE_ENTRIES: usize = 1024;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct TerminalKey {
    pub(super) uid: u32,
    pub(super) agent: String,
    pub(super) session: String,
}

pub(super) struct Supervisor {
    pub(super) unit: String,
    pub(super) generation: String,
    pub(super) control: Mutex<UnixStream>,
}

pub(super) struct BrokerState {
    supervisors: Mutex<HashMap<TerminalKey, Arc<Supervisor>>>,
    changed: Condvar,
    nonces: Mutex<VecDeque<(u32, String)>>,
}

impl BrokerState {
    pub(super) fn new() -> Self {
        Self {
            supervisors: Mutex::new(HashMap::new()),
            changed: Condvar::new(),
            nonces: Mutex::new(VecDeque::new()),
        }
    }

    pub(super) fn register(&self, key: TerminalKey, supervisor: Supervisor) -> Result<(), ()> {
        let mut entries = self.supervisors.lock().map_err(|_error| ())?;
        let same = entries.get(&key);
        if same.is_some_and(|entry| control_is_live(entry))
            || (same.is_none() && entries.len() >= MAX_STATE_ENTRIES)
        {
            return Err(());
        }
        entries.insert(key, Arc::new(supervisor));
        drop(entries);
        self.changed.notify_all();
        Ok(())
    }

    pub(super) fn get(&self, key: &TerminalKey) -> Option<Arc<Supervisor>> {
        self.supervisors.lock().ok()?.get(key).cloned()
    }

    pub(super) fn wait(
        &self,
        key: &TerminalKey,
        unit: &str,
        timeout: Duration,
    ) -> Option<Arc<Supervisor>> {
        let deadline = Instant::now() + timeout;
        let mut entries = self.supervisors.lock().ok()?;
        loop {
            if let Some(entry) = entries.get(key).filter(|entry| entry.unit == unit).cloned() {
                drop(entries);
                return Some(entry);
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                drop(entries);
                return None;
            };
            let (next, result) = self.changed.wait_timeout(entries, remaining).ok()?;
            entries = next;
            if result.timed_out() {
                drop(entries);
                return None;
            }
        }
    }

    pub(super) fn remove(&self, key: &TerminalKey, generation: &str) {
        if let Ok(mut entries) = self.supervisors.lock()
            && entries
                .get(key)
                .is_some_and(|entry| entry.generation == generation)
        {
            entries.remove(key);
        }
    }

    pub(super) fn consume_nonce(&self, uid: u32, nonce: String) -> bool {
        let Ok(mut nonces) = self.nonces.lock() else {
            return false;
        };
        let key = (uid, nonce);
        if nonces.contains(&key) {
            return false;
        }
        nonces.push_back(key);
        if nonces.len() > MAX_STATE_ENTRIES {
            nonces.pop_front();
        }
        true
    }
}

fn control_is_live(supervisor: &Supervisor) -> bool {
    let Ok(control) = supervisor.control.lock() else {
        return false;
    };
    let mut byte = [0_u8; 1];
    rustix::net::recv(
        &*control,
        &mut byte,
        rustix::net::RecvFlags::DONTWAIT | rustix::net::RecvFlags::PEEK,
    )
    .is_err_and(|error| error == rustix::io::Errno::AGAIN)
}
