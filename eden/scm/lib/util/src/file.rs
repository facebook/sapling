/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io;
use std::path::Path;

use ::metrics::Counter;
use fs::File;
use fs::OpenOptions;
use fs_err as fs;
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

pub(crate) static UMASK: Lazy<u32> = Lazy::new(|| unsafe {
    #[cfg(unix)]
    {
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

pub fn get_umask() -> u32 {
    *UMASK
}

pub fn apply_umask(mode: u32) -> u32 {
    mode & !get_umask()
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

    with_retry(&mut open_fn, path)
}

pub fn create(path: impl AsRef<Path>) -> io::Result<File> {
    open(path, "wct")
}

fn is_retryable(err: &io::Error) -> bool {
    cfg!(target_os = "macos")
        && (err.kind() == io::ErrorKind::TimedOut
            || err.kind() == io::ErrorKind::StaleNetworkFileHandle)
}

fn with_retry<'a, F, T>(io_operation: &mut F, path: &'a Path) -> io::Result<T>
where
    F: FnMut(&'a Path) -> io::Result<T>,
{
    let mut attempts: u32 = 0;
    loop {
        match io_operation(path) {
            Ok(v) => {
                if attempts > 0 {
                    FILE_UTIL_RETRY_SUCCESS.increment();
                }
                return Ok(v);
            }
            Err(err) if is_retryable(&err) => {
                if attempts >= *MAX_IO_RETRIES {
                    if attempts > 0 {
                        FILE_UTIL_RETRY_FAILURE.increment();
                    }
                    return Err(err);
                }
                attempts += 1;
            }
            Err(err) => return Err(err),
        }
    }
}

pub fn exists(path: impl AsRef<Path>) -> io::Result<Option<std::fs::Metadata>> {
    let path = path.as_ref();
    match with_retry(&mut fs::metadata, path) {
        Ok(m) => Ok(Some(m)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

pub fn unlink_if_exists(path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref();
    match with_retry(&mut fs::remove_file, path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

pub fn read_to_string_if_exists(path: impl AsRef<Path>) -> io::Result<Option<String>> {
    let path = path.as_ref();
    match with_retry(&mut fs::read_to_string, path) {
        Ok(contents) => Ok(Some(contents)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;

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

    static TEST_FILE_CONTENT: &str = "test";

    fn check_io_with_retry(
        path: &Path,
        error_kind: std::io::ErrorKind,
        expected_attempts: u32,
        should_succeed: bool,
    ) {
        let mut attempts: u32 = 0;
        let mut open_fn = |p: &Path| -> io::Result<File> {
            attempts += 1;
            if attempts >= *MAX_IO_RETRIES {
                fs::File::open(p)
            } else {
                Err(io::Error::new(error_kind, error_kind.to_string()))
            }
        };
        let io_result = with_retry(&mut open_fn, path);

        if should_succeed {
            let mut file = io_result.unwrap();
            assert_eq!(attempts, expected_attempts);
            let mut buf = String::new();
            io::Read::read_to_string(&mut file, &mut buf).unwrap();
            assert_eq!(buf, TEST_FILE_CONTENT);
        } else {
            assert_eq!(io_result.err().map(|e| e.kind()).unwrap(), error_kind);
            assert_eq!(attempts, expected_attempts);
        }
    }

    fn get_test_path(name: &str, tempdir: &tempfile::TempDir) -> std::path::PathBuf {
        let path = tempdir.path().join(name);
        std::fs::write(&path, TEST_FILE_CONTENT).unwrap();
        path
    }

    #[test]
    fn test_retry_io() -> Result<()> {
        use std::io::ErrorKind;

        let tempdir = tempfile::tempdir().unwrap();
        let mut test_cases = HashMap::new();
        // The behavior of these test cases varies by platform
        let should_succeed = cfg!(target_os = "macos");
        let expected_attempts = if should_succeed { *MAX_IO_RETRIES } else { 1 };
        test_cases.insert(
            get_test_path("test_timeout", &tempdir),
            (ErrorKind::TimedOut, expected_attempts, should_succeed),
        );
        test_cases.insert(
            get_test_path("test_stale", &tempdir),
            (
                ErrorKind::StaleNetworkFileHandle,
                expected_attempts,
                should_succeed,
            ),
        );

        // These test cases should behave the same on all platforms
        test_cases.insert(
            get_test_path("test_too_many_args", &tempdir),
            (ErrorKind::ArgumentListTooLong, 1, false),
        );
        test_cases.insert(
            get_test_path("test_permission_denied", &tempdir),
            (ErrorKind::PermissionDenied, 1, false),
        );

        for (test_path, results) in test_cases {
            check_io_with_retry(&test_path, results.0, results.1, results.2);
        }

        // .*_if_exists() functions should still return None if the file doesn't exist
        let path = tempdir.path().join("does_not_exist");
        let res = read_to_string_if_exists(path)?;
        assert_eq!(res, Option::None);

        Ok(())
    }
}
