/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;

use anyhow::Context;
use blob::Blob;
use configmodel::Config;
use configmodel::ConfigExt;
use configmodel::convert::ByteCount;
use indexedlog::log::IndexOutput;
use indexedlog::rotate::OpenOptions;
use indexedlog::rotate::RotateLog;
use types::Blake3;
use types::RepoPath;

/// Two rotated logs retain roughly one full cache of history after rotation.
const DEFAULT_LOG_COUNT: u8 = 2;
const DEFAULT_MAX_BYTES_PER_LOG: u64 = 4 * 1024 * 1024;

pub const CACHE_KEY_LEN: usize = Blake3::len();

pub type CacheKey = [u8; CACHE_KEY_LEN];

/// Persistent record of file content known to be lint clean.
///
/// Each entry is a fixed-size key derived from a file path, its content hash,
/// and a caller-provided epoch that can change to invalidate existing
/// entries. Log rotation bounds size and staleness.
pub struct LintCache {
    log: RotateLog,
}

impl LintCache {
    /// Open the cache in `dir`, creating it if missing.
    ///
    /// Log sizes follow the standard `indexedlog.filelint.*` tuning keys.
    pub fn open(dir: &Path, config: &dyn Config) -> anyhow::Result<Self> {
        let max_bytes_per_log = config
            .get_opt::<ByteCount>("indexedlog", "filelint.max-bytes-per-log")?
            .map_or(DEFAULT_MAX_BYTES_PER_LOG, |count| count.value())
            .max(1);
        let max_log_count = config
            .get_opt::<u8>("indexedlog", "filelint.max-log-count")?
            .unwrap_or(DEFAULT_LOG_COUNT)
            .max(1);
        let log = OpenOptions::new()
            .max_log_count(max_log_count)
            .max_bytes_per_log(max_bytes_per_log)
            .index("key", |_| {
                vec![IndexOutput::Reference(0..CACHE_KEY_LEN as u64)]
            })
            .create(true)
            .open(dir)
            .with_context(|| format!("opening lint clean-content cache `{}`", dir.display()))?;
        Ok(Self { log })
    }

    /// Derive the cache key identifying lint-clean `content` at `path` under `epoch`.
    pub fn key(path: &RepoPath, content: &Blake3, epoch: &[u8]) -> CacheKey {
        let path = path.as_byte_slice();
        let mut data = Vec::with_capacity(path.len() + 1 + CACHE_KEY_LEN + epoch.len());
        data.extend_from_slice(path);
        data.push(0);
        data.extend_from_slice(content.as_ref());
        data.extend_from_slice(epoch);
        Blob::from(data).blake3().into_byte_array()
    }

    pub fn contains(&self, key: &CacheKey) -> anyhow::Result<bool> {
        match self.log.lookup(0, key.to_vec())?.next() {
            Some(Ok(_entry)) => Ok(true),
            Some(Err(err)) => Err(err).context("reading lint clean-content cache"),
            None => Ok(false),
        }
    }

    /// Persist keys that are not already present.
    pub fn record(&mut self, keys: impl IntoIterator<Item = CacheKey>) -> anyhow::Result<()> {
        let mut added = false;
        for key in keys {
            if !self.contains(&key)? {
                // Fixed-size arrays are `Appendable`, writing straight into
                // the log buffer without an intermediate allocation.
                self.log.append(key)?;
                added = true;
            }
        }
        if added {
            self.log
                .sync()
                .context("writing lint clean-content cache")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tempfile::tempdir;
    use types::testutil::repo_path;

    use super::*;

    fn test_config() -> BTreeMap<&'static str, &'static str> {
        BTreeMap::new()
    }

    #[test]
    fn records_and_finds_persistent_keys() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let content = Blob::from_static(b"formatted\n").blake3();
        let key = LintCache::key(repo_path("src/a.py"), &content, b"epoch1");

        let mut cache = LintCache::open(dir.path(), &test_config())?;
        assert!(!cache.contains(&key)?);
        cache.record([key])?;
        assert!(cache.contains(&key)?);
        // Recording an existing key is a no-op rather than a duplicate entry.
        cache.record([key])?;

        let reopened = LintCache::open(dir.path(), &test_config())?;
        assert!(
            reopened.contains(&key)?,
            "cache entries should survive reopening"
        );
        Ok(())
    }

    #[test]
    fn respects_indexedlog_size_configuration() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let config = BTreeMap::from([
            ("indexedlog.filelint.max-bytes-per-log", "1"),
            ("indexedlog.filelint.max-log-count", "2"),
        ]);
        let content = Blob::from_static(b"formatted\n").blake3();
        let first = LintCache::key(repo_path("src/a.py"), &content, b"1");
        let last = LintCache::key(repo_path("src/a.py"), &content, b"9");

        let mut cache = LintCache::open(dir.path(), &config)?;
        for epoch in [b"1", b"2", b"3", b"4", b"5", b"6", b"7", b"8", b"9"] {
            cache.record([LintCache::key(repo_path("src/a.py"), &content, epoch)])?;
        }

        assert!(cache.contains(&last)?, "recent entries should survive");
        assert!(
            !cache.contains(&first)?,
            "tiny size limits should rotate out old entries"
        );
        Ok(())
    }
}
