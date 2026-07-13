//! Bounded durable backing for session JSONL streams.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Error, ErrorKind, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{FileExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{cell::Cell, thread_local};

use arrow_array::{RecordBatch, StringArray, UInt64Array};
use arrow_schema::{DataType, Field, Schema};
use parquet::arrow::ArrowWriter;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::file::properties::WriterProperties;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use nix::fcntl::{Flock, FlockArg};
use nix::libc;

#[cfg(test)]
use std::sync::{Barrier, Mutex, OnceLock};

use crate::abi::path::{AbiPathKind, parse_abi_path};
use crate::authority::helpers::generated_sibling_name;
use crate::support::jsonl::read_jsonl_line;
use crate::support::plain::{
    CreatePlainDirMessages, create_plain_dir_exclusive, create_plain_dir_with,
    open_plain_directory, open_plain_file, plain_file_name, read_small_text_file, sync_plain_dir,
};
use crate::{atomic_create_text_with_mode, atomic_replace_text_with_mode};

const STORE_DIR: &str = ".store";
const LOCK_FILE: &str = "lock";
const WAL_FILE: &str = "wal.jsonl";
const MANIFEST_FILE: &str = "manifest.json";
const DATA_DIR: &str = "data";
const INDEX_DIR: &str = "index";
const CLAIM_DIR: &str = "claim";
const CLAIM_CURSOR_FILE: &str = ".cursor.json";
const CLAIM_VERSION: u32 = 1;
const MAX_SHARD_ROWS: usize = 128;
const MAX_SHARD_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_PAYLOAD_BYTES: usize = 256 * 1024;
const MAX_WAL_OVERHEAD_BYTES: usize = 1024;
const MAX_WAL_FRAME_BYTES: usize = MAX_PAYLOAD_BYTES * 2 + MAX_WAL_OVERHEAD_BYTES;
const INDEX_RECORD_BYTES: usize = 48;
const MAX_CLAIM_BYTES: u64 = 16 * 1024 * 1024;
const MAX_CLAIM_CURSOR_BYTES: u64 = 16 * 1024;

#[cfg(test)]
type TestBarrierPair = (Arc<Barrier>, Arc<Barrier>);

#[cfg(test)]
static PRUNE_BARRIERS: OnceLock<Mutex<Option<TestBarrierPair>>> = OnceLock::new();

#[cfg(test)]
type LegacySnapshotBarrier = (PathBuf, Arc<Barrier>, Arc<Barrier>);

#[cfg(test)]
static LEGACY_SNAPSHOT_BARRIER: OnceLock<Mutex<Option<LegacySnapshotBarrier>>> = OnceLock::new();

thread_local! {
    static EXPORT_COPY_FAILURE: Cell<bool> = const { Cell::new(false) };
    static SHARD_WRITE_FAILURE: Cell<bool> = const { Cell::new(false) };
    static SHARD_RENAME_FAILURE: Cell<bool> = const { Cell::new(false) };
    static PRUNE_RENAME_FAILURE: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
thread_local! {
    static INDEX_RECORD_READS: Cell<usize> = const { Cell::new(0) };
    static SHARD_OPENS: Cell<usize> = const { Cell::new(0) };
    static CLAIM_SCAN_BYTES: Cell<usize> = const { Cell::new(0) };
    static CLAIM_CURSOR_FAILURE: Cell<bool> = const { Cell::new(false) };
}

/// One projected session JSONL stream.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stream {
    /// Conversation messages.
    Messages,
    /// Runtime events.
    Events,
}

enum HistoryView {
    Legacy(LegacySnapshot),
    Columnar,
}

struct LegacySnapshot {
    messages: MarkerSnapshot,
    events: MarkerSnapshot,
}

impl LegacySnapshot {
    fn open(session: &Path) -> std::io::Result<Self> {
        let messages = MarkerSnapshot::open(&session.join(Stream::Messages.marker()))?;
        wait_during_legacy_snapshot(session);
        Ok(Self {
            messages,
            events: MarkerSnapshot::open(&session.join(Stream::Events.marker()))?,
        })
    }

    fn marker(&self, stream: Stream) -> &MarkerSnapshot {
        match stream {
            Stream::Messages => &self.messages,
            Stream::Events => &self.events,
        }
    }
}

struct MarkerSnapshot {
    file: File,
    length: Cell<u64>,
    receipt: FileReceipt,
}

impl MarkerSnapshot {
    fn open(path: &Path) -> std::io::Result<Self> {
        let file = open_plain_file(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "session marker is not a plain file",
            ));
        }
        Ok(Self {
            file,
            length: Cell::new(metadata.len()),
            receipt: FileReceipt {
                dev: metadata.dev(),
                ino: metadata.ino(),
            },
        })
    }

    fn read_at(&self, offset: u64, size: usize) -> std::io::Result<Vec<u8>> {
        let remaining = self.length.get().saturating_sub(offset);
        let available = usize::try_from(remaining).unwrap_or(usize::MAX).min(size);
        let mut bytes = vec![0; available];
        self.file.read_exact_at(&mut bytes, offset)?;
        Ok(bytes)
    }

    fn tail(&self, max_bytes: usize) -> std::io::Result<Vec<u8>> {
        let limit =
            u64::try_from(max_bytes).map_err(|_error| Error::other("tail limit too large"))?;
        let offset = self.length.get().saturating_sub(limit);
        let mut bytes = self.read_at(offset, max_bytes)?;
        trim_partial_tail(offset, &mut bytes);
        Ok(bytes)
    }

    fn metadata_matches(&self, metadata: &fs::Metadata, length: u64) -> bool {
        metadata.is_file()
            && (metadata.dev(), metadata.ino()) == (self.receipt.dev, self.receipt.ino)
            && metadata.len() == length
    }

    fn verify_current(&self, path: &Path) -> std::io::Result<()> {
        let length = self.length.get();
        if !self.metadata_matches(&self.file.metadata()?, length)
            || !self.metadata_matches(&fs::symlink_metadata(path)?, length)
        {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "session marker changed since history snapshot",
            ));
        }
        Ok(())
    }
}

/// Stable lock guard for one session's authoritative history representation.
pub(crate) struct HistoryGuard<'a> {
    session: &'a Path,
    view: HistoryView,
    _lock: Option<Flock<File>>,
}

impl<'a> HistoryGuard<'a> {
    /// Acquires the session history lock for mutation.
    pub(crate) fn exclusive(session: &'a Path) -> std::io::Result<Self> {
        Self::lock(session, FlockArg::LockExclusive)
    }

