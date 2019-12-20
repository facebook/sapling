/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::{capture_pattern, json, match_pattern};
use crate::event::Event;
use anyhow::Result;
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use indexedlog::log::IndexOutput;
use indexedlog::rotate::{OpenOptions, RotateLog, RotateLowLevelExt};
use lazy_static::lazy_static;
use serde_json::Value;
use std::cell::Cell;
use std::collections::BTreeSet;
use std::fs;
use std::io::{Cursor, Write};
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
pub struct Entry {
    pub timestamp: u64,
    pub session_id: u64,
    pub data: Event,

    // Prevent constructing `Entry` directly.
    phantom: (),
}

/// Convert to JSON Value for pattern matching.
pub trait ToValue {
    fn to_value(&self) -> Value;
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
            .auto_sync_threshold(1 << 21) // 20MB in-memory buffer
            .index("event", |bytes| {
                // Index on fields of `event`. This index includes fields from some dedicated
                // events. For example, timestamps of Start and Finish, etc.
                let mut result = Vec::new();
                let mut push = |head: u8, data: &[u8]| {
                    let mut bytes = vec![head];
                    bytes.write_all(data).expect("Vec::write should not fail");
                    result.push(IndexOutput::Owned(bytes.into_boxed_slice()));
                };
                if let Some(entry) = Entry::from_slice(bytes) {
                    match entry.data {
                        Event::Start {
                            timestamp_ms, pid, ..
                        } => {
                            push(INDEX_EVENT_START_TIME, &u64_to_slice(timestamp_ms)[..]);
                            push(INDEX_EVENT_START_PID, &u64_to_slice(pid as u64)[..]);
                        }
                        Event::Finish {
                            timestamp_ms,
                            duration_ms,
                            ..
                        } => {
                            push(INDEX_EVENT_FINISH_TIME, &u64_to_slice(timestamp_ms)[..]);
                            push(INDEX_EVENT_FINISH_DURATION, &u64_to_slice(duration_ms)[..]);
                        }
                        Event::Tags { names } => {
                            for name in names {
                                push(INDEX_EVENT_TAG_NAME, name.as_bytes());
                            }
                        }
                        _ => (),
                    }
                }
                result
            })
            .index("session_id", |_| {
                vec![IndexOutput::Reference(
                    TIMESTAMP_BYTES as u64..HEADER_BYTES as u64,
                )]
            })
            .create(true)
    }
}

const INDEX_EVENT_MISC: usize = 0;
const INDEX_SESSION_ID: usize = 1;

// Sub-index used by INDEX_EVENT_MISC.
const INDEX_EVENT_START_TIME: u8 = 0;
const INDEX_EVENT_START_PID: u8 = 1;
const INDEX_EVENT_FINISH_TIME: u8 = 2;
const INDEX_EVENT_FINISH_DURATION: u8 = 3;
const INDEX_EVENT_TAG_NAME: u8 = 4;

