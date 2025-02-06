/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::path::Path;

use ::metrics::Counter;
use once_cell::sync::Lazy;

use crate::errors::IOContext;

static MAX_IO_RETRIES: Lazy<u32> = Lazy::new(|| {
    std::env::var("SL_IO_RETRIES")
        .unwrap_or("3".to_string())
        .parse::<u32>()
        .unwrap_or(3)
});

static FILE_UTIL_RETRY_SUCCESS: Counter = Counter::new_counter("util.file_retry_success");
static FILE_UTIL_RETRY_FAILURE: Counter = Counter::new_counter("util.file_retry_failure");

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
    // Can't implement retries on IO timeouts because op is FnOnce
    atomicfile::atomic_write(path, 0o644, false, op).path_context("error atomic writing file", path)
}

/// Open a path for atomic writing.
pub fn atomic_open(path: &Path) -> io::Result<atomicfile::AtomicFile> {
    let mut open_fn = |p: &Path| -> io::Result<atomicfile::AtomicFile> {
        atomicfile::AtomicFile::open(p, 0o644, false)
    };

    match with_retry(&mut open_fn, path) {
        Ok(m) => Ok(m),
        Err(err) => Err(err).path_context("error opening file", path),
    }
}

pub fn open(path: impl AsRef<Path>, mode: &str) -> io::Result<File> {
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
    let mut open_fn = |p: &Path| -> io::Result<File> { opts.open(p) };

    match with_retry(&mut open_fn, path) {
        Ok(m) => Ok(m),
        Err(err) => Err(err).path_context("error opening file", path),
    }
}

pub fn create(path: impl AsRef<Path>) -> io::Result<File> {
    open(path, "wct")
}

fn is_retryable(err: &io::Error) -> bool {
    cfg!(target_os = "macos") && err.kind() == io::ErrorKind::TimedOut
}

fn with_retry<'a, F, T>(io_operation: &mut F, path: &'a Path) -> io::Result<T>
where
    F: FnMut(&'a Path) -> io::Result<T>,
{
    let mut retries: u32 = 0;
    loop {
        match io_operation(path) {
            Ok(v) => {
                if retries > 0 {
                    FILE_UTIL_RETRY_SUCCESS.increment();
                }
                return Ok(v);
            }
            Err(err) if is_retryable(&err) => {
                if retries >= *MAX_IO_RETRIES {
                    if retries > 0 {
                        FILE_UTIL_RETRY_FAILURE.increment();
                    }
                    return Err(err);
                }
                retries += 1;
            }
            Err(err) => return Err(err),
        }
    }
}

pub fn exists(path: impl AsRef<Path>) -> io::Result<Option<std::fs::Metadata>> {
    let path = path.as_ref();
    match with_retry(&mut std::fs::metadata, path) {
        Ok(m) => Ok(Some(m)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).path_context("error reading file", path),
    }
}

pub fn unlink_if_exists(path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref();
    match with_retry(&mut std::fs::remove_file, path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).path_context("error deleting file", path),
    }
}

pub fn read_to_string_if_exists(path: impl AsRef<Path>) -> io::Result<Option<String>> {
    let path = path.as_ref();
    match with_retry(&mut std::fs::read_to_string, path) {
        Ok(contents) => Ok(Some(contents)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).path_context("error reading file", path),
    }
}

#[cfg(test)]
mod test {
    use std::io::Read;

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

    #[test]
    fn test_retry_io() -> Result<()> {
        // NOTE: These test cases are run together because running them separately sometimes fails
        // when testing with cargo. This is because the tests are run in parallel, and
        // std::evn::var() can overwrite environment variable values for other running tests.
        //
        // To avoid this, we run the tests serially in a single test case.
        let dir = tempdir()?;
        let path = dir.path().join("test");
        std::fs::write(&path, "test")?;
        let mut retries = 0;

        let mut open_fn = |p: &Path| -> io::Result<File> {
            retries += 1;
            if retries >= (*MAX_IO_RETRIES) {
                std::fs::File::open(p)
            } else {
                Err(io::Error::new(io::ErrorKind::TimedOut, "timed out"))
            }
        };

        let file = with_retry(&mut open_fn, &path);

        if cfg!(target_os = "macos") {
            let mut file = file?;
            assert_eq!(retries, *MAX_IO_RETRIES);
            let mut buf = String::new();
            file.read_to_string(&mut buf)?;
            assert_eq!(buf, "test");
            assert_eq!(FILE_UTIL_RETRY_SUCCESS.value(), 1);
        } else {
            file.as_ref().err();
            assert_eq!(
                file.err().map(|e| e.kind()).unwrap(),
                io::ErrorKind::TimedOut
            );
            assert_eq!(retries, 1);
            assert_eq!(FILE_UTIL_RETRY_FAILURE.value(), 0);
        }

        // Test error case
        let dir = tempdir()?;
        let path = dir.path().join("does_not_exist");
        let res = read_to_string_if_exists(path)?;
        assert_eq!(res, Option::None);

        Ok(())
    }
}