    /// Acquires the session history lock for projected reads.
    pub(crate) fn shared(session: &'a Path) -> std::io::Result<Self> {
        let store = session.join(STORE_DIR);
        loop {
            if let Some(file) = open_existing_shared_lock(&store)? {
                let lock = Flock::lock(file, FlockArg::LockShared)
                    .map_err(|(_file, error)| Error::from(error))?;
                return Ok(Self {
                    session,
                    view: history_view(session, &store)?,
                    _lock: Some(lock),
                });
            }

            let snapshot = LegacySnapshot::open(session)?;
            if open_existing_shared_lock(&store)?.is_some() {
                continue;
            }
            if manifest_committed(&store)? {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    "committed session store has no lock",
                ));
            }
            return Ok(Self {
                session,
                view: HistoryView::Legacy(snapshot),
                _lock: None,
            });
        }
    }

    fn lock(session: &'a Path, mode: FlockArg) -> std::io::Result<Self> {
        let store = session.join(STORE_DIR);
        create_store_dir(&store)?;
        let file = open_lock(&store.join(LOCK_FILE))?;
        let lock = Flock::lock(file, mode).map_err(|(_file, error)| Error::from(error))?;
        Ok(Self {
            session,
            view: history_view(session, &store)?,
            _lock: Some(lock),
        })
    }

    /// Returns whether a committed manifest has activated columnar projection.
    pub(crate) fn is_columnar(&self) -> bool {
        matches!(self.view, HistoryView::Columnar)
    }

    /// Appends complete lines to the currently authoritative representation.
    pub(crate) fn append(&self, stream: Stream, lines: &[&str]) -> std::io::Result<()> {
        match self.view {
            HistoryView::Legacy(ref snapshot) => {
                let marker = snapshot.marker(stream);
                append_marker_locked(&self.session.join(stream.marker()), marker, lines)
            }
            HistoryView::Columnar => append_locked(self.session, stream, lines),
        }
    }

    /// Reads projected bytes while retaining the stable history lock.
    pub(crate) fn read_at(
        &self,
        stream: Stream,
        offset: u64,
        size: usize,
    ) -> std::io::Result<Vec<u8>> {
        match self.view {
            HistoryView::Legacy(ref snapshot) => snapshot.marker(stream).read_at(offset, size),
            HistoryView::Columnar => read_at_locked(self.session, stream, offset, size),
        }
    }

    /// Returns projected stream length while retaining the stable history lock.
    pub(crate) fn len(&self, stream: Stream) -> std::io::Result<u64> {
        match self.view {
            HistoryView::Legacy(ref snapshot) => Ok(snapshot.marker(stream).length.get()),
            HistoryView::Columnar => len_locked(self.session, stream),
        }
    }

    /// Reads one bounded projected stream as UTF-8.
    pub(crate) fn read_text(&self, stream: Stream, max_bytes: u64) -> std::io::Result<String> {
        let length = self.len(stream)?;
        if length > max_bytes {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "session history exceeds read limit",
            ));
        }
        let size = usize::try_from(length)
            .map_err(|_error| Error::other("session history is too large"))?;
        let bytes = self.read_at(stream, 0, size)?;
        if bytes.len() != size {
            return Err(Error::new(
                ErrorKind::UnexpectedEof,
                "session history changed during locked read",
            ));
        }
        String::from_utf8(bytes)
            .map_err(|_error| Error::new(ErrorKind::InvalidData, "session history is not UTF-8"))
    }

    /// Reads a bounded recent projected tail without a partial leading line.
    pub(crate) fn tail(&self, stream: Stream, max_bytes: usize) -> std::io::Result<Vec<u8>> {
        match self.view {
            HistoryView::Legacy(ref snapshot) => snapshot.marker(stream).tail(max_bytes),
            HistoryView::Columnar => tail_locked(self.session, stream, max_bytes),
        }
    }

    /// Advances the durable derived send-claim index to the projected EOF.
    pub(crate) fn refresh_claims(&self) -> std::io::Result<()> {
        if let HistoryView::Legacy(ref snapshot) = self.view {
            for stream in [Stream::Messages, Stream::Events] {
                snapshot
                    .marker(stream)
                    .verify_current(&self.session.join(stream.marker()))?;
            }
        }
        let claim_dir = self.session.join(STORE_DIR).join(CLAIM_DIR);
        create_store_dir(&claim_dir)?;
        let mut cursor = read_claim_cursor(&claim_dir)?;
        for stream in [Stream::Messages, Stream::Events] {
            let advanced = self.scan_claim_stream(&claim_dir, stream, cursor.stream(stream))?;
            cursor.set_stream(stream, advanced);
            #[cfg(test)]
            fail_claim_cursor_write()?;
            write_claim_cursor(&claim_dir, &cursor)?;
        }
        Ok(())
    }

    /// Resolves one indexed send claim after advancing the bounded cursor.
    pub(crate) fn lookup_send(
        &self,
        id: &str,
        input: &str,
        scope: &str,
        cwd: Option<&str>,
    ) -> std::io::Result<SendClaim> {
        self.refresh_claims()?;
        let claim_dir = self.session.join(STORE_DIR).join(CLAIM_DIR);
        let Some(claim) = read_claim(&claim_dir, id)? else {
            return Ok(SendClaim::Vacant);
        };
        let (Some(user), Some(start)) = (claim.user.as_ref(), claim.start.as_ref()) else {
            return Ok(SendClaim::Corrupt);
        };
        if claim.corrupt || !claim.seen || !start.canonical {
            return Ok(SendClaim::Corrupt);
        }
        if user.content.as_deref() != Some(input)
            || start.scope.as_deref() != Some(scope)
            || !start.cwd.matches(cwd)
        {
            return Ok(SendClaim::Conflict);
        }
        Ok(SendClaim::Replay(claim.done.as_ref().map_or_else(
            || start.source.line.clone(),
            |done| done.source.line.clone(),
        )))
    }

    fn scan_claim_stream(
        &self,
        claim_dir: &Path,
        stream: Stream,
        mut cursor: ClaimStreamCursor,
    ) -> std::io::Result<ClaimStreamCursor> {
        let length = self.len(stream)?;
        if cursor.offset > length {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "session claim cursor is past projected history",
            ));
        }
        if self.is_columnar() {
            cursor.raw = None;
            let reader = ProjectedReader {
                guard: self,
                stream,
                offset: cursor.offset,
                end: length,
            };
            cursor.offset =
                scan_claim_reader(BufReader::new(reader), cursor.offset, stream, claim_dir)?;
        } else {
            cursor = self.scan_raw_claim_stream(claim_dir, stream, cursor, length)?;
        }
        if cursor.offset != length {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "session claim cursor did not reach projected EOF",
            ));
        }
        Ok(cursor)
    }

    fn scan_raw_claim_stream(
        &self,
        claim_dir: &Path,
        stream: Stream,
        mut cursor: ClaimStreamCursor,
        length: u64,
    ) -> std::io::Result<ClaimStreamCursor> {
        let snapshot = match self.view {
            HistoryView::Legacy(ref snapshot) => snapshot,
            HistoryView::Columnar => {
                return Err(Error::other("columnar history has no raw snapshot"));
            }
        };
        let marker = snapshot.marker(stream);
        if marker.length.get() < cursor.offset {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "session marker shrank before claim scan",
            ));
        }
        let receipt = marker.receipt;
        if cursor.raw.is_some_and(|expected| expected != receipt) {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "session marker changed before claim scan",
            ));
        }
        let mut file = marker.file.try_clone()?;
        file.seek(SeekFrom::Start(cursor.offset))?;
        let remaining = length
            .checked_sub(cursor.offset)
            .ok_or_else(|| Error::other("session claim cursor underflow"))?;
        cursor.offset = scan_claim_reader(
            BufReader::new(file.take(remaining)),
            cursor.offset,
            stream,
            claim_dir,
        )?;
        cursor.raw = Some(receipt);
        Ok(cursor)
    }
}

fn history_view(session: &Path, store: &Path) -> std::io::Result<HistoryView> {
    if manifest_committed(store)? {
        Ok(HistoryView::Columnar)
    } else {
        LegacySnapshot::open(session).map(HistoryView::Legacy)
    }
}

