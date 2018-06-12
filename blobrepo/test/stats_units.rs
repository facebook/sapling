// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::{HashMap, HashSet};
use std::fmt::{self, Arguments};
use std::sync::{Arc, Mutex};

use async_unit;
use failure::Error;
use futures::Future;
use futures_ext::FutureExt;
use slog::{self, Drain, Logger, OwnedKVList, Record, Serializer, KV};

use blobrepo::BlobRepo;
use blobstore::LazyMemblob;
use mercurial_types::RepoPath;

use utils::{create_changeset_no_parents, run_future, upload_file_no_parents,
            upload_manifest_no_parents};

fn get_logging_blob_repo(logger: Logger) -> BlobRepo {
    BlobRepo::new_memblob_empty(Some(logger), Some(Arc::new(LazyMemblob::new())))
        .expect("cannot create BlobRepo")
}

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

macro_rules! recover_logged_data {
    ( $name:ident, $drain:ident; $( $x:ident ),+; $( $phase:expr ),+; $msg:expr) => {
        struct $name {
            $(
                pub $x: String,
            )*
            pub poll_count: u64,
            pub poll_time_us: u64,
            pub completion_time_us: u64,
        }

        impl $name {
            pub fn new(source: GatherData) -> Self {
                Self {
                    $(
                        $x: source
                            .string_data
                            .get(stringify!($x))
                            .expect(stringify!(No $x supplied))
                            .clone(),
                    )*
                    poll_count: *source
                        .u64_data
                        .get("poll_count")
                        .expect("No poll_count supplied"),
                    poll_time_us: *source
                        .u64_data
                        .get("poll_time_us")
                        .expect("No poll_time_us supplied"),
                    completion_time_us: *source
                        .u64_data
                        .get("completion_time_us")
                        .expect("No completion_time_us supplied"),
                }
            }
        }

        struct $drain {
            pub records: Arc<Mutex<HashMap<String, $name>>>,
        }

        impl $drain {
            pub fn new() -> Self {
                Self {
                    records: Arc::new(Mutex::new(HashMap::new())),
                }
            }
        }

        impl Drain for $drain {
            type Ok = ();
            type Err = Error;

            fn log(&self, record: &Record, _values: &OwnedKVList) -> Result<Self::Ok, Self::Err> {
                let msg = fmt::format(record.msg().clone());
                if msg != $msg {
                    return Ok(())
                }
                let mut source = GatherData::new();
                record
                    .kv()
                    .serialize(record, &mut source)
                    .expect("Failed to serialize");
                assert_eq!(
                    source.seen_keys,
                    hashset!{"phase".into(),
                    $(
                        stringify!($x).into(),
                    )*
                    "poll_count".into(),
                    "poll_time_us".into(),
                    "completion_time_us".into()},
                    "Wrong set of keys supplied"
                );
                let mut records = self.records.lock().expect("Lock poisoned");
                let phase = source
                    .string_data
                    .get("phase")
                    .expect("No phase supplied")
                    .clone();
                let allowed_phases: HashSet<String> = hashset!{$($phase.into(),)*};
                assert!(
                    allowed_phases.contains(phase.as_str()),
                    "Illegal phase {}",
                    phase
                );
                assert!(
                    records
                        .insert(phase.clone(), $name::new(source))
                        .is_none(),
                    "Duplicate phase {}",
                    phase
                );
                Ok(())
            }
        }
    };
}

macro_rules! check_stats {
    ( $records:ident, $data:ident; $( $phase:expr ),+; $data_checks:block ) => {
        for phase in vec![$($phase.into(),)*].into_iter() {
            let records = $records.lock().expect("Lock poisoned");
            let $data = records
                .get(phase)
                .expect(&format!("No records for phase {}", phase));
            $data_checks
            assert!($data.poll_count > 0, "Not polled before completion!");
        }
    };
    ( $records:ident, $data:ident; $( $phase:expr ),+ ) => {
        check_stats!($records, $data; $( $phase, )+; {} );
    }
}

recover_logged_data!(ChangesetsData, ChangesetsTestDrain;
    changeset_uuid;
    "upload_entries", "wait_for_parents_ready", "changeset_created", "parents_complete", "finished";
    "Changeset creation"
);

#[test]
fn test_create_changeset_stats() {
    async_unit::tokio_unit_test(|| {
        let drain = ChangesetsTestDrain::new();
        let records = drain.records.clone();
        let logger = Logger::root(drain.fuse(), o!("drain" => "test"));
        let repo = get_logging_blob_repo(logger);

        let fake_file_path = RepoPath::file("file").expect("Can't generate fake RepoPath");

        let (filehash, file_future) = upload_file_no_parents(&repo, "blob", &fake_file_path);
        let (_, root_manifest_future) =
            upload_manifest_no_parents(&repo, format!("file\0{}\n", filehash), &RepoPath::root());

        let commit = create_changeset_no_parents(
            &repo,
            root_manifest_future.map(Some).boxify(),
            vec![file_future],
        );
        let _ = run_future(commit.get_completed_changeset()).unwrap();

        let mut uuid = None;
        check_stats!(records, data;
            "upload_entries", "wait_for_parents_ready", "changeset_created", "parents_complete", "finished";
            {
                let changeset_uuid = Some(data.changeset_uuid.clone());
                if uuid.is_none() {
                    uuid = changeset_uuid;
                } else {
                    assert_eq!(uuid, changeset_uuid, "Changeset logging UUID not constant");
                }
            }
        );
    });
}
