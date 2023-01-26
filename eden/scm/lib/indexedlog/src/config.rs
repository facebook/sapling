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
