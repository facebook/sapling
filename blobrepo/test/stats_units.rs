// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::{HashMap, HashSet};
use std::fmt::Arguments;
use std::sync::{Arc, Mutex};

use failure::Error;
use slog::{self, Drain, Logger, OwnedKVList, Record, Serializer, KV};

use blobrepo::BlobRepo;
use changesets::SqliteChangesets;
use memblob::LazyMemblob;
use membookmarks::MemBookmarks;
use memheads::MemHeads;
use memlinknodes::MemLinknodes;
use mercurial_types::{RepoPath, RepositoryId};

use utils::{run_future, upload_file_no_parents};
// These are minimal tests, just confirming that the right sort of data comes out of our futures
// ready for later tests to depend on

// This errors if it sees the same key twice, and stores data that I've used before as typed KV
// pairs. I can use this to get nicer data out of a logging record.
struct GatherData {
    pub seen_keys: HashSet<String>,
    pub string_data: HashMap<String, String>,
    pub u64_data: HashMap<String, u64>,
    pub i64_data: HashMap<String, i64>,
}

impl GatherData {
    pub fn new() -> Self {
        Self {
            seen_keys: HashSet::new(),
            string_data: HashMap::new(),
            u64_data: HashMap::new(),
            i64_data: HashMap::new(),
        }
    }
}

impl Serializer for GatherData {
    fn emit_arguments(&mut self, key: slog::Key, _val: &Arguments) -> slog::Result {
        panic!("Unhandled data type for {}", key);
    }

    fn emit_u64(&mut self, key: slog::Key, val: u64) -> slog::Result {
        let key = String::from(key);
        assert!(self.seen_keys.insert(key.clone()), "Key {} seen twice", key);
        self.u64_data.insert(key, val);
        Ok(())
    }

    fn emit_i64(&mut self, key: slog::Key, val: i64) -> slog::Result {
        let key = String::from(key);
        assert!(self.seen_keys.insert(key.clone()), "Key {} seen twice", key);
        self.i64_data.insert(key, val);
        Ok(())
    }

    fn emit_str(&mut self, key: slog::Key, val: &str) -> slog::Result {
        let key = String::from(key);
        let val = String::from(val);
        assert!(self.seen_keys.insert(key.clone()), "Key {} seen twice", key);
        self.string_data.insert(key, val);
        Ok(())
    }
}

struct UploadBlobData {
    pub path: String,
    pub nodeid: String,
    pub poll_count: u64,
    pub poll_time_ms: i64,
    pub completion_time_ms: i64,
}

impl UploadBlobData {
    pub fn new(source: GatherData) -> Self {
        Self {
            path: source
                .string_data
                .get("path")
                .expect("No path supplied")
                .clone(),
            nodeid: source
                .string_data
                .get("nodeid")
                .expect("No nodeid supplied")
                .clone(),
            poll_count: *source
                .u64_data
                .get("poll_count")
                .expect("No poll_count supplied"),
            poll_time_ms: *source
                .i64_data
                .get("poll_time_ms")
                .expect("No poll_time_ms supplied"),
            completion_time_ms: *source
                .i64_data
                .get("completion_time_ms")
                .expect("No completion_time_ms supplied"),
        }
    }
}

struct UploadBlobTestDrain {
    pub records: Arc<Mutex<HashMap<String, UploadBlobData>>>,
}

impl UploadBlobTestDrain {
    pub fn new() -> Self {
        Self {
            records: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Drain for UploadBlobTestDrain {
    type Ok = ();
    type Err = Error;

    fn log(&self, record: &Record, _values: &OwnedKVList) -> Result<Self::Ok, Self::Err> {
        let mut source = GatherData::new();
        record
            .kv()
            .serialize(record, &mut source)
            .expect("Failed to serialize");
        assert_eq!(
            source.seen_keys,
            hashset!{"phase".into(),
            "path".into(),
            "nodeid".into(),
            "poll_count".into(),
            "poll_time_ms".into(),
            "completion_time_ms".into()},
            "Wrong set of keys supplied"
        );
        let mut records = self.records.lock().expect("Lock poisoned");
        let phase = source
            .string_data
            .get("phase")
            .expect("No phase supplied")
            .clone();
        let allowed_phases = hashset!{"content_uploaded", "finished"};
        assert!(
            allowed_phases.contains(phase.as_str()),
            "Illegal phase {}",
            phase
        );
        assert!(
            records
                .insert(phase.clone(), UploadBlobData::new(source))
                .is_none(),
            "Duplicate phase {}",
            phase
        );
        Ok(())
    }
}

fn get_logging_blob_repo(logger: Logger) -> BlobRepo {
    let bookmarks: MemBookmarks = MemBookmarks::new();
    let heads: MemHeads = MemHeads::new();
    let blobs = LazyMemblob::new();
    let linknodes = MemLinknodes::new();
    let changesets = SqliteChangesets::in_memory().expect("cannot create in memory changesets");
    let repoid = RepositoryId::new(0);

    BlobRepo::new_lazymemblob(
        Some(logger),
        heads,
        bookmarks,
        blobs,
        linknodes,
        changesets,
        repoid,
    )
}

#[test]
fn test_upload_blob_stats() {
    let drain = UploadBlobTestDrain::new();
    let records = drain.records.clone();
    let logger = Logger::root(drain.fuse(), o!("drain" => "test"));
    let repo = get_logging_blob_repo(logger);
    let fake_path = RepoPath::file("fakefile").expect("Can't generate fake RepoPath");

    let (nodeid, future) = upload_file_no_parents(&repo, "blob", &fake_path);
    let _ = run_future(future);
    for phase in vec!["content_uploaded", "finished"].into_iter() {
        let records = records.lock().expect("Lock poisoned");
        let data = records
            .get(phase.into())
            .expect(&format!("No records for phase {}", phase));
        assert_eq!(data.nodeid, format!("{}", nodeid));
        assert!(data.poll_count > 0, "Not polled before completion!");
        assert!(data.poll_time_ms >= 0, "Negative time in polling");
        assert!(data.completion_time_ms >= 0, "Negative time to completion");
    }
}
