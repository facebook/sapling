/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::path::Path;

#[cfg(unix)]
use once_cell::sync::Lazy;

use crate::errors::IOContext;
use crate::errors::IOResult;

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

pub fn atomic_write(path: &Path, op: impl FnOnce(&mut File) -> io::Result<()>) -> IOResult<File> {
    atomicfile::atomic_write(path, 0o644, false, op).path_context("error atomic writing file", path)
}

/// Open a path for atomic writing.
pub fn atomic_open(path: &Path) -> IOResult<atomicfile::AtomicFile> {
    atomicfile::AtomicFile::open(path, 0o644, false).path_context("error atomic opening file", path)
}

pub fn open(path: impl AsRef<Path>, mode: &str) -> IOResult<File> {
    let path = path.as_ref();

    let mut opts = OpenOptions::new();
    for opt in mode.chars() {
        match opt {
            'r' => opts.read(true),
            'w' => opts.write(true),
            'a' => opts.append(true),
            'c' => opts.create(true),
            't' => opts.truncate(true),
            'x' => opts.create_new(true),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("invalid open() mode {}", opt),
                ))
                .path_context("error opening file", path);
            }
        };
    }

    opts.open(path).path_context("error opening file", path)
}

pub fn create(path: impl AsRef<Path>) -> IOResult<File> {
    open(path, "wct")
}

pub fn read(path: impl AsRef<Path>) -> IOResult<Vec<u8>> {
    std::fs::read(path.as_ref()).path_context("error reading file", path.as_ref())
}

pub fn read_to_string(path: impl AsRef<Path>) -> IOResult<String> {
    std::fs::read_to_string(path.as_ref()).path_context("error reading file", path.as_ref())
}

#[cfg(test)]
mod test {
    use anyhow::Result;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_open_context() -> Result<()> {
        let dir = tempdir()?;

        let path = dir.path().join("doesnt").join("exist");
        let err_str = format!("{}", open(&path, "cwa").unwrap_err());

        // Make sure error contains path.
        assert!(err_str.contains(path.display().to_string().as_str()));

        // And the original error.
        let orig_err = format!("{}", std::fs::File::open(&path).unwrap_err());
        assert!(err_str.contains(&orig_err));

        Ok(())
    }
}