lazy_static! {
    static ref START_TIME_PATTERN: Value = json!(
        ["or",
         {"start": {"timestamp_ms": ["prefix", "range", ["capture", "START", "_"], ["capture", "END", "_"]]}},
         ["and",
          ["prefix", "and"],
          ["contain",
           {"start": {"timestamp_ms": ["prefix", "range", ["capture", "START", "_"], ["capture", "END", "_"]]}}]]]);
    static ref START_PID_PATTERN: Value = json!(
        ["or",
         {"start": {"pid": ["capture", "PID", "_"]}},
         ["and",
          ["prefix", "and"],
          ["contain",
           {"start": {"pid": ["capture", "PID", "_"]}}]]]);
    static ref FINISH_TIME_PATTERN: Value = json!(
        ["or",
         {"finish": {"timestamp_ms": ["prefix", "range", ["capture", "START", "_"], ["capture", "END", "_"]]}},
         ["and",
          ["prefix", "and"],
          ["contain",
           {"finish": {"timestamp_ms": ["prefix", "range", ["capture", "START", "_"], ["capture", "END", "_"]]}}]]]);
    static ref FINISH_DURATION_PATTERN: Value = json!(
        ["or",
         {"finish": {"duration_ms": ["prefix", "range", ["capture", "MIN", "_"], ["capture", "MAX", "_"]]}},
         ["and",
          ["prefix", "and"],
          ["contain",
           {"finish": {"duration_ms": ["prefix", "range", ["capture", "MIN", "_"], ["capture", "MAX", "_"]]}}]]]);
    static ref TAG_NAME_PATTERN: Value = json!(
        ["or",
         {"tags": {"names": ["prefix", "contain", ["capture", "NAME", "_"]]}},
         ["and",
          ["prefix", "and"],
          ["contain",
           {"tags": {"names": ["prefix", "contain", ["capture", "NAME", "_"]]}}]]]);
}

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

    pub fn session_id(&self) -> SessionId {
        SessionId(self.session_id)
    }

    /// Log an event. Maybe write it to disk immediately.
    ///
    /// If an error happens, `log` will try to rotate the bad logs and retry.
    /// If it still fails, `log` will simply give up.
    pub fn log(&mut self, data: &Event) {
        if self.is_broken.get() {
            return;
        }

        let now = time_to_u64(&SystemTime::now());
        if let Some(buf) = Entry::to_vec(data, now, self.session_id) {
            let _ = self.log.append(&buf);
        }
    }

    /// Write buffered data to disk.
    pub fn sync(&mut self) {
        if !self.is_broken.get() {
            // Ignore failures.
            let _ = self.log.sync();
        }
    }

    /// Filter blackbox by patterns.
    /// See `match_pattern.rs` for how to specify patterns.
    ///
    /// The pattern will match again the JSON form of an `Event`. For example,
    /// - Pattern `{"alias": {"from": "foo" }}` matches `Event::Alias { from, to }`
    ///   where `from` is `"foo"`.
    /// - Pattern `{"finish": {"duration_ms": ["range", 1000, 2000] }}` matches
    ///   `Event::Finish { duration_ms, ... }` where `duration_ms` is between
    ///   1000 and 2000.
    pub fn session_ids_by_pattern(&self, pattern: &Value) -> BTreeSet<SessionId> {
        let index: Option<(u8, _, _)> = capture_pattern(pattern, &START_TIME_PATTERN)
            .map(|captured| {
                let start = captured["START"].as_u64().unwrap_or(0);
                let end = captured["END"].as_u64().unwrap_or(0);
                (
                    INDEX_EVENT_START_TIME,
                    u64_to_boxed_slice(start),
                    u64_to_boxed_slice(start.max(end)),
                )
            })
            .or_else(|| {
                capture_pattern(pattern, &START_PID_PATTERN).map(|captured| {
                    let pid = captured["PID"].as_u64().unwrap_or(0);
                    (
                        INDEX_EVENT_START_PID,
                        u64_to_boxed_slice(pid),
                        u64_to_boxed_slice(pid),
                    )
                })
            })
            .or_else(|| {
                capture_pattern(pattern, &FINISH_TIME_PATTERN).map(|captured| {
                    let start = captured["START"].as_u64().unwrap_or(0);
                    let end = captured["END"].as_u64().unwrap_or(0);
                    (
                        INDEX_EVENT_FINISH_TIME,
                        u64_to_boxed_slice(start),
                        u64_to_boxed_slice(start.max(end)),
                    )
                })
            })
            .or_else(|| {
                capture_pattern(pattern, &FINISH_DURATION_PATTERN).map(|captured| {
                    let min = captured["MIN"].as_u64().unwrap_or(0);
                    let max = captured["MAX"].as_u64().unwrap_or(0);
                    (
                        INDEX_EVENT_FINISH_DURATION,
                        u64_to_boxed_slice(min),
                        u64_to_boxed_slice(max.max(min)),
                    )
                })
            })
            .or_else(|| {
                capture_pattern(pattern, &TAG_NAME_PATTERN).and_then(|captured| {
                    let name = captured["NAME"].as_str().unwrap_or("");
                    if name.is_empty() {
                        None
                    } else {
                        let bytes = name.as_bytes().to_vec().into_boxed_slice();
                        Some((INDEX_EVENT_TAG_NAME, bytes.clone(), bytes.clone()))
                    }
                })
            });

        let mut result = BTreeSet::new();
        match &index {
            Some((index_id, start, end)) => {
                // Use index to narrow down session_ids. Then search through the session_ids.
                // The real index key has the index_id has its header byte.
                let start: Vec<u8> = [&[*index_id][..], &start[..]].concat();
                let end: Vec<u8> = [&[*index_id][..], &end[..]].concat();
                let mut candidate_session_ids = Vec::new();
                for log in self.log.logs().iter() {
                    if let Ok(iter) = log.lookup_range(INDEX_EVENT_MISC, &start[..]..=&end[..]) {
                        for pair in iter {
                            if let Ok((_key, values)) = pair {
                                for value in values {
                                    if let Ok(bytes) = value {
                                        if let Some(session_id) =
                                            Entry::session_id_from_slice(bytes)
                                        {
                                            candidate_session_ids.push(session_id)
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                'next_session_id: for session_id in candidate_session_ids {
                    if let Ok(iter) = self
                        .log
                        .lookup(INDEX_SESSION_ID, &u64_to_slice(session_id.0)[..])
                    {
                        for bytes in iter {
                            if let Ok(bytes) = bytes {
                                if let Some(entry) = Entry::from_slice(bytes) {
                                    if entry.match_pattern(pattern) {
                                        result.insert(session_id);
                                        continue 'next_session_id;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            None => {
                // Cannot use index. Go through every entry.
                for next in self.log.iter() {
                    if let Ok(bytes) = next {
                        let session_id = match Entry::session_id_from_slice(bytes) {
                            Some(id) => id,
                            None => continue,
                        };
                        if result.contains(&session_id) {
                            // The session_id is already included in the result set.
                            // Skip deserializing it.
                            continue;
                        }
                        if let Some(entry) = Entry::from_slice(bytes) {
                            if entry.match_pattern(pattern) {
                                result.insert(session_id);
                            }
                        }
                    }
                }
            }
        }
        result
    }

    /// Get all [`Entry`]s with specified `session_id`s.
    ///
    /// This function is usually used together with `session_ids_by_pattern`.
    ///
    /// Entries that cannot be read or deserialized are ignored silently.
    pub fn entries_by_session_ids(
        &self,
        session_ids: impl IntoIterator<Item = SessionId>,
    ) -> Vec<Entry> {
        let mut result = Vec::new();
        for session_id in session_ids {
            if let Ok(iter) = self
                .log
                .lookup(INDEX_SESSION_ID, &u64_to_slice(session_id.0)[..])
            {
                for bytes in iter {
                    if let Ok(bytes) = bytes {
                        if let Some(entry) = Entry::from_slice(bytes) {
                            result.push(entry)
                        }
                    }
                }
            }
        }
        result.reverse();
        result
    }

    pub fn entries_by_session_id(&self, session_id: SessionId) -> Vec<Entry> {
        self.entries_by_session_ids(vec![session_id])
    }
}

/// Session Id used in public APIs.
#[derive(Copy, Clone, Ord, Eq, PartialOrd, PartialEq, Debug)]
pub struct SessionId(pub u64);

impl Drop for Blackbox {
    fn drop(&mut self) {
        self.sync();
    }
}

impl Entry {
    /// Test if `Entry` matches a specific pattern or not.
    pub fn match_pattern(&self, pattern: &Value) -> bool {
        match_pattern(&self.data.to_value(), pattern)
    }

    /// Partially decode `bytes` into session_id and timestamp.
    fn session_id_from_slice(bytes: &[u8]) -> Option<SessionId> {
        if bytes.len() >= HEADER_BYTES {
            let mut cur = Cursor::new(bytes);
            let _timestamp = cur.read_u64::<BigEndian>().unwrap();
            let session_id = cur.read_u64::<BigEndian>().unwrap();
            Some(SessionId(session_id))
        } else {
            None
        }
    }

    fn from_slice(bytes: &[u8]) -> Option<Self> {
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

impl Entry {
    fn to_vec(data: &Event, timestamp: u64, session_id: u64) -> Option<Vec<u8>> {
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

fn u64_to_slice(value: u64) -> [u8; 8] {
    // The field can be used for index range query. So it has to be BE.
    unsafe { std::mem::transmute(value.to_be()) }
}

fn u64_to_boxed_slice(value: u64) -> Box<[u8]> {
    (&u64_to_slice(value)[..]).to_vec().into_boxed_slice()
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
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn test_query_by_session_ids() {
        let dir = tempdir().unwrap();
        let mut blackbox = BlackboxOptions::new().open(&dir.path()).unwrap();

        let events = [
            Event::Alias {
                from: "a".to_string(),
                to: "b".to_string(),
            },
            Event::Debug {
                value: json!([1, 2, 3]),
            },
            Event::Alias {
                from: "x".to_string(),
                to: "y".to_string(),
            },
            Event::Debug {
                value: json!("foo"),
            },
            Event::Debug {
                value: json!({"p": "q"}),
            },
        ];

        // Write some events.
        // Session 0
        let mut session_ids = Vec::new();
        blackbox.log(&events[0]);
        blackbox.log(&events[1]);
        session_ids.push(blackbox.session_id());

        // Session 1
        blackbox.refresh_session_id();
        blackbox.log(&events[2]);
        blackbox.log(&events[3]);
        session_ids.push(blackbox.session_id());

        // Session 2
        blackbox.refresh_session_id();
        blackbox.log(&events[4]);
        session_ids.push(blackbox.session_id());

        let query = |pattern: serde_json::Value| -> Vec<usize> {
            let ids = blackbox.session_ids_by_pattern(&pattern);
            session_ids
                .iter()
                .enumerate()
                .filter(|(_i, session_id)| ids.contains(session_id))
                .map(|(i, _)| i)
                .collect()
        };

        // Query session_ids by patterns.
        // Only Session 0 and 1 have Event::Alias.
        assert_eq!(query(json!({"alias": "_"})), [0, 1]);
        // Only Session 1 has Event::Alias with from = "x".
        assert_eq!(query(json!({"alias": {"from": "x"}})), [1]);
        // All sessions have Event::Debug.
        assert_eq!(query(json!({"debug": "_"})), [0, 1, 2]);
        // Session 0 has Event::Debug with 2 in its "value" array.
        assert_eq!(query(json!({"debug": {"value": ["contain", 2]}})), [0]);
        // Session 2 has Event::Debug with the "p" key in its "value" object.
        assert_eq!(query(json!({"debug": {"value": {"p": "_"}}})), [2]);

        // Query Events by session_ids.
        let query = |i: usize| -> Vec<Event> {
            blackbox
                .entries_by_session_id(session_ids[i])
                .into_iter()
                .map(|e| e.data)
                .collect()
        };
        assert_eq!(query(0), &events[0..2]);
        assert_eq!(query(1), &events[2..4]);
        assert_eq!(query(2), &events[4..5]);
    }

    pub(crate) fn all_entries(blackbox: &Blackbox) -> Vec<Entry> {
        let session_ids = blackbox.session_ids_by_pattern(&json!("_"));
        session_ids
            .into_iter()
            .flat_map(|id| blackbox.entries_by_session_id(id))
            .collect()
    }
}
