/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Configure indexedlog using Sapling configuration.

use std::sync::atomic;

use indexedlog::config::CHMOD_DIR;
use indexedlog::config::CHMOD_FILE;
use indexedlog::config::INDEX_CHECKSUM_MAX_CHAIN_LEN;
use indexedlog::config::INDEX_VERIFIED_CACHE;
use indexedlog::config::SYMLINK_ATOMIC_WRITE;
use indexedlog::config::WEAK_BUFFER_GC_THRESHOLD;
use indexedlog::config::set_global_fsync;
use indexedlog::config::set_index_lag_flush_timeout_secs;
use indexedlog::config::set_page_out_threshold;

/// Configure various settings based on a `Config`.
/// Settings are applied in order; an error leaves earlier settings applied.
pub fn configure(config: &dyn configmodel::Config) -> configmodel::Result<()> {
    use configmodel::ConfigExt;
    use configmodel::convert::ByteCount;

    if cfg!(unix) {
        let chmod_file = config.get_or("permissions", "chmod-file", || -1)?;
        if chmod_file >= 0 {
            CHMOD_FILE.store(chmod_file, atomic::Ordering::Release);
        }

        let chmod_dir = config.get_or("permissions", "chmod-dir", || -1)?;
        if chmod_dir >= 0 {
            CHMOD_DIR.store(chmod_dir, atomic::Ordering::Release);
        }

        let use_symlink_atomic_write: bool =
            config.get_or_default("format", "use-symlink-atomic-write")?;
        SYMLINK_ATOMIC_WRITE.store(use_symlink_atomic_write, atomic::Ordering::Release);

        #[cfg(all(unix, feature = "sigbus-handler"))]
        {
            let use_sigbus_handler: bool =
                config.get_or_default("unsafe", "indexedlog-zerofill-mmap-sigbus")?;
            if use_sigbus_handler {
                indexedlog::config::register_sigbus_handler();
            }
        }
    }

    if let Some(max_chain_len) =
        config.get_opt::<u32>("storage", "indexedlog-max-index-checksum-chain-len")?
    {
        INDEX_CHECKSUM_MAX_CHAIN_LEN.store(max_chain_len, atomic::Ordering::Release);
    }

    if let Some(enabled) = config.get_opt::<bool>("storage", "indexedlog-index-verified-cache")? {
        INDEX_VERIFIED_CACHE.store(enabled, atomic::Ordering::Release);
    }

    if let Some(value) =
        config.get_opt::<usize>("storage", "indexedlog-weak-buffer-gc-threshold")?
    {
        WEAK_BUFFER_GC_THRESHOLD.store(value, atomic::Ordering::Release);
    }

    if let Some(threshold) =
        config.get_opt::<ByteCount>("storage", "indexedlog-page-out-threshold")?
    {
        set_page_out_threshold(threshold.value() as _);
    }

    if let Some(timeout_secs) =
        config.get_opt::<u64>("storage", "indexedlog-lag-flush-timeout-secs")?
    {
        set_index_lag_flush_timeout_secs(timeout_secs);
    }

    let fsync: bool = config.get_or_default("storage", "indexedlog-fsync")?;
    set_global_fsync(fsync);

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use indexedlog::config::get_global_fsync;
    use indexedlog::config::get_index_lag_flush_timeout_secs;

    use super::*;

    #[test]
    fn test_configure() {
        let config = BTreeMap::from([
            ("storage.indexedlog-fsync", "true"),
            ("storage.indexedlog-index-verified-cache", "false"),
            ("storage.indexedlog-lag-flush-timeout-secs", "0"),
            ("storage.indexedlog-max-index-checksum-chain-len", "3"),
            ("storage.indexedlog-weak-buffer-gc-threshold", "7"),
        ]);
        configure(&config).unwrap();
        assert!(get_global_fsync());
        assert_eq!(get_index_lag_flush_timeout_secs(), 0);
        assert!(!INDEX_VERIFIED_CACHE.load(atomic::Ordering::Acquire));
        assert_eq!(
            INDEX_CHECKSUM_MAX_CHAIN_LEN.load(atomic::Ordering::Acquire),
            3
        );
        assert_eq!(WEAK_BUFFER_GC_THRESHOLD.load(atomic::Ordering::Acquire), 7);

        configure(&BTreeMap::<&str, &str>::new()).unwrap();
        assert!(!get_global_fsync());
        assert_eq!(
            INDEX_CHECKSUM_MAX_CHAIN_LEN.load(atomic::Ordering::Acquire),
            3
        );

        let invalid = BTreeMap::from([
            ("storage.indexedlog-max-index-checksum-chain-len", "9"),
            ("storage.indexedlog-fsync", "invalid"),
        ]);
        assert!(configure(&invalid).is_err());
        assert_eq!(
            INDEX_CHECKSUM_MAX_CHAIN_LEN.load(atomic::Ordering::Acquire),
            9
        );

        let sigbus = BTreeMap::from([("unsafe.indexedlog-zerofill-mmap-sigbus", "invalid")]);
        assert_eq!(
            configure(&sigbus).is_err(),
            cfg!(all(unix, feature = "sigbus-handler"))
        );
    }
}
