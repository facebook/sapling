/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Helpers shared by the Sapling on-disk format fuzz targets.
//!
//! The `indexedlog`, `metalog` and `revisionstore` harnesses all have the same
//! shape: build one pristine on-disk structure per process, then per iteration
//! write a fuzzer-corrupted copy of it into a fresh temporary directory and
//! re-open it. The patch-record decoder here is bounds-critical, so it lives in
//! one place rather than in a copy per harness package.

pub mod corrupt;

/// Configure indexedlog to write byte-identical metadata across processes.
///
/// Call this once, before creating the seed logs. The setting is process-wide
/// and should not be changed while logs are in use.
pub fn configure_indexedlog_for_fuzzing() {
    indexedlog::config::set_deterministic_metadata(true);
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use indexedlog::log::IndexOutput;
    use indexedlog::log::OpenOptions;

    use super::*;

    fn open_options() -> OpenOptions {
        OpenOptions::new()
            .create(true)
            .index("first", |_| vec![IndexOutput::Reference(0..1)])
    }

    fn write_log(dir: &Path) {
        let mut log = open_options().open(dir).expect("seed log should open");
        log.append(b"abc").expect("first entry should append");
        log.append(b"def").expect("second entry should append");
        log.sync().expect("seed log should sync");
    }

    #[test]
    fn indexedlog_metadata_is_deterministic() {
        configure_indexedlog_for_fuzzing();
        let a = tempfile::TempDir::new().expect("first temp dir should be created");
        let b = tempfile::TempDir::new().expect("second temp dir should be created");
        write_log(a.path());
        write_log(b.path());

        assert_eq!(
            fs::read(a.path().join("meta")).expect("first meta should be readable"),
            fs::read(b.path().join("meta")).expect("second meta should be readable")
        );

        let log = open_options()
            .open(a.path())
            .expect("deterministic seed log should reopen");
        let values: Vec<_> = log
            .lookup(0, b"a")
            .expect("lookup should succeed")
            .map(|value| value.expect("value should be readable").to_vec())
            .collect();
        assert_eq!(values, vec![b"abc".to_vec()]);
        assert_eq!(log.iter().count(), 2);
    }
}
