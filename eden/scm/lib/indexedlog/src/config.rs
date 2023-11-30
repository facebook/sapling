/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Default configs for indexed log.

use std::sync::atomic;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::AtomicU32;

/// If set to true, prefer symlinks to normal files for atomic_write. This avoids
/// states where the metadata file is empty in theory.
///
/// Be careful with cases like mixing using ntfs-3g and Windows NTFS on files - they
/// might use different forms of symlink and are incompatible with this feature.
pub static SYMLINK_ATOMIC_WRITE: atomic::AtomicBool = atomic::AtomicBool::new(cfg!(test));

/// If set to true, enable fsync for writing.
static ENFORCE_FSYNC: atomic::AtomicBool = atomic::AtomicBool::new(false);

/// Default chmod mode for directories.
/// u: rwx g:rws o:r-x
pub static CHMOD_DIR: AtomicI64 = AtomicI64::new(0o2775);

// XXX: This works around https://github.com/Stebalien/tempfile/pull/61.
/// Default chmod mode for atomic_write files.
pub static CHMOD_FILE: AtomicI64 = AtomicI64::new(0o664);

/// Default maximum chain length for index. See `index::OpenOptions::checksum_max_chain_len`.
pub static INDEX_CHECKSUM_MAX_CHAIN_LEN: AtomicU32 = AtomicU32::new(10);

/// Set whether to fsync globally. fsync will be performed if either the local
/// or global fsync flag is set.
pub fn set_global_fsync(flag: bool) {
    ENFORCE_FSYNC.store(flag, atomic::Ordering::Release);
}

/// Get the fsync flag set by `set_global_fsync`.
pub fn get_global_fsync() -> bool {
    ENFORCE_FSYNC.load(atomic::Ordering::Acquire)
}

/// Set the "page_out" threshold in bytes.
///
/// When `Log` entry reads exceed the limit, try to inform the kernel to
/// release the memory and reload the mmap buffers from disk later.
///
/// On Windows, use `EmptyWorkingSet` which affects the entire process.
/// On *nix, use `madvise(..., MADV_DONTNEED)` for all mmap buffers
/// created by this crate when `threshold > 0`.
///
/// Note the byte count is an approximate:
/// - The kernel might read ahead.
/// - We don't de-duplicate reads of a same region.
/// - We don't count (frequent, small) index read for performance reasons.
/// The `madvise(..., MADV_DONTNEED)` might not take immediate effect.
/// See its manual page for details.
///
/// If `threshold <= 0`, auto page out is disabled.
pub fn set_page_out_threshold(threshold: i64) {
    let old_threshold = crate::page_out::THRESHOLD.swap(threshold, atomic::Ordering::AcqRel);
    let delta = threshold - old_threshold;
    crate::page_out::adjust_available(delta);
}

/// Configure various settings based on a `Config`.
#[cfg(feature = "configurable")]
pub fn configure(config: &dyn configmodel::Config) -> configmodel::Result<()> {
    use configmodel::convert::ByteCount;
    use configmodel::ConfigExt;

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

        let use_sigbus_handler: bool =
            config.get_or_default("unsafe", "indexedlog-zerofill-mmap-sigbus")?;
        #[cfg(all(unix, feature = "sigbus-handler"))]
        if use_sigbus_handler {
            crate::sigbus::register_sigbus_handler();
        }
    }

    if let Some(max_chain_len) =
        config.get_opt::<u32>("storage", "indexedlog-max-index-checksum-chain-len")?
    {
        INDEX_CHECKSUM_MAX_CHAIN_LEN.store(max_chain_len, atomic::Ordering::Release);
    }

    if let Some(threshold) =
        config.get_opt::<ByteCount>("storage", "indexedlog-page-out-threshold")?
    {
        set_page_out_threshold(threshold.value() as _);
    }

    let fsync: bool = config.get_or_default("storage", "indexedlog-fsync")?;
    set_global_fsync(fsync);

    Ok(())
}
