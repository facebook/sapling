/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! To run the benchmarks, from the `benches` directory:
//!
//! ```bash,ignore
//! # use buck
//! buck2 run @//mode/opt :backingstore-benches
//! # or, use cargo internally:
//! cargo run --release --features fb
//! ```
//!
//! Append benchmark names to only run a subset of them.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use backingstore::BackingStore;
use backingstore::FetchMode;
use minibench::bench;
use minibench::bench_enabled;
use minibench::measure;
use minibench::Measure;
use types::HgId;
use types::Key;
use types::RepoPathBuf;

fn main() {
    let n = load_test_keys().len();

    bench_matrix("get_blob serial (1k)", |store, mode| {
        for key in load_test_keys().iter().take(1000) {
            let fetched = store.get_blob(key.hgid.as_ref(), mode);
            assert!(matches!(mode, FetchMode::LocalOnly) || matches!(fetched, Ok(Some(_))));
        }
    });

    bench_matrix(&format!("get_blob_batch ({}k)", n / 1000), |store, mode| {
        store.get_blob_batch(load_test_keys().clone(), mode, |_, fetched| {
            assert!(matches!(mode, FetchMode::LocalOnly) || matches!(fetched, Ok(Some(_))));
        })
    });

    bench_matrix("get_file_aux serial (1k)", |store, mode| {
        for key in load_test_keys().iter().take(1000) {
            let fetched = store.get_file_aux(key.hgid.as_ref(), mode);
            assert!(matches!(mode, FetchMode::LocalOnly) || matches!(fetched, Ok(Some(_))));
        }
    });

    bench_matrix(
        &format!("get_file_aux_batch ({}k)", n / 1000),
        |store, mode| {
            store.get_file_aux_batch(load_test_keys().clone(), mode, |_, fetched| {
                assert!(matches!(mode, FetchMode::LocalOnly) || matches!(fetched, Ok(Some(_))));
            })
        },
    );

    eprintln!("Max RSS: {} MB", rss_mb());
}

/// Run benchmarks with local/remote * cold/warm matrix.
fn bench_matrix(name: &str, func: fn(&BackingStore, FetchMode)) {
    bench(format!("{name} (local, cold cache)"), || {
        let dir = tempdir();
        let store = dir.store();
        measured(move || func(&store, FetchMode::LocalOnly))
    });

    bench(format!("{name} (remote, cold cache)"), || {
        let dir = tempdir();
        let store = dir.store();
        measured(move || func(&store, FetchMode::AllowRemote))
    });

    let title = format!("{name} (local, warm cache)");
    if bench_enabled(&title) {
        let dir = tempdir();
        dir.warm_up_files();
        bench(&title, move || {
            let store = dir.store();
            measured(move || func(&store, FetchMode::LocalOnly))
        });
    }

    let title = format!("{name} (remote, warm cache)");
    if bench_enabled(&title) {
        let dir = tempdir();
        dir.warm_up_files();
        bench(title, move || {
            let store = dir.store();
            measured(move || func(&store, FetchMode::AllowRemote))
        });
    }
}

/// Measure both wall clock and IO (Linux).
type M = measure::Both<measure::WallClock, measure::IO>;

fn measured(func: impl FnMut()) -> Result<M, String> {
    M::measure(func)
}

fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

trait TempDirExt {
    fn store(&self) -> BackingStore;
    fn warm_up_files(&self);
}

impl TempDirExt for tempfile::TempDir {
    fn store(&self) -> BackingStore {
        let cache_path = self.path();
        let mut configs = vec![format!("remotefilelog.cachepath={}", cache_path.display())];
        if let Ok(s) = std::env::var("CONFIGS") {
            for s in s.split_whitespace() {
                configs.push(s.to_string());
            }
        }
        let mut root = std::env::current_dir().unwrap();
        loop {
            match identity::sniff_dir(&root).unwrap() {
                Some(id) => {
                    let dot_path = root.join(id.dot_dir());
                    if let Some((shared_path, _ident)) =
                        repo::repo::read_sharedpath(&dot_path).unwrap()
                    {
                        root = shared_path;
                    }
                    break;
                }
                None => {
                    root = root.parent().unwrap().to_owned();
                }
            }
        }
        BackingStore::new_with_config(root, false, &configs).unwrap()
    }

    fn warm_up_files(&self) {
        let store = self.store();
        let keys = load_test_keys();
        store.get_blob_batch(keys.clone(), FetchMode::AllowRemote, |_, _| ());
        store.flush();
    }
}

/// Load (path, node) pairs for test input.
fn load_test_keys() -> &'static Vec<Key> {
    static KEYS: OnceLock<Vec<Key>> = OnceLock::new();
    KEYS.get_or_init(|| {
        let n: usize = match std::env::var("N") {
            Ok(n) => n.parse().unwrap_or(usize::MAX),
            _ => usize::MAX,
        };
        let test_input_path = std::env::var("KEYS").unwrap_or_else(|_| "test-paths.txt".to_owned());
        // Racy. But this is just a test.
        if !Path::new(&test_input_path).is_file() {
            let script_path = "gen-test-paths.py";
            if Path::new(script_path).is_file() {
                Command::new("sl")
                    .args(["debugshell", script_path])
                    .status()
                    .unwrap();
            }
        }
        let data = fs::read_to_string(&test_input_path).unwrap();
        let mut keys = Vec::new();
        for line in data.lines().take(n) {
            let (hex_node, path) = line.split_once(' ').unwrap();
            let id = HgId::from_hex(hex_node.as_bytes()).unwrap();
            let path = RepoPathBuf::from_string(path.to_owned()).unwrap();
            keys.push(Key::new(path, id));
        }
        eprintln!("KEYS={}: {} files", test_input_path, keys.len());
        keys
    })
}

/// Max RSS in MB.
fn rss_mb() -> u64 {
    procinfo::max_rss_bytes() >> 20
}