fn manifest_committed(store: &Path) -> std::io::Result<bool> {
    match fs::symlink_metadata(store.join(MANIFEST_FILE)) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => Ok(true),
        Ok(_) => Err(Error::new(
            ErrorKind::InvalidData,
            "session manifest is not a plain file",
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn read_committed_manifest(store: &Path) -> std::io::Result<Manifest> {
    if !manifest_committed(store)? {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "committed session manifest is missing",
        ));
    }
    read_manifest(store)
}

/// Result of resolving one durable indexed send claim.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SendClaim {
    /// No fact exists for the requested id after both cursors reached EOF.
    Vacant,
    /// The canonical request already exists; return its terminal or start frame.
    Replay(String),
    /// The id exists with different request fields.
    Conflict,
    /// The indexed history for the id is incomplete or contradictory.
    Corrupt,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct ClaimStreamCursor {
    offset: u64,
    raw: Option<FileReceipt>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct FileReceipt {
    dev: u64,
    ino: u64,
}

#[derive(Debug, Deserialize, Serialize)]
struct ClaimCursor {
    version: u32,
    messages: ClaimStreamCursor,
    events: ClaimStreamCursor,
}

impl Default for ClaimCursor {
    fn default() -> Self {
        Self {
            version: CLAIM_VERSION,
            messages: ClaimStreamCursor::default(),
            events: ClaimStreamCursor::default(),
        }
    }
}

impl ClaimCursor {
    fn stream(&self, stream: Stream) -> ClaimStreamCursor {
        match stream {
            Stream::Messages => self.messages,
            Stream::Events => self.events,
        }
    }

    fn set_stream(&mut self, stream: Stream, cursor: ClaimStreamCursor) {
        match stream {
            Stream::Messages => self.messages = cursor,
            Stream::Events => self.events = cursor,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct ClaimSource {
    offset: u64,
    end: u64,
    line: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct UserClaim {
    source: ClaimSource,
    content: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct StartClaim {
    source: ClaimSource,
    canonical: bool,
    scope: Option<String>,
    cwd: ClaimCwd,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct DoneClaim {
    source: ClaimSource,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum ClaimCwd {
    Missing,
    Null,
    Text(String),
    Invalid,
}

impl ClaimCwd {
    fn from_value(value: Option<&Value>) -> Self {
        match value {
            None => Self::Missing,
            Some(value) if value.is_null() => Self::Null,
            Some(value) => value
                .as_str()
                .map_or(Self::Invalid, |value| Self::Text(value.to_owned())),
        }
    }

    fn is_canonical(&self) -> bool {
        matches!(self, Self::Null | Self::Text(_))
    }

    fn matches(&self, expected: Option<&str>) -> bool {
        expected.map_or(
            matches!(self, Self::Null),
            |expected| matches!(self, Self::Text(recorded) if recorded == expected),
        )
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct ClaimFile {
    version: u32,
    id: String,
    seen: bool,
    corrupt: bool,
    user: Option<UserClaim>,
    start: Option<StartClaim>,
    done: Option<DoneClaim>,
}

impl ClaimFile {
    fn new(id: &str) -> Self {
        Self {
            version: CLAIM_VERSION,
            id: id.to_owned(),
            seen: false,
            corrupt: false,
            user: None,
            start: None,
            done: None,
        }
    }
}

struct ProjectedReader<'guard, 'session> {
    guard: &'guard HistoryGuard<'session>,
    stream: Stream,
    offset: u64,
    end: u64,
}

impl Read for ProjectedReader<'_, '_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() || self.offset >= self.end {
            return Ok(0);
        }
        let remaining = usize::try_from(self.end.saturating_sub(self.offset))
            .unwrap_or(usize::MAX)
            .min(buf.len());
        let bytes = self.guard.read_at(self.stream, self.offset, remaining)?;
        if bytes.is_empty() {
            return Err(Error::new(
                ErrorKind::UnexpectedEof,
                "projected session history ended before its length",
            ));
        }
        let read = bytes.len();
        buf.get_mut(..read)
            .ok_or_else(|| Error::other("projected session read exceeded buffer"))?
            .copy_from_slice(&bytes);
        self.offset = self
            .offset
            .checked_add(
                u64::try_from(read).map_err(|_error| Error::other("session read overflow"))?,
            )
            .ok_or_else(|| Error::other("session read overflow"))?;
        Ok(read)
    }
}

fn scan_claim_reader(
    mut reader: impl BufRead,
    mut offset: u64,
    stream: Stream,
    claim_dir: &Path,
) -> std::io::Result<u64> {
    let max_claim_bytes = usize::try_from(MAX_CLAIM_BYTES)
        .map_err(|_error| Error::other("session claim size limit overflow"))?;
    while let Some(line) = read_jsonl_line(&mut reader, max_claim_bytes)? {
        let bytes = line
            .len()
            .checked_add(1)
            .ok_or_else(|| Error::other("session claim offset overflow"))?;
        let end = offset
            .checked_add(
                u64::try_from(bytes)
                    .map_err(|_error| Error::other("session claim offset overflow"))?,
            )
            .ok_or_else(|| Error::other("session claim offset overflow"))?;
        #[cfg(test)]
        CLAIM_SCAN_BYTES.with(|value| value.set(value.get().saturating_add(bytes)));
        index_claim_line(claim_dir, stream, offset, end, &line)?;
        offset = end;
    }
    Ok(offset)
}

fn index_claim_line(
    claim_dir: &Path,
    stream: Stream,
    offset: u64,
    end: u64,
    line: &str,
) -> std::io::Result<()> {
    let value = serde_json::from_str::<Value>(line)
        .map_err(|_error| Error::new(ErrorKind::InvalidData, "invalid session history JSON"))?;
    let source = ClaimSource {
        offset,
        end,
        line: line.to_owned(),
    };
    match stream {
        Stream::Messages => index_message_claim(claim_dir, &value, source),
        Stream::Events => index_event_claim(claim_dir, &value, source),
    }
}

fn index_message_claim(
    claim_dir: &Path,
    value: &Value,
    source: ClaimSource,
) -> std::io::Result<()> {
    let Some(run) = value
        .get("run")
        .and_then(Value::as_str)
        .filter(|run| crate::is_object_name(run))
    else {
        return Ok(());
    };
    update_claim(claim_dir, run, |claim| {
        claim.seen = true;
        if value.get("role").and_then(Value::as_str) == Some("user") {
            let user = UserClaim {
                source,
                content: value
                    .get("content")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            };
            claim.corrupt |= user.content.is_none() || merge_claim_fact(&mut claim.user, user);
        }
    })
}

fn index_event_claim(claim_dir: &Path, value: &Value, source: ClaimSource) -> std::io::Result<()> {
    let run = value
        .get("run")
        .and_then(Value::as_str)
        .filter(|run| crate::is_object_name(run));
    if let Some(run) = run {
        update_claim(claim_dir, run, |claim| claim.seen = true)?;
    }
    let event_type = value.get("type").and_then(Value::as_str);
    if event_type == Some("start") {
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| crate::is_object_name(id));
        if let Some(id) = id {
            index_start_claim(claim_dir, id, id, run, value, source.clone())?;
        }
        if let Some(run) = run
            && Some(run) != id
        {
            index_start_claim(
                claim_dir,
                run,
                id.unwrap_or_default(),
                Some(run),
                value,
                source,
            )?;
        }
    } else if event_type == Some("done")
        && let Some(run) = run
    {
        let valid = matches!(
            value.get("status").and_then(Value::as_str),
            Some("ok" | "error" | "cancelled")
        );
        update_claim(claim_dir, run, |claim| {
            claim.corrupt |= !valid
                || merge_claim_fact(
                    &mut claim.done,
                    DoneClaim {
                        source: source.clone(),
                    },
                );
        })?;
    }
    Ok(())
}

fn index_start_claim(
    claim_dir: &Path,
    claim_id: &str,
    event_id: &str,
    run: Option<&str>,
    value: &Value,
    source: ClaimSource,
) -> std::io::Result<()> {
    let scope = value
        .get("scope")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let cwd = ClaimCwd::from_value(value.get("cwd"));
    let canonical =
        event_id == claim_id && run == Some(claim_id) && scope.is_some() && cwd.is_canonical();
    update_claim(claim_dir, claim_id, |claim| {
        claim.corrupt |= merge_claim_fact(
            &mut claim.start,
            StartClaim {
                source,
                canonical,
                scope,
                cwd,
            },
        );
    })
}

fn merge_claim_fact<T: Eq>(slot: &mut Option<T>, value: T) -> bool {
    if let Some(current) = slot.as_ref() {
        current != &value
    } else {
        *slot = Some(value);
        false
    }
}

fn update_claim(
    claim_dir: &Path,
    id: &str,
    update: impl FnOnce(&mut ClaimFile),
) -> std::io::Result<()> {
    let mut claim = read_claim(claim_dir, id)?.unwrap_or_else(|| ClaimFile::new(id));
    update(&mut claim);
    write_claim(claim_dir, &claim)
}

fn read_claim(claim_dir: &Path, id: &str) -> std::io::Result<Option<ClaimFile>> {
    if !crate::is_object_name(id) {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "invalid session claim id",
        ));
    }
    let path = claim_dir.join(id);
    match read_small_text_file(&path, MAX_CLAIM_BYTES) {
        Ok(content) => {
            let claim = serde_json::from_str::<ClaimFile>(&content)
                .map_err(|_error| Error::new(ErrorKind::InvalidData, "invalid session claim"))?;
            if claim.version != CLAIM_VERSION || claim.id != id {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    "session claim identity mismatch",
                ));
            }
            Ok(Some(claim))
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn write_claim(claim_dir: &Path, claim: &ClaimFile) -> std::io::Result<()> {
    let mut content = serde_json::to_string(claim).map_err(Error::other)?;
    content.push('\n');
    write_claim_text(&claim_dir.join(&claim.id), &content)
}

fn read_claim_cursor(claim_dir: &Path) -> std::io::Result<ClaimCursor> {
    match read_small_text_file(&claim_dir.join(CLAIM_CURSOR_FILE), MAX_CLAIM_CURSOR_BYTES) {
        Ok(content) => {
            let cursor = serde_json::from_str::<ClaimCursor>(&content).map_err(|_error| {
                Error::new(ErrorKind::InvalidData, "invalid session claim cursor")
            })?;
            if cursor.version != CLAIM_VERSION {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    "unsupported session claim cursor",
                ));
            }
            Ok(cursor)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(ClaimCursor::default()),
        Err(error) => Err(error),
    }
}

fn write_claim_cursor(claim_dir: &Path, cursor: &ClaimCursor) -> std::io::Result<()> {
    let mut content = serde_json::to_string(cursor).map_err(Error::other)?;
    content.push('\n');
    write_claim_text(&claim_dir.join(CLAIM_CURSOR_FILE), &content)
}

fn write_claim_text(path: &Path, content: &str) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            atomic_replace_text_with_mode(path, content, 0o600)
        }
        Ok(_) => Err(Error::new(
            ErrorKind::InvalidData,
            "session claim path is not a plain file",
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            atomic_create_text_with_mode(path, content, 0o600)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
fn fail_claim_cursor_write() -> std::io::Result<()> {
    if CLAIM_CURSOR_FAILURE.with(|value| value.replace(false)) {
        return Err(Error::other("injected session claim cursor failure"));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn reset_claim_scan_bytes() {
    CLAIM_SCAN_BYTES.with(|value| value.set(0));
}

#[cfg(test)]
pub(crate) fn claim_scan_bytes() -> usize {
    CLAIM_SCAN_BYTES.with(Cell::get)
}

#[cfg(test)]
pub(crate) fn set_claim_cursor_failure(fail: bool) {
    CLAIM_CURSOR_FAILURE.with(|value| value.set(fail));
}

fn append_marker_locked(
    path: &Path,
    expected: &MarkerSnapshot,
    lines: &[&str],
) -> std::io::Result<()> {
    validate_payloads(lines, ErrorKind::InvalidInput)?;
    let growth = lines.iter().try_fold(0_u64, |total, line| {
        let line_bytes = u64::try_from(line.len())
            .map_err(|_error| Error::other("session marker growth overflow"))?;
        total
            .checked_add(line_bytes)
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| Error::other("session marker growth overflow"))
    })?;
    let before_length = expected.length.get();
    let after_length = before_length
        .checked_add(growth)
        .ok_or_else(|| Error::other("session marker length overflow"))?;
    let mut file = OpenOptions::new()
        .append(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let before = file.metadata()?;
    if !expected.metadata_matches(&before, before_length) {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "session marker changed before append",
        ));
    }
    expected.verify_current(path)?;
    for line in lines {
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
    }
    file.flush()?;
    file.sync_all()?;
    let after = file.metadata()?;
    let current = fs::symlink_metadata(path)?;
    if !expected.metadata_matches(&after, after_length)
        || !expected.metadata_matches(&current, after_length)
    {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "session marker changed during append",
        ));
    }
    expected.length.set(after_length);
    Ok(())
}

/// Reads one projected stream as bounded UTF-8 text.
pub(crate) fn read_text(session: &Path, stream: Stream, max_bytes: u64) -> std::io::Result<String> {
    HistoryGuard::shared(session)?.read_text(stream, max_bytes)
}

/// Reads one bounded projected stream tail as UTF-8 text.
pub(crate) fn tail_text(
    session: &Path,
    stream: Stream,
    max_bytes: usize,
) -> std::io::Result<String> {
    String::from_utf8(HistoryGuard::shared(session)?.tail(stream, max_bytes)?)
        .map_err(|_error| Error::new(ErrorKind::InvalidData, "session history is not UTF-8"))
}

/// Durably appends complete JSONL line bodies to a session stream.
pub fn append(session: &Path, stream: Stream, lines: &[&str]) -> std::io::Result<()> {
    validate_payloads(lines, ErrorKind::InvalidInput)?;
    let _guard = HistoryGuard::exclusive(session)?;
    append_locked(session, stream, lines)
}

fn append_locked(session: &Path, stream: Stream, lines: &[&str]) -> std::io::Result<()> {
    validate_payloads(lines, ErrorKind::InvalidInput)?;
    let store = session.join(STORE_DIR);
    create_store_dir(&store)?;
    let wal = store.join(WAL_FILE);
    ensure_wal(&wal)?;
    repair_wal(&wal)?;
    migrate_legacy(session, &store, &wal)?;
    let mut ordinal = next_ordinal(&wal, stream)?;
    for line in lines {
        append_wal(&wal, stream, ordinal, line)?;
        ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| Error::other("session ordinal overflow"))?;
    }
    let manifest = read_manifest(&store)?;
    if wal_flush_threshold_reached(&wal, &manifest)? {
        flush_locked(session)?;
    }
    Ok(())
}

/// Commits all uncommitted WAL rows to bounded immutable Parquet shards.
pub fn flush(session: &Path) -> std::io::Result<()> {
    with_store_lock(session, FlockArg::LockExclusive, || flush_locked(session))
}

fn flush_locked(session: &Path) -> std::io::Result<()> {
    let store = session.join(STORE_DIR);
    create_store_dir(&store)?;
    create_store_dir(&store.join(DATA_DIR))?;
    create_store_dir(&store.join(INDEX_DIR))?;
    let wal = store.join(WAL_FILE);
    ensure_wal(&wal)?;
    repair_wal(&wal)?;
    loop {
        let mut manifest = read_manifest(&store)?;
        let rows = read_wal_batch(&wal, &manifest)?;
        if rows.is_empty() {
            if fs::metadata(&wal)?.len() > 0 {
                prune_wal(&store, &wal, &manifest)?;
            }
            return Ok(());
        }
        let shard = write_shard(&store, manifest.next_shard, &rows)?;
        truncate_indexes(&store, &manifest)?;
        append_shard_indexes(&store, &manifest, &shard)?;
        manifest.messages.add(shard.messages);
        manifest.events.add(shard.events);
        manifest.generation = manifest.generation.saturating_add(1);
        manifest.next_shard = manifest.next_shard.saturating_add(1);
        write_manifest(&store, &manifest)?;
        prune_wal(&store, &wal, &manifest)?;
    }
}

/// Reads at most `size` projected JSONL bytes at `offset`.
pub fn read_at(
    session: &Path,
    stream: Stream,
    offset: u64,
    size: usize,
) -> std::io::Result<Vec<u8>> {
    HistoryGuard::shared(session)?.read_at(stream, offset, size)
}

fn read_at_locked(
    session: &Path,
    stream: Stream,
    offset: u64,
    size: usize,
) -> std::io::Result<Vec<u8>> {
    let store = session.join(STORE_DIR);
    let manifest = read_committed_manifest(&store)?;
    if size == 0 {
        return Ok(Vec::new());
    }
    read_at_snapshot(&store, &manifest, stream, offset, size)
}

fn read_at_snapshot(
    store: &Path,
    manifest: &Manifest,
    stream: Stream,
    offset: u64,
    size: usize,
) -> std::io::Result<Vec<u8>> {
    let wal = store.join(WAL_FILE);
    let mut position = manifest.counts(stream).bytes;
    let mut output = Vec::with_capacity(size);
    if offset < position {
        match find_index_record(store, manifest, stream, offset)? {
            Some(mut index) => {
                let mut record = read_index_record(store, manifest, stream, index)?;
                position = record.start_byte;
                loop {
                    if record.start_byte != position {
                        return Err(Error::new(
                            ErrorKind::InvalidData,
                            "session index is not contiguous",
                        ));
                    }
                    let path = shard_path(store, record.shard_id);
                    read_shard_at(&path, stream, offset, size, &mut position, &mut output)?;
                    if output.len() == size {
                        return Ok(output);
                    }
                    if position != record.end_byte {
                        return Err(Error::new(
                            ErrorKind::InvalidData,
                            "session index does not match shard",
                        ));
                    }
                    index = index
                        .checked_add(1)
                        .ok_or_else(|| Error::other("session index overflow"))?;
                    if index >= manifest.head(stream).index_records {
                        break;
                    }
                    record = read_index_record(store, manifest, stream, index)?;
                }
            }
            None => {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    "session manifest has no matching index record",
                ));
            }
        }
    }
    position = manifest.counts(stream).bytes;
    scan_wal(&wal, |row| {
        if row.stream != stream || row.ordinal < manifest.counts(stream).lines {
            return Ok(());
        }
        append_projection(
            &mut output,
            offset,
            size,
            &mut position,
            row.payload.as_bytes(),
        )?;
        append_projection(&mut output, offset, size, &mut position, b"\n")?;
        Ok(())
    })?;
    Ok(output)
}

/// Returns the projected JSONL byte length for one stream.
pub fn len(session: &Path, stream: Stream) -> std::io::Result<u64> {
    HistoryGuard::shared(session)?.len(stream)
}

fn len_locked(session: &Path, stream: Stream) -> std::io::Result<u64> {
    let store = session.join(STORE_DIR);
    let manifest = read_committed_manifest(&store)?;
    len_snapshot(&store, &manifest, stream)
}

fn len_snapshot(store: &Path, manifest: &Manifest, stream: Stream) -> std::io::Result<u64> {
    let mut bytes = manifest.counts(stream).bytes;
    let wal = store.join(WAL_FILE);
    if !wal.is_file() {
        return Ok(bytes);
    }
    scan_wal(&wal, |row| {
        if row.stream != stream || row.ordinal < manifest.counts(stream).lines {
            return Ok(());
        }
        let row_bytes = u64::try_from(row.payload.len().saturating_add(1))
            .map_err(|_error| Error::other("session row too large"))?;
        bytes = bytes
            .checked_add(row_bytes)
            .ok_or_else(|| Error::other("session projection too large"))?;
        Ok(())
    })?;
    Ok(bytes)
}

/// Returns a bounded recent JSONL tail without a partial leading line.
pub fn tail(session: &Path, stream: Stream, max_bytes: usize) -> std::io::Result<Vec<u8>> {
    HistoryGuard::shared(session)?.tail(stream, max_bytes)
}

fn tail_locked(session: &Path, stream: Stream, max_bytes: usize) -> std::io::Result<Vec<u8>> {
    let store = session.join(STORE_DIR);
    let manifest = read_committed_manifest(&store)?;
    if max_bytes == 0 {
        return Ok(Vec::new());
    }
    let length = len_snapshot(&store, &manifest, stream)?;
    let limit = u64::try_from(max_bytes).map_err(|_error| Error::other("tail limit too large"))?;
    let offset = length.saturating_sub(limit);
    let mut bytes = read_at_snapshot(&store, &manifest, stream, offset, max_bytes)?;
    trim_partial_tail(offset, &mut bytes);
    Ok(bytes)
}

fn trim_partial_tail(offset: u64, bytes: &mut Vec<u8>) {
    if offset > 0
        && let Some(newline) = bytes.iter().position(|byte| *byte == b'\n')
    {
        bytes.drain(..=newline);
    }
}

/// Exports committed session history as an independent Parquet dataset.
pub fn export(session: &Path, output: &Path) -> std::io::Result<()> {
    with_store_lock(session, FlockArg::LockExclusive, || {
        export_locked(session, output)
    })
}

fn export_locked(session: &Path, output: &Path) -> std::io::Result<()> {
    flush_locked(session)?;
    let store = session.join(STORE_DIR);
    let manifest = read_manifest(&store)?;
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    let parent_dir = open_plain_directory(parent)?;
    let output_name = plain_file_name(output)?;
    let mut created = None;
    for attempt in 0_u8..16 {
        let name = generated_sibling_name(output_name, "export", attempt);
        let path = parent.join(&name);
        match create_plain_dir_exclusive(&path, 0o700) {
            Ok(directory) => {
                created = Some((name, path, directory));
                break;
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    let (temp_name, temp_path, temp_dir) = created.ok_or_else(|| {
        Error::new(
            ErrorKind::AlreadyExists,
            "cannot create unique export temp directory",
        )
    })?;
    let result = (|| {
        for shard_id in 0..manifest.next_shard {
            let source_path = shard_path(&store, shard_id);
            let name = source_path
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| Error::new(ErrorKind::InvalidData, "invalid session shard name"))?;
            let mut source = open_plain_file(&source_path)?;
            let mut destination = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(temp_path.join(name))?;
            std::io::copy(&mut source, &mut destination)?;
            fail_export_copy()?;
            destination.flush()?;
            destination.sync_all()?;
        }
        temp_dir.sync_all()?;
        nix::fcntl::renameat2(
            &parent_dir,
            temp_name.as_str(),
            &parent_dir,
            output_name,
            nix::fcntl::RenameFlags::RENAME_NOREPLACE,
        )
        .map_err(Error::from)?;
        parent_dir.sync_all()
    })();
    if result.is_err() {
        let _ignored = fs::remove_dir_all(&temp_path);
        let _ignored = parent_dir.sync_all();
    }
    result
}

fn fail_export_copy() -> std::io::Result<()> {
    if EXPORT_COPY_FAILURE.with(|value| value.replace(false)) {
        Err(Error::other("injected session export copy failure"))
    } else {
        Ok(())
    }
}

#[derive(Serialize)]
struct WalRow<'a> {
    stream: Stream,
    ordinal: u64,
    payload: &'a str,
}

#[derive(Deserialize, Serialize)]
struct WalRowOwned {
    stream: Stream,
    ordinal: u64,
    payload: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
struct Counts {
    lines: u64,
    bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
struct Head {
    lines: u64,
    bytes: u64,
    index_records: u64,
}

impl Head {
    fn counts(self) -> Counts {
        Counts {
            lines: self.lines,
            bytes: self.bytes,
        }
    }

    fn add(&mut self, counts: Counts) {
        self.lines = self.lines.saturating_add(counts.lines);
        self.bytes = self.bytes.saturating_add(counts.bytes);
        if counts.lines > 0 {
            self.index_records = self.index_records.saturating_add(1);
        }
    }
}

#[derive(Debug)]
struct ShardCommit {
    shard_id: u64,
    rows: u64,
    messages: Counts,
    events: Counts,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct IndexRecord {
    start_byte: u64,
    end_byte: u64,
    start_line: u64,
    line_count: u64,
    shard_id: u64,
    row_group: u64,
}

impl IndexRecord {
    fn encode(self) -> [u8; INDEX_RECORD_BYTES] {
        let mut bytes = [0_u8; INDEX_RECORD_BYTES];
        for (slot, value) in bytes.chunks_exact_mut(8).zip([
            self.start_byte,
            self.end_byte,
            self.start_line,
            self.line_count,
            self.shard_id,
            self.row_group,
        ]) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    fn decode(bytes: [u8; INDEX_RECORD_BYTES]) -> Self {
        let mut values = [0_u64; 6];
        for (value, slot) in values.iter_mut().zip(bytes.chunks_exact(8)) {
            let mut encoded = [0_u8; 8];
            encoded.copy_from_slice(slot);
            *value = u64::from_le_bytes(encoded);
        }
        Self {
            start_byte: values[0],
            end_byte: values[1],
            start_line: values[2],
            line_count: values[3],
            shard_id: values[4],
            row_group: values[5],
        }
    }
}

impl ShardCommit {
    fn counts(&self, stream: Stream) -> Counts {
        match stream {
            Stream::Messages => self.messages,
            Stream::Events => self.events,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct Manifest {
    version: u32,
    generation: u64,
    next_shard: u64,
    messages: Head,
    events: Head,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            version: 1,
            generation: 0,
            next_shard: 0,
            messages: Head::default(),
            events: Head::default(),
        }
    }
}

fn with_store_lock<T>(
    session: &Path,
    mode: FlockArg,
    action: impl FnOnce() -> std::io::Result<T>,
) -> std::io::Result<T> {
    let _guard = HistoryGuard::lock(session, mode)?;
    action()
}

#[cfg(test)]
pub(crate) fn with_test_store_lock<T>(
    session: &Path,
    exclusive: bool,
    action: impl FnOnce() -> std::io::Result<T>,
) -> std::io::Result<T> {
    let mode = if exclusive {
        FlockArg::LockExclusive
    } else {
        FlockArg::LockShared
    };
    with_store_lock(session, mode, action)
}

fn open_lock(path: &Path) -> std::io::Result<File> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_file() => {}
        Ok(_) => {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "session lock is not a plain file",
            ));
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            if let Err(create_error) = atomic_create_text_with_mode(path, "", 0o600)
                && !fs::symlink_metadata(path)
                    .is_ok_and(|metadata| !metadata.file_type().is_symlink() && metadata.is_file())
            {
                return Err(create_error);
            }
        }
        Err(error) => return Err(error),
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    file.sync_all()?;
    Ok(file)
}

fn open_shared_lock(path: &Path) -> std::io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "session lock is not a plain file",
        ));
    }
    Ok(file)
}

fn open_existing_shared_lock(store: &Path) -> std::io::Result<Option<File>> {
    match fs::symlink_metadata(store) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "session store is not a plain directory",
            ));
        }
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    }
    match open_shared_lock(&store.join(LOCK_FILE)) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
