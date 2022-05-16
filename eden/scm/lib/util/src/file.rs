/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs::File;
use std::io;
use std::path::Path;

#[cfg(unix)]
use once_cell::sync::Lazy;

#[cfg(unix)]
static UMASK: Lazy<u32> = Lazy::new(|| unsafe {
    let umask = libc::umask(0);
    libc::umask(umask);
    #[allow(clippy::useless_conversion)] // mode_t is u16 on mac and u32 on linux
    umask.into()
});

#[cfg(unix)]
pub fn apply_umask(mode: u32) -> u32 {
    mode & !*UMASK
}

pub fn atomic_write(path: &Path, op: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<File> {
    atomicfile::atomic_write(path, 0o644, false, op)
}

/// Open a path for atomic writing.
pub fn atomic_open(path: &Path) -> io::Result<atomicfile::AtomicFile> {
    atomicfile::AtomicFile::open(path, 0o644, false)
}
