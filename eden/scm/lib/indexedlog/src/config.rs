/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Default configs for indexed log.

use std::sync::LazyLock;
use std::sync::atomic;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::AtomicU64;

#[cfg(all(unix, feature = "sigbus-handler"))]
pub use crate::sigbus::register_sigbus_handler;

static UMASK: LazyLock<u32> = LazyLock::new(|| {
    #[cfg(unix)]
    unsafe {
        let umask = libc::umask(0);
        libc::umask(umask);
        #[allow(clippy::useless_conversion)] // mode_t is u16 on mac and u32 on linux
        return umask.into();
    }
    #[cfg(not(unix))]
    {
        return 0;
    }
});

fn apply_masked_umask(mode: u32, mask: u32) -> u32 {
    mode ^ (mask & *UMASK)
}

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
pub static CHMOD_DIR: LazyLock<AtomicI64> =
    LazyLock::new(|| AtomicI64::new(apply_masked_umask(0o2777, 0o7) as _));

// XXX: This works around https://github.com/Stebalien/tempfile/pull/61.
/// Default chmod mode for atomic_write files.
pub static CHMOD_FILE: LazyLock<AtomicI64> =
    LazyLock::new(|| AtomicI64::new(apply_masked_umask(0o666, 0o7) as _));

/// Default maximum chain length for index. See `index::OpenOptions::checksum_max_chain_len`.
pub static INDEX_CHECKSUM_MAX_CHAIN_LEN: AtomicU32 = AtomicU32::new(10);

/// Whether indexes remember verified checksum chunks across processes on the
/// same OS boot. See `index::OpenOptions::verified_cache`.
pub static INDEX_VERIFIED_CACHE: atomic::AtomicBool = atomic::AtomicBool::new(true);

/// How many `WeakBuffers.track` calls triggers cleaning up dropped weak buffers.
pub static WEAK_BUFFER_GC_THRESHOLD: atomic::AtomicUsize = atomic::AtomicUsize::new(16);

/// How long index lag may persist on disk before `Log::open` forces a flush.
/// A value of 0 disables the timeout-based correction.
static INDEX_LAG_FLUSH_TIMEOUT_SECS: AtomicU64 = AtomicU64::new(60);

/// Whether metadata written for newly-created logs is deterministic.
static DETERMINISTIC_METADATA: atomic::AtomicBool = atomic::AtomicBool::new(false);

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
/// When approximate `Log` reads exceed the limit, try to inform the kernel to
/// release the memory and reload the mmap buffers from disk later. Reads smaller
/// than the system page size are counted as one page to better approximate page
/// residency.
///
/// On Windows, use `EmptyWorkingSet` which affects the entire process.
/// On *nix, use `madvise(..., MADV_DONTNEED)` for all mmap buffers created by
/// this crate when `threshold > 0`.
///
/// The byte count is approximate: the kernel might map nearby pages through
/// fault-around, and repeated reads of the same region are counted more than
/// once.
///
/// The `madvise(..., MADV_DONTNEED)` might not take immediate effect.
/// See its manual page for details.
///
/// If `threshold <= 0`, auto page out is disabled.
pub fn set_page_out_threshold(threshold: i64) {
    let old_threshold = crate::page_out::THRESHOLD.swap(threshold, atomic::Ordering::AcqRel);
    let delta = threshold - old_threshold;
    crate::page_out::adjust_available(delta);
}

/// Set how long index lag may persist on disk before `Log::open` forces a flush.
/// A value of 0 disables the timeout-based correction.
pub fn set_index_lag_flush_timeout_secs(timeout_secs: u64) {
    INDEX_LAG_FLUSH_TIMEOUT_SECS.store(timeout_secs, atomic::Ordering::Release);
}

/// Get how long index lag may persist on disk before `Log::open` forces a flush.
pub fn get_index_lag_flush_timeout_secs() -> u64 {
    INDEX_LAG_FLUSH_TIMEOUT_SECS.load(atomic::Ordering::Acquire)
}

/// Make metadata for newly-created logs deterministic.
///
/// This is intended for fuzzers and tests that need byte-identical on-disk
/// seeds across processes. When enabled, new logs use a fixed epoch and do not
/// record wall-clock index-lag timestamps. Set it before opening any logs and
/// do not change it while logs are in use.
///
/// Not for production: a log deleted and recreated in place gets the same
/// epoch, so readers cannot reliably detect that non-append-only change, and the
/// index-lag flush timeout never fires.
pub fn set_deterministic_metadata(flag: bool) {
    DETERMINISTIC_METADATA.store(flag, atomic::Ordering::Release);
}

pub(crate) fn get_deterministic_metadata() -> bool {
    DETERMINISTIC_METADATA.load(atomic::Ordering::Acquire)
}