pub(crate) fn set_prune_barriers(barriers: Option<(Arc<Barrier>, Arc<Barrier>)>) {
    let slot = PRUNE_BARRIERS.get_or_init(|| Mutex::new(None));
    if let Ok(mut current) = slot.lock() {
        *current = barriers;
    }
}

#[cfg(test)]
pub(crate) fn set_legacy_snapshot_barrier(barrier: Option<LegacySnapshotBarrier>) {
    let slot = LEGACY_SNAPSHOT_BARRIER.get_or_init(|| Mutex::new(None));
    if let Ok(mut current) = slot.lock() {
        *current = barrier;
    }
}

fn wait_during_legacy_snapshot(session: &Path) {
    let _ = session;
    #[cfg(test)]
    {
        let barriers = LEGACY_SNAPSHOT_BARRIER
            .get_or_init(|| Mutex::new(None))
            .lock()
            .ok()
            .and_then(|mut value| {
                if value
                    .as_ref()
                    .is_some_and(|barrier| barrier.0.as_path() == session)
                {
                    value.take()
                } else {
                    None
                }
            });
        if let Some((_target, entered, release)) = barriers {
            entered.wait();
            release.wait();
        }
    }
}

fn wait_before_prune_replace() {
    #[cfg(test)]
    {
        let barriers = PRUNE_BARRIERS
            .get_or_init(|| Mutex::new(None))
            .lock()
            .ok()
            .and_then(|mut value| value.take());
        if let Some((entered, release)) = barriers {
            entered.wait();
            release.wait();
        }
    }
}

