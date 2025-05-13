/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
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
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use backingstore::BackingStore;
use backingstore::backingstore_global_init;
use minibench::Measure;
use minibench::bench;
use minibench::bench_enabled;
use minibench::measure;
use types::FetchContext;
use types::HgId;
use types::Key;
use types::RepoPathBuf;
use types::fetch_mode::FetchMode;

fn main() {
    // This sets up things like tracing logger, factory constructors, and other global tweaks eden does.
    backingstore_global_init();

    let n = load_test_keys().len();

    bench_matrix("get_blob serial (1k)", |store, mode| {
        for key in load_test_keys().iter().take(1000) {
            let fetched = store.get_blob(FetchContext::new(mode), key.hgid.as_ref());
            assert!(matches!(mode, FetchMode::LocalOnly) || matches!(fetched, Ok(Some(_))));
        }
    });

    bench_matrix(&format!("get_blob_batch ({}k)", n / 1000), |store, mode| {
        let fetch_count = AtomicUsize::new(0);
        store.get_blob_batch(
            FetchContext::new(mode),
            load_test_keys().clone(),
            |_, fetched| {
                fetch_count.fetch_add(1, Ordering::AcqRel);
                assert!(matches!(mode, FetchMode::LocalOnly) || matches!(fetched, Ok(Some(_))));
            },
        );
        assert_eq!(fetch_count.load(Ordering::Acquire), load_test_keys().len());
    });

    bench_matrix("get_file_aux serial (1k)", |store, mode| {
        for key in load_test_keys().iter().take(1000) {
            let fetched = store.get_file_aux(FetchContext::new(mode), key.hgid.as_ref());
            assert!(matches!(mode, FetchMode::LocalOnly) || matches!(fetched, Ok(Some(_))));
        }
    });

    bench_matrix(
        &format!("get_file_aux_batch ({}k)", n / 1000),
        |store, mode| {
            let fetch_count = AtomicUsize::new(0);
            store.get_file_aux_batch(
                FetchContext::new(mode),
                load_test_keys().clone(),
                |_, fetched| {
                    fetch_count.fetch_add(1, Ordering::AcqRel);
                    assert!(matches!(mode, FetchMode::LocalOnly) || matches!(fetched, Ok(Some(_))));
                },
            );
            assert_eq!(fetch_count.load(Ordering::Acquire), load_test_keys().len());
        },
    );

    let n = load_tree_keys().len();

    bench_matrix("get_tree serial (1k)", |store, mode| {
        for key in load_tree_keys().iter().take(1000) {
            let fetched = store.get_tree(FetchContext::new(mode), key.hgid.as_ref());
            assert!(matches!(mode, FetchMode::LocalOnly) || matches!(fetched, Ok(Some(_))));
        }
    });

    bench_matrix(&format!("get_tree_batch ({}k)", n / 1000), |store, mode| {
        let fetch_count = AtomicUsize::new(0);
        store.get_tree_batch(
            FetchContext::new(mode),
            load_tree_keys().clone(),
            |_, fetched| {
                fetch_count.fetch_add(1, Ordering::AcqRel);
                assert!(matches!(mode, FetchMode::LocalOnly) || matches!(fetched, Ok(Some(_))));
            },
        );
        assert_eq!(fetch_count.load(Ordering::Acquire), load_tree_keys().len());
    });

    bench_matrix("get_tree_aux serial (1k)", |store, mode| {
        for key in load_tree_keys().iter().take(1000) {
            let fetched = store.get_tree_aux(FetchContext::new(mode), key.hgid.as_ref());
            assert!(matches!(mode, FetchMode::LocalOnly) || matches!(fetched, Ok(Some(_))));
        }
    });

    bench_matrix(
        &format!("get_tree_aux_batch ({}k)", n / 1000),
        |store, mode| {
            let fetch_count = AtomicUsize::new(0);
            store.get_tree_aux_batch(
                FetchContext::new(mode),
                load_tree_keys().clone(),
                |_, fetched| {
                    fetch_count.fetch_add(1, Ordering::AcqRel);
                    assert!(matches!(mode, FetchMode::LocalOnly) || matches!(fetched, Ok(Some(_))));
                },
            );
            assert_eq!(fetch_count.load(Ordering::Acquire), load_tree_keys().len());
        },
    );

    eprintln!("Max RSS: {} MB", rss_mb());
}

/// Run benchmarks with local/remote * cold/warm matrix.
fn bench_matrix(name: &str, func: fn(&BackingStore, FetchMode)) {
    let title = format!("{name} (local, cold cache)");
    if bench_enabled(&title) {
        bench(&title, || {
            let dir = tempdir();
            let store = dir.store();
            measured(move || func(&store, FetchMode::LocalOnly))
        });
    }

    let title = format!("{name} (remote, cold cache)");
    if bench_enabled(&title) {
        bench(&title, || {
            let dir = tempdir();
            let store = dir.store();
            measured(move || func(&store, FetchMode::AllowRemote))
        });
    }

    let title = format!("{name} (local, warm cache)");
    if bench_enabled(&title) {
        let dir = tempdir();
        dir.warm_up(name);
        bench(&title, move || {
            let store = dir.store();
            measured(move || func(&store, FetchMode::LocalOnly))
        });
    }

    let title = format!("{name} (remote, warm cache)");
    if bench_enabled(&title) {
        let dir = tempdir();
        dir.warm_up(name);
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
    fn warm_up(&self, title: &str);
}

impl TempDirExt for tempfile::TempDir {
    fn store(&self) -> BackingStore {
        let cache_path = self.path();
        let mut configs = vec![
            format!("remotefilelog.cachepath={}", cache_path.display()),
            "experimental.tree-resolver-cache-size=0".to_string(),
            // This moves the tree aux cache into the shared cache so updating the
            // cachepath above will drop this cache as well.
            "scmstore.store-tree-aux-in-shared-cache=true".to_string(),
        ];
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
        BackingStore::new_with_config(root, &configs).unwrap()
    }

    fn warm_up(&self, test_title: &str) {
        let store = self.store();
        if test_title.contains("tree") {
            let keys = load_tree_keys();
            store.get_tree_batch(FetchContext::default(), keys.clone(), |_, _| ());
        } else {
            let keys = load_test_keys();
            store.get_blob_batch(FetchContext::default(), keys.clone(), |_, _| ());
        }
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

/// Load (path, node) pairs for tree tests.
fn load_tree_keys() -> &'static Vec<Key> {
    static KEYS: OnceLock<Vec<Key>> = OnceLock::new();
    KEYS.get_or_init(|| {
        let n: usize = match std::env::var("N") {
            Ok(n) => n.parse().unwrap_or(usize::MAX),
            _ => usize::MAX,
        };
        let test_input_path =
            std::env::var("TREE_KEYS").unwrap_or_else(|_| "test-trees.txt".to_owned());
        let data = fs::read_to_string(&test_input_path).unwrap();
        let mut keys = Vec::new();
        for hex_node in data.lines().take(n) {
            let id = HgId::from_hex(hex_node.as_bytes()).unwrap();
            let path = RepoPathBuf::new();
            keys.push(Key::new(path, id));
        }
        eprintln!("TREE_KEYS={}: {} trees", test_input_path, keys.len());
        keys
    })
}

/// Max RSS in MB.
fn rss_mb() -> u64 {
    procinfo::max_rss_bytes() >> 20
}
