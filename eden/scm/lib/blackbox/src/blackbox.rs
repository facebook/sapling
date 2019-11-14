/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::match_pattern;
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use failure::Fallible as Result;
use indexedlog::log::IndexOutput;
use indexedlog::rotate::{OpenOptions, RotateLog, RotateLowLevelExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cell::Cell;
use std::fs;
use std::io::Cursor;
use std::ops::Bound::{Excluded, Included, Unbounded};
use std::ops::RangeBounds;
use std::path::Path;
use std::time::SystemTime;

/// Local, rotated log consists of events tagged with "Invocation ID" and
/// timestamps.
pub struct Blackbox {
    // Used by singleton.
    pub(crate) log: RotateLog,
    opts: BlackboxOptions,

    // An ID that can be "grouped by" to figure everything about a session.
    pub(crate) session_id: u64,

    // The on-disk files are considered bad (ex. no permissions, or no disk space)
    // and further write attempts will be ignored.
    is_broken: Cell<bool>,

    // Timestamp of the last write operation. Used to reduce frequency of
    // log.sync().
    last_write_time: Cell<u64>,
}

#[derive(Copy, Clone)]
pub struct BlackboxOptions {
    max_bytes_per_log: u64,
    max_log_count: u8,
}

/// A wrapper for some serializable data.
///
/// It adds two fields: `timestamp` and `session_id`.
#[derive(Debug)]
pub struct Entry<T> {
    pub timestamp: u64,
    pub session_id: u64,
    pub data: T,

    // Prevent constructing `Entry` directly.
    phantom: (),
}

/// Convert to JSON Value for pattern matching.
pub trait ToValue {
    fn to_value(&self) -> Value;
}

/// Specify how to filter entries by indexes. Input of [`Blackbox::filter`].
pub enum IndexFilter {
    /// Filter by session ID.
    SessionId(u64),

    /// Filter by time range.
    Time(u64, u64),

    /// No filter. Get everything.
    Nop,
}

// The serialized format of `Entry` is:
//
// 8 Bytes: Milliseconds since epoch. Big-Endian.
// 4 Bytes: Session ID. Big-Endian.
// n Bytes: data.serialize() via serde-cbor.
//
// In case the format changes in the future, a simple strategy will be just
// renaming the directory used for logging.

const TIMESTAMP_BYTES: usize = 8;
const SESSION_ID_BYTES: usize = 8;
const HEADER_BYTES: usize = TIMESTAMP_BYTES + SESSION_ID_BYTES;

impl BlackboxOptions {
    /// Create a [`Blackbox`] instance at the given path using the specified options.
    pub fn open(self, path: impl AsRef<Path>) -> Result<Blackbox> {
        let path = path.as_ref();
        let opts = self.rotate_log_open_options();
        let log = match opts.clone().open(path) {
            Err(_) => {
                // Some error at opening (ex. metadata corruption).
                // As a simple recovery strategy, rmdir and retry.
                fs::remove_dir_all(path)?;
                opts.open(path)?
            }
            Ok(log) => log,
        };
        let blackbox = Blackbox {
            log,
            opts: self,
            // pid is used as an initial guess of "unique" session id
            session_id: new_session_id(),
            is_broken: Cell::new(false),
            last_write_time: Cell::new(0),
        };
        Ok(blackbox)
    }

    pub fn create_in_memory(self) -> Result<Blackbox> {
        let opts = self.rotate_log_open_options();
        let log = opts.create_in_memory()?;
        Ok(Blackbox {
            log,
            opts: self,
            // pid is used as an initial guess of "unique" session id
            session_id: new_session_id(),
            is_broken: Cell::new(false),
            last_write_time: Cell::new(0),
        })
    }

    pub fn new() -> Self {
        Self {
            max_bytes_per_log: 100_000_000,
            max_log_count: 3,
        }
    }

    pub fn max_bytes_per_log(mut self, bytes: u64) -> Self {
        self.max_bytes_per_log = bytes;
        self
    }

    pub fn max_log_count(mut self, count: u8) -> Self {
        self.max_log_count = count;
        self
    }

    fn rotate_log_open_options(&self) -> OpenOptions {
        OpenOptions::new()
            .max_bytes_per_log(self.max_bytes_per_log)
            .max_log_count(self.max_log_count)
            .index("timestamp", |_| {
                vec![IndexOutput::Reference(0..TIMESTAMP_BYTES as u64)]
            })
            .index("session_id", |_| {
                vec![IndexOutput::Reference(
                    TIMESTAMP_BYTES as u64..HEADER_BYTES as u64,
                )]
            })
            .create(true)
    }
}

const INDEX_TIMESTAMP: usize = 0;
const INDEX_SESSION_ID: usize = 1;

impl Blackbox {
    /// Assign a likely unused "Session ID".
    ///
    /// Events logged afterwards with be associated with this ID.
    ///
    /// Currently, uniqueness is not guaranteed, but perhaps "good enough".
    pub fn refresh_session_id(&mut self) {
        let session_id = new_session_id();
        if self.session_id >= session_id {
            self.session_id += 1 << 23;
        } else {
            self.session_id = session_id;
        }
    }

    /// Get the pid stored in session_id.
    pub(crate) fn session_pid(&self) -> u32 {
        (self.session_id & 0xffffff) as u32
    }

    pub fn session_id(&self) -> u64 {
        self.session_id
    }

    /// Log an event. Write it to disk immediately.
    ///
    /// If an error happens, `log` will try to rotate the bad logs and retry.
    /// If it still fails, `log` will simply give up.
    pub fn log(&mut self, data: &impl Serialize) {
        if self.is_broken.get() {
            return;
        }

        let now = time_to_u64(&SystemTime::now());
        if let Some(buf) = Entry::to_vec(data, now, self.session_id) {
            self.log.append(&buf).unwrap();

            // Skip sync() for frequent writes (within a threshold).
            let last = self.last_write_time.get();
            // On Linux, sync() takes 1-2ms. On Windows, sync() takes 100ms
            // (atomicwrite a file takes 20ms. That adds up).
            // Threshold is set so the sync() overhead is <2%.
            let threshold = if cfg!(windows) { 5000 } else { 100 };
            if last <= now && now - last < threshold {
                return;
            }
            self.last_write_time.set(now);

            if self.log.sync().is_err() {
                // Not fatal. Try rotate the log.
                if self.log.force_rotate().is_err() {
                    self.is_broken.set(true);
                } else {
                    // `force_rotate` might drop the data. Append again.
                    self.log.append(&buf).unwrap();
                    if self.log.sync().is_err() {
                        self.is_broken.set(true);
                    }
                }
            }
        }
    }

    /// Write buffered data to disk.
    pub fn sync(&mut self) {
        if !self.is_broken.get() {
            // Ignore failures.
            let _ = self.log.sync();
        }
    }

    /// IndexFilter entries. Newest first.
    ///
    /// - `filter` is backed by indexes.
    /// - `pattern` requires an expensive linear scan.
    ///
    /// Entries that cannot be read or deserialized are ignored silently.
    pub fn filter<'a, 'b: 'a, T: Deserialize<'a> + ToValue>(
        &'b self,
        filter: IndexFilter,
        pattern: Option<Value>,
    ) -> Vec<Entry<T>> {
        // API: Consider returning an iterator to get some laziness.
        let index_id = filter.index_id();
        let (start, end) = filter.index_range();
        let mut result = Vec::new();
        for log in self.log.logs().iter() {
            let range = (Included(&start[..]), Excluded(&end[..]));
            if let Ok(iter) = log.lookup_range(index_id, range) {
                for next in iter.rev() {
                    if let Ok((_key, entries)) = next {
                        for next in entries {
                            if let Ok(bytes) = next {
                                if let Some(entry) = Entry::from_slice(bytes) {
                                    if let Some(ref pattern) = pattern {
                                        let data: &T = &entry.data;
                                        let value = data.to_value();
                                        if !match_pattern(&value, pattern) {
                                            continue;
                                        }
                                    }
                                    result.push(entry)
                                }
                            }
                        }
                    }
                }
            }
        }
        result
    }
}

impl Drop for Blackbox {
    fn drop(&mut self) {
        self.sync();
    }
}

impl<'a, T: Deserialize<'a>> Entry<T> {
    fn from_slice(bytes: &'a [u8]) -> Option<Self> {
        if bytes.len() >= HEADER_BYTES {
            let mut cur = Cursor::new(bytes);
            let timestamp = cur.read_u64::<BigEndian>().unwrap();
            let session_id = cur.read_u64::<BigEndian>().unwrap();
            let pos = cur.position();
            let bytes = cur.into_inner();
            let bytes = &bytes[pos as usize..];
            if let Ok(data) = serde_cbor::from_slice(bytes) {
                let entry = Entry {
                    timestamp,
                    session_id,
                    data,
                    phantom: (),
                };
                return Some(entry);
            }
        }
        None
    }
}

impl<T: Serialize> Entry<T> {
    fn to_vec(data: &T, timestamp: u64, session_id: u64) -> Option<Vec<u8>> {
        let mut buf = Vec::with_capacity(32);
        buf.write_u64::<BigEndian>(timestamp).unwrap();
        buf.write_u64::<BigEndian>(session_id).unwrap();

        if serde_cbor::to_writer(&mut buf, data).is_ok() {
            Some(buf)
        } else {
            None
        }
    }
}

impl IndexFilter {
    fn index_id(&self) -> usize {
        match self {
            IndexFilter::SessionId(_) => INDEX_SESSION_ID,
            IndexFilter::Time(_, _) => INDEX_TIMESTAMP,
            IndexFilter::Nop => INDEX_TIMESTAMP,
        }
    }

    fn index_range(&self) -> (Box<[u8]>, Box<[u8]>) {
        match self {
            IndexFilter::SessionId(id) => (
                u64_to_slice(*id).to_vec().into_boxed_slice(),
                u64_to_slice(*id + 1).to_vec().into_boxed_slice(),
            ),
            IndexFilter::Time(start, end) => (
                u64_to_slice(*start).to_vec().into_boxed_slice(),
                u64_to_slice(*end).to_vec().into_boxed_slice(),
            ),
            IndexFilter::Nop => (
                u64_to_slice(0).to_vec().into_boxed_slice(),
                u64_to_slice(u64::max_value()).to_vec().into_boxed_slice(),
            ),
        }
    }
}

impl<T: RangeBounds<SystemTime>> From<T> for IndexFilter {
    fn from(range: T) -> IndexFilter {
        let start = match range.start_bound() {
            Included(v) => time_to_u64(v),
            Excluded(v) => time_to_u64(v) + 1,
            Unbounded => 0,
        };
        let end = match range.end_bound() {
            Included(v) => time_to_u64(v) + 1,
            Excluded(v) => time_to_u64(v),
            Unbounded => u64::max_value(),
        };
        IndexFilter::Time(start, end)
    }
}

fn u64_to_slice(value: u64) -> [u8; 8] {
    // The field can be used for index range query. So it has to be BE.
    unsafe { std::mem::transmute(value.to_be()) }
}

fn time_to_u64(time: &SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

// The session_id is intended to be:
// 1. Somehow unique among multiple machines for at least 3 months
//    (for analysis over time).
// 2. Related to timestamp. So Scuba might be able to delta-compress them.
//
// At the time of writing, millisecond percision seems already enough to
// distinguish sessions across machines. To make it more "future proof", take
// some bits from the pid.
//
// At the time of writing, /proc/sys/kernel/pid_max shows pid can fit in 3
// bytes.
fn new_session_id() -> u64 {
    // 40 bits from millisecond timestamp. That's 34 years.
    // 24 bits from pid.
    ((time_to_u64(&SystemTime::now()) & 0xffffffffff) << 24)
        | ((unsafe { libc::getpid() } as u64) & 0xffffff)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_derive::{Deserialize, Serialize};
    use std::collections::HashSet;
    use std::{
        fs,
        io::{Seek, SeekFrom, Write},
    };
    use tempfile::tempdir;

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    pub(crate) enum Event {
        A(u64),
        B(String),
    }

    impl ToValue for Event {
        fn to_value(&self) -> Value {
            serde_json::to_value(self).unwrap()
        }
    }

    #[test]
    fn test_basic() {
        let time_start = SystemTime::now();
        let dir = tempdir().unwrap();
        let mut blackbox = BlackboxOptions::new().open(&dir.path().join("1")).unwrap();
        let events = vec![Event::A(0), Event::B("Foo".to_string()), Event::A(12)];

        let session_count = 4;
        let first_session_id = blackbox.session_id();
        for _ in 0..session_count {
            for event in events.iter() {
                blackbox.log(event);
                let mut blackbox = BlackboxOptions::new().open(&dir.path().join("2")).unwrap();
                blackbox.log(event);
            }
            blackbox.refresh_session_id();
        }
        let time_end = SystemTime::now();

        // Test find by session id.
        assert_eq!(
            blackbox
                .filter::<Event>(IndexFilter::SessionId(first_session_id), None)
                .len(),
            events.len()
        );

        // Test find by time range.
        let entries = blackbox.filter::<Event>((time_start..=time_end).into(), None);

        // The time range covers everything, so it should match "find all".
        assert_eq!(
            blackbox.filter::<Event>(IndexFilter::Nop, None).len(),
            entries.len()
        );
        assert_eq!(entries.len(), events.len() * session_count);
        assert_eq!(
            entries
                .iter()
                .map(|e| e.session_id)
                .collect::<HashSet<_>>()
                .len(),
            session_count,
        );

        // Entries match data (events), and are in the "newest first" order.
        for (entry, event) in entries
            .iter()
            .rev()
            .zip((0..session_count).flat_map(|_| events.iter()))
        {
            assert_eq!(&entry.data, event)
        }

        // Check logging with multiple blackboxes.
        let blackbox = BlackboxOptions::new().open(&dir.path().join("2")).unwrap();
        assert_eq!(
            blackbox.filter::<Event>(IndexFilter::Nop, None).len(),
            entries.len()
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_data_corruption() {
        let dir = tempdir().unwrap();
        let mut blackbox = BlackboxOptions::new().open(&dir.path()).unwrap();
        let events: Vec<_> = (1..=400).map(|i| Event::B(format!("{:030}", i))).collect();

        for event in events.iter() {
            blackbox.log(event);
        }

        let entries = blackbox.filter::<Event>(IndexFilter::Nop, None);
        assert_eq!(entries.len(), events.len());
        blackbox.sync();

        // Corrupt log
        let log_path = dir.path().join("0").join("log");
        backup(&log_path);
        for (bytes, corrupted_count) in [(1, 1), (60, 2), (160, 3)].iter() {
            // Corrupt the last few bytes.
            corrupt(&log_path, *bytes);

            // The other entries can still be read without errors.
            let entries = blackbox.filter::<Event>(IndexFilter::Nop, None);
            assert_eq!(entries.len(), events.len() - corrupted_count);
            assert!(entries
                .iter()
                .rev()
                .map(|e| &e.data)
                .eq(events.iter().take(entries.len())));
        }
        restore(&log_path);

        // Corrupt index.
        let index_path = dir.path().join("0").join("index-timestamp");
        corrupt(&index_path, 1);

        // Requires a reload of the blackbox so the in-memory checksum table
        // gets updated.
        let blackbox = BlackboxOptions::new().open(&dir.path()).unwrap();
        let entries = blackbox.filter::<Event>(IndexFilter::Nop, None);

        // Loading this Log would trigger a rewrite.
        // TODO: Add some auto-recovery logic to the indexes on `Log`.
        assert!(entries.is_empty());
    }

    /// Corrupt data at the end.
    fn corrupt(path: &Path, size: usize) {
        let mut file = fs::OpenOptions::new()
            .read(true)
            .create(false)
            .write(true)
            .open(path)
            .unwrap();
        file.seek(SeekFrom::End(-(size as i64))).unwrap();
        file.write_all(&vec![0; size]).unwrap();
    }

    fn backup(path: &Path) {
        fs::copy(path, path.with_extension("bak")).unwrap();
    }

    fn restore(path: &Path) {
        fs::copy(path.with_extension("bak"), path).unwrap();
    }
}