fn create_store_dir(store: &Path) -> std::io::Result<()> {
    let created = create_plain_dir_with(
        store,
        CreatePlainDirMessages {
            mode: 0o700,
            existing_not_dir_kind: ErrorKind::InvalidData,
            existing_not_dir_message: "session store is not a plain directory",
            contains_non_dir_kind: ErrorKind::InvalidData,
            contains_non_dir_message: "session store path contains a non-directory",
            invalid_name_message: "invalid session store path",
        },
    );
    if let Err(error) = created
        && !(error.kind() == ErrorKind::AlreadyExists
            && fs::symlink_metadata(store)
                .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink()))
    {
        return Err(error);
    }
    let directory = open_plain_directory(store)?;
    directory.set_permissions(fs::Permissions::from_mode(0o700))?;
    directory.sync_all()
}

fn migrate_legacy(session: &Path, store: &Path, wal: &Path) -> std::io::Result<()> {
    let manifest = store.join(MANIFEST_FILE);
    if fs::symlink_metadata(&manifest).is_ok_and(|metadata| metadata.is_file()) {
        clear_markers(session)?;
        return Ok(());
    }
    migrate_stream(session, wal, Stream::Messages)?;
    migrate_stream(session, wal, Stream::Events)?;
    if fs::metadata(wal)?.len() == 0 {
        write_manifest(store, &Manifest::default())?;
        return clear_markers(session);
    }
    flush_locked(session)?;
    clear_markers(session)
}

