/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Benchmark for some of the VFS features.

use std::hint::black_box;

use minibench::bench;
use minibench::elapsed;
use types::RepoPathBuf;
use vfs::FsFeatures;
use vfs::PathAuditor;

fn make_paths() -> Vec<(RepoPathBuf, bool)> {
    const MAX_DEPTH: usize = 16;
    const N: usize = 1024;
    let mut paths = Vec::with_capacity(MAX_DEPTH * N);
    for depth in 1..=MAX_DEPTH {
        for i in 0..N {
            let components: Vec<String> = (0..depth)
                .map(|d| {
                    // Sprinkle in .hg and .sl components occasionally.
                    if d == depth / 2 && i % 13 == 0 {
                        ".hg".to_string()
                    } else if d == depth / 2 && i % 17 == 0 {
                        ".sl".to_string()
                    } else {
                        format!("d{}", d * 4096 + i)
                    }
                })
                .collect();
            let path_str = components.join("/");
            let expect_ok = !path_str.split('/').any(|c| c == ".hg" || c == ".sl");
            paths.push((RepoPathBuf::from_string(path_str).unwrap(), expect_ok));
        }
    }
    paths
}

fn main() {
    let paths = make_paths();

    // Note: PathAuditor::audit is no longer used by the Rust VFS.
    bench("PathAuditor::audit".to_string(), || {
        let dir = tempfile::tempdir().unwrap();
        elapsed(|| {
            let auditor = PathAuditor::new(dir.path(), false);
            for (path, expect_ok) in &paths {
                let result = auditor.audit(path);
                debug_assert_eq!(result.is_ok(), *expect_ok);
                let _ = black_box(result);
            }
        })
    });

    for fs_features in [
        FsFeatures::empty(),
        FsFeatures::APPLE_NFD,
        FsFeatures::CASE_INSENSITIVE,
        FsFeatures::CASE_INSENSITIVE | FsFeatures::APPLE_NFD,
    ] {
        bench(format!("audit_components ({fs_features:?})"), || {
            let fs_features = black_box(fs_features);
            elapsed(|| {
                for (path, expect_ok) in &paths {
                    let result = vfs::audit_invalid_components(path.as_str(), fs_features);
                    debug_assert_eq!(result.is_ok(), *expect_ok);
                    let _ = black_box(result);
                }
            })
        })
    }
}

// Supports turning on tracing via LOG=...
dev_logger::init!();