fn migrate_stream(session: &Path, wal: &Path, stream: Stream) -> std::io::Result<()> {
    let marker = session.join(stream.marker());
    let mut reader = BufReader::new(open_plain_file(&marker)?);
    let skip = next_ordinal(wal, stream)?;
    let mut ordinal = 0_u64;
    while let Some(line) = read_jsonl_line(&mut reader, MAX_PAYLOAD_BYTES)? {
        if ordinal >= skip {
            append_wal(wal, stream, ordinal, &line)?;
        }
        ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| Error::other("session ordinal overflow"))?;
    }
    Ok(())
}

fn append_wal(path: &Path, stream: Stream, ordinal: u64, payload: &str) -> std::io::Result<()> {
    validate_payload(payload, ErrorKind::InvalidInput)?;
    let buffer = encode_wal(&WalRow {
        stream,
        ordinal,
        payload,
    })?;
    let mut file = OpenOptions::new()
        .append(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "session WAL is not a plain file",
        ));
    }
    file.write_all(&buffer)?;
    file.flush()?;
    file.sync_all()
}

fn encode_wal(row: &impl Serialize) -> std::io::Result<Vec<u8>> {
    let mut buffer = serde_json::to_vec(row).map_err(Error::other)?;
    buffer.push(b'\n');
    Ok(buffer)
}

fn validate_payload(payload: &str, kind: ErrorKind) -> std::io::Result<()> {
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(Error::new(kind, "session row too large"));
    }
    if payload.contains(['\n', '\r']) {
        return Err(Error::new(kind, "session row contains a newline"));
    }
    serde_json::from_str::<Value>(payload)
        .map(|_value| ())
        .map_err(|_error| Error::new(kind, "session row is not one JSON value"))
}

fn validate_payloads(payloads: &[&str], kind: ErrorKind) -> std::io::Result<()> {
    payloads
        .iter()
        .try_for_each(|payload| validate_payload(payload, kind))
}

fn clear_markers(session: &Path) -> std::io::Result<()> {
    atomic_replace_text_with_mode(&session.join(Stream::Messages.marker()), "", 0o600)?;
    atomic_replace_text_with_mode(&session.join(Stream::Events.marker()), "", 0o600)
}

fn ensure_wal(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(path)?;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
            file.sync_all()
        }
        Ok(_) => Err(Error::new(
            ErrorKind::InvalidData,
            "session WAL is not a plain file",
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            atomic_create_text_with_mode(path, "", 0o600)
        }
        Err(error) => Err(error),
    }
}

#[derive(Clone, Copy)]
struct WalScan {
    committed_len: u64,
    torn_tail: bool,
}

fn scan_wal(
    path: &Path,
    mut visit: impl FnMut(&WalRowOwned) -> std::io::Result<()>,
) -> std::io::Result<WalScan> {
    let mut reader = BufReader::new(open_plain_file(path)?);
    let mut committed_len = 0_u64;
    loop {
        let mut bytes = Vec::new();
        let limit = u64::try_from(MAX_WAL_FRAME_BYTES.saturating_add(1))
            .map_err(|_error| Error::other("WAL frame limit too large"))?;
        let read = reader.by_ref().take(limit).read_until(b'\n', &mut bytes)?;
        if read == 0 {
            return Ok(WalScan {
                committed_len,
                torn_tail: false,
            });
        }
        if !bytes.ends_with(b"\n") {
            if bytes.len() > MAX_WAL_FRAME_BYTES {
                return Err(Error::new(ErrorKind::InvalidData, "WAL frame too large"));
            }
            return Ok(WalScan {
                committed_len,
                torn_tail: true,
            });
        }
        bytes.pop();
        if bytes.len() > MAX_WAL_FRAME_BYTES {
            return Err(Error::new(ErrorKind::InvalidData, "WAL frame too large"));
        }
        let frame = String::from_utf8(bytes)
            .map_err(|_error| Error::new(ErrorKind::InvalidData, "WAL frame is not UTF-8"))?;
        let row: WalRowOwned = serde_json::from_str(&frame)
            .map_err(|_error| Error::new(ErrorKind::InvalidData, "invalid committed WAL frame"))?;
        validate_payload(&row.payload, ErrorKind::InvalidData)?;
        visit(&row)?;
        committed_len = reader.stream_position()?;
    }
}

fn repair_wal(path: &Path) -> std::io::Result<()> {
    let scan = scan_wal(path, |_row| Ok(()))?;
    if !scan.torn_tail {
        return Ok(());
    }
    let file = OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "session WAL is not a plain file",
        ));
    }
    file.set_len(scan.committed_len)?;
    file.sync_all()
}

fn next_ordinal(path: &Path, stream: Stream) -> std::io::Result<u64> {
    let store = path
        .parent()
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "WAL has no store directory"))?;
    let manifest = read_manifest(store)?;
    let committed = manifest.counts(stream).lines;
    let mut next = committed;
    scan_wal(path, |row| {
        if row.stream == stream {
            next = next.max(
                row.ordinal
                    .checked_add(1)
                    .ok_or_else(|| Error::other("session ordinal overflow"))?,
            );
        }
        Ok(())
    })?;
    Ok(next)
}

impl Manifest {
    fn counts(&self, stream: Stream) -> Counts {
        self.head(stream).counts()
    }

    fn head(&self, stream: Stream) -> Head {
        match stream {
            Stream::Messages => self.messages,
            Stream::Events => self.events,
        }
    }
}

impl Stream {
    /// Maps one canonical session marker ABI path to its projected stream.
    #[must_use]
    pub fn from_abi_path(abi_path: &str) -> Option<Self> {
        match parse_abi_path(abi_path) {
            AbiPathKind::SessionFile {
                file: "messages.jsonl",
                ..
            } => Some(Self::Messages),
            AbiPathKind::SessionFile {
                file: "events.jsonl",
                ..
            } => Some(Self::Events),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Messages => "messages",
            Self::Events => "events",
        }
    }

    fn marker(self) -> &'static str {
        match self {
            Self::Messages => "messages.jsonl",
            Self::Events => "events.jsonl",
        }
    }
}

fn read_manifest(store: &Path) -> std::io::Result<Manifest> {
    let path = store.join(MANIFEST_FILE);
    match read_small_text_file(&path, MAX_MANIFEST_BYTES) {
        Ok(content) => serde_json::from_str(&content).map_err(Error::other),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(Manifest::default()),
        Err(error) => Err(error),
    }
}

fn index_path(store: &Path, stream: Stream) -> PathBuf {
    store
        .join(INDEX_DIR)
        .join(format!("{}.idx", stream.as_str()))
}

fn index_offset(index: u64) -> std::io::Result<u64> {
    index
        .checked_mul(
            u64::try_from(INDEX_RECORD_BYTES)
                .map_err(|_error| Error::other("index record size overflow"))?,
        )
        .ok_or_else(|| Error::other("session index offset overflow"))
}

fn open_index(path: &Path, writable: bool) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).custom_flags(libc::O_NOFOLLOW);
    if writable {
        options.write(true).create(true).mode(0o600);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "session index is not a plain file",
        ));
    }
    if writable {
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

fn truncate_indexes(store: &Path, manifest: &Manifest) -> std::io::Result<()> {
    for stream in [Stream::Messages, Stream::Events] {
        let file = open_index(&index_path(store, stream), true)?;
        file.set_len(index_offset(manifest.head(stream).index_records)?)?;
        file.sync_all()?;
    }
    Ok(())
}

fn append_shard_indexes(
    store: &Path,
    manifest: &Manifest,
    shard: &ShardCommit,
) -> std::io::Result<()> {
    if shard.messages.lines.saturating_add(shard.events.lines) != shard.rows {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "session shard row counts do not match",
        ));
    }
    for stream in [Stream::Messages, Stream::Events] {
        let counts = shard.counts(stream);
        if counts.lines == 0 {
            continue;
        }
        let head = manifest.head(stream);
        let record = IndexRecord {
            start_byte: head.bytes,
            end_byte: head
                .bytes
                .checked_add(counts.bytes)
                .ok_or_else(|| Error::other("session projection too large"))?,
            start_line: head.lines,
            line_count: counts.lines,
            shard_id: shard.shard_id,
            row_group: 0,
        };
        let mut file = open_index(&index_path(store, stream), true)?;
        file.seek(SeekFrom::Start(index_offset(head.index_records)?))?;
        file.write_all(&record.encode())?;
        file.flush()?;
        file.sync_all()?;
    }
    sync_plain_dir(&store.join(INDEX_DIR))
}

fn read_index_record(
    store: &Path,
    manifest: &Manifest,
    stream: Stream,
    index: u64,
) -> std::io::Result<IndexRecord> {
    let head = manifest.head(stream);
    if index >= head.index_records {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "session index record is outside manifest",
        ));
    }
    let file = open_index(&index_path(store, stream), false)?;
    let mut bytes = [0_u8; INDEX_RECORD_BYTES];
    file.read_exact_at(&mut bytes, index_offset(index)?)?;
    #[cfg(test)]
    INDEX_RECORD_READS.with(|value| value.set(value.get().saturating_add(1)));
    let record = IndexRecord::decode(bytes);
    let shard_committed = record.shard_id < manifest.next_shard;
    let valid = record.start_byte < record.end_byte
        && record.line_count > 0
        && shard_committed
        && record.row_group == 0;
    if !valid {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "invalid session index record",
        ));
    }
    Ok(record)
}

fn find_index_record(
    store: &Path,
    manifest: &Manifest,
    stream: Stream,
    offset: u64,
) -> std::io::Result<Option<u64>> {
    let mut low = 0_u64;
    let mut high = manifest.head(stream).index_records;
    while low < high {
        let middle = low + (high - low) / 2;
        if read_index_record(store, manifest, stream, middle)?.end_byte > offset {
            high = middle;
        } else {
            low = middle
                .checked_add(1)
                .ok_or_else(|| Error::other("session index overflow"))?;
        }
    }
    Ok((low < manifest.head(stream).index_records).then_some(low))
}

fn write_manifest(store: &Path, manifest: &Manifest) -> std::io::Result<()> {
    let path = store.join(MANIFEST_FILE);
    let mut content = serde_json::to_string(manifest).map_err(Error::other)?;
    content.push('\n');
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() => atomic_replace_text_with_mode(&path, &content, 0o600),
        Ok(_) => Err(Error::new(
            ErrorKind::InvalidData,
            "session manifest is not a plain file",
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            atomic_create_text_with_mode(&path, &content, 0o600)
        }
        Err(error) => Err(error),
    }
}

fn read_wal_batch(path: &Path, manifest: &Manifest) -> std::io::Result<Vec<WalRowOwned>> {
    let mut rows = Vec::with_capacity(MAX_SHARD_ROWS);
    let mut payload_bytes = 0_usize;
    let mut full = false;
    scan_wal(path, |row| {
        if row.ordinal < manifest.counts(row.stream).lines {
            return Ok(());
        }
        let next_bytes = payload_bytes
            .checked_add(row.payload.len())
            .ok_or_else(|| Error::other("session shard payload too large"))?;
        if !full
            && rows.len() < MAX_SHARD_ROWS
            && (rows.is_empty() || next_bytes <= MAX_SHARD_PAYLOAD_BYTES)
        {
            rows.push(WalRowOwned {
                stream: row.stream,
                ordinal: row.ordinal,
                payload: row.payload.clone(),
            });
            payload_bytes = next_bytes;
        } else {
            full = true;
        }
        Ok(())
    })?;
    Ok(rows)
}

fn wal_flush_threshold_reached(path: &Path, manifest: &Manifest) -> std::io::Result<bool> {
    let mut rows = 0_usize;
    let mut payload_bytes = 0_usize;
    scan_wal(path, |row| {
        if row.ordinal >= manifest.counts(row.stream).lines {
            rows = rows.saturating_add(1);
            payload_bytes = payload_bytes.saturating_add(row.payload.len());
        }
        Ok(())
    })?;
    Ok(rows >= MAX_SHARD_ROWS || payload_bytes >= MAX_SHARD_PAYLOAD_BYTES)
}

fn write_shard(store: &Path, index: u64, rows: &[WalRowOwned]) -> std::io::Result<ShardCommit> {
    let data = store.join(DATA_DIR);
    let name = format!("part-{index:06}.parquet");
    let final_path = data.join(&name);
    let schema = history_schema();
    let batch = history_batch(Arc::clone(&schema), rows)?;
    let properties = WriterProperties::builder()
        .set_max_row_group_row_count(Some(MAX_SHARD_ROWS))
        .build();
    let (temp_path, file) = create_unique_temp_file(&data, &name, "shard")?;
    let write_result = (|| {
        fail_temp_step(&SHARD_WRITE_FAILURE, "injected shard write failure")?;
        let mut writer =
            ArrowWriter::try_new(file, schema, Some(properties)).map_err(Error::other)?;
        writer.write(&batch).map_err(Error::other)?;
        writer.finish().map_err(Error::other)?;
        writer.inner().sync_all()?;
        drop(writer);
        fail_temp_step(&SHARD_RENAME_FAILURE, "injected shard rename failure")?;
        fs::rename(&temp_path, &final_path)?;
        sync_plain_dir(&data)
    })();
    if write_result.is_err() {
        let _ignored = fs::remove_file(&temp_path);
    }
    write_result?;

    let mut shard = ShardCommit {
        shard_id: index,
        rows: u64::try_from(rows.len()).map_err(|_error| Error::other("too many shard rows"))?,
        messages: Counts::default(),
        events: Counts::default(),
    };
    for row in rows {
        let bytes = u64::try_from(row.payload.len().saturating_add(1))
            .map_err(|_error| Error::other("session row too large"))?;
        let counts = match row.stream {
            Stream::Messages => &mut shard.messages,
            Stream::Events => &mut shard.events,
        };
        counts.lines = counts.lines.saturating_add(1);
        counts.bytes = counts.bytes.saturating_add(bytes);
    }
    Ok(shard)
}

fn history_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("stream", DataType::Utf8, false),
        Field::new("ordinal", DataType::UInt64, false),
        Field::new("kind", DataType::Utf8, true),
        Field::new("role", DataType::Utf8, true),
        Field::new("run", DataType::Utf8, true),
        Field::new("payload_json", DataType::Utf8, false),
    ]))
}

fn history_batch(schema: Arc<Schema>, rows: &[WalRowOwned]) -> std::io::Result<RecordBatch> {
    let streams = StringArray::from_iter_values(rows.iter().map(|row| row.stream.as_str()));
    let ordinals = UInt64Array::from_iter_values(rows.iter().map(|row| row.ordinal));
    let mut kinds = Vec::with_capacity(rows.len());
    let mut roles = Vec::with_capacity(rows.len());
    let mut runs = Vec::with_capacity(rows.len());
    for row in rows {
        let value = serde_json::from_str::<Value>(&row.payload).ok();
        kinds.push(json_field(value.as_ref(), "type"));
        roles.push(json_field(value.as_ref(), "role"));
        runs.push(json_field(value.as_ref(), "run"));
    }
    let payloads = StringArray::from_iter_values(rows.iter().map(|row| row.payload.as_str()));
    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(streams),
            Arc::new(ordinals),
            Arc::new(StringArray::from(kinds)),
            Arc::new(StringArray::from(roles)),
            Arc::new(StringArray::from(runs)),
            Arc::new(payloads),
        ],
    )
    .map_err(Error::other)
}

fn json_field(value: Option<&Value>, field: &str) -> Option<String> {
    value?.get(field).and_then(Value::as_str).map(str::to_owned)
}

fn read_shard_at(
    path: &Path,
    stream: Stream,
    offset: u64,
    size: usize,
    position: &mut u64,
    output: &mut Vec<u8>,
) -> std::io::Result<()> {
    #[cfg(test)]
    SHARD_OPENS.with(|value| value.set(value.get().saturating_add(1)));
    let file = open_plain_file(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file).map_err(Error::other)?;
    let reader = builder
        .with_batch_size(MAX_SHARD_ROWS)
        .build()
        .map_err(Error::other)?;
    for batch in reader {
        let batch = batch.map_err(Error::other)?;
        let streams = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "invalid parquet stream column"))?;
        let payloads = batch
            .column(5)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "invalid parquet payload column"))?;
        for index in 0..batch.num_rows() {
            if streams.value(index) != stream.as_str() {
                continue;
            }
            append_projection(
                output,
                offset,
                size,
                position,
                payloads.value(index).as_bytes(),
            )?;
            append_projection(output, offset, size, position, b"\n")?;
            if output.len() == size {
                return Ok(());
            }
        }
    }
    Ok(())
}

fn shard_path(store: &Path, shard_id: u64) -> PathBuf {
    store
        .join(DATA_DIR)
        .join(format!("part-{shard_id:06}.parquet"))
}

#[cfg(test)]
pub(crate) fn reset_read_counters() {
    INDEX_RECORD_READS.with(|value| value.set(0));
    SHARD_OPENS.with(|value| value.set(0));
}

#[cfg(test)]
pub(crate) fn read_counters() -> (usize, usize) {
    (
        INDEX_RECORD_READS.with(Cell::get),
        SHARD_OPENS.with(Cell::get),
    )
}

#[cfg(test)]
pub(crate) fn set_export_copy_failure(enabled: bool) {
    EXPORT_COPY_FAILURE.with(|value| value.set(enabled));
}

#[cfg(test)]
pub(crate) fn set_temp_failures(shard_write: bool, shard_rename: bool, prune_rename: bool) {
    SHARD_WRITE_FAILURE.with(|value| value.set(shard_write));
    SHARD_RENAME_FAILURE.with(|value| value.set(shard_rename));
    PRUNE_RENAME_FAILURE.with(|value| value.set(prune_rename));
}

fn fail_temp_step(
    flag: &'static std::thread::LocalKey<Cell<bool>>,
    message: &'static str,
) -> std::io::Result<()> {
    if flag.with(|value| value.replace(false)) {
        Err(Error::other(message))
    } else {
        Ok(())
    }
}

fn create_unique_temp_file(
    parent: &Path,
    target: &str,
    kind: &str,
) -> std::io::Result<(PathBuf, File)> {
    for attempt in 0_u8..16 {
        let path = parent.join(generated_sibling_name(target, kind, attempt));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(Error::new(
        ErrorKind::AlreadyExists,
        "cannot create unique session temp file",
    ))
}

fn prune_wal(store: &Path, wal: &Path, manifest: &Manifest) -> std::io::Result<()> {
    let (temp, file) = create_unique_temp_file(store, WAL_FILE, "prune")?;
    let prune_result = (|| {
        let mut output = BufWriter::new(file);
        scan_wal(wal, |row| {
            if row.ordinal >= manifest.counts(row.stream).lines {
                output.write_all(&encode_wal(row)?)?;
            }
            Ok(())
        })?;
        output.flush()?;
        output.get_ref().sync_all()?;
        drop(output);
        wait_before_prune_replace();
        fail_temp_step(&PRUNE_RENAME_FAILURE, "injected WAL prune rename failure")?;
        fs::rename(&temp, wal)?;
        sync_plain_dir(store)
    })();
    if prune_result.is_err() {
        let _ignored = fs::remove_file(&temp);
    }
    prune_result
}

fn append_projection(
    output: &mut Vec<u8>,
    offset: u64,
    size: usize,
    position: &mut u64,
    bytes: &[u8],
) -> std::io::Result<()> {
    let len = u64::try_from(bytes.len()).map_err(|_error| Error::other("row too large"))?;
    let end = position
        .checked_add(len)
        .ok_or_else(|| Error::other("session projection too large"))?;
    if end > offset && output.len() < size {
        let start = usize::try_from(offset.saturating_sub(*position).min(len))
            .map_err(|_error| Error::other("projection offset too large"))?;
        let take = (size - output.len()).min(bytes.len().saturating_sub(start));
        let range_end = start
            .checked_add(take)
            .ok_or_else(|| Error::other("projection range overflow"))?;
        let selected = bytes
            .get(start..range_end)
            .ok_or_else(|| Error::other("invalid projection range"))?;
        output.extend_from_slice(selected);
    }
    *position = end;
    Ok(())
}
