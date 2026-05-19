/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt;
use std::io;
use std::path::Path;
use std::path::PathBuf;

#[cfg(target_os = "windows")]
use winapi::shared::winerror::ERROR_CANT_RESOLVE_FILENAME;

pub(crate) const CREATE_FILE: &str = "failed to create file";
pub(crate) const CREATE_DIR: &str = "failed to create directory";
pub(crate) const OPEN_FILE: &str = "failed to open file";
pub(crate) const READ_LINK: &str = "failed to read symbolic link";
pub(crate) const REMOVE_DIR: &str = "failed to remove directory";
pub(crate) const REMOVE_FILE: &str = "failed to remove file";
pub(crate) const SET_PERMISSIONS: &str = "failed to set permissions for file";
pub(crate) const SYMLINK_METADATA: &str = "failed to query metadata of symlink";
pub(crate) const WRITE_FILE: &str = "failed to write to file";

/// `fs_err` has an internal path-aware error type, but it does not expose a
/// public constructor for fd-relative operations. Keep this small wrapper local
/// so syscall errors from `*at` APIs can still mention the user path.
pub(crate) fn build(
    source: io::Error,
    operation: &'static str,
    path: impl Into<PathBuf>,
) -> io::Error {
    let kind = source.kind();
    io::Error::new(
        kind,
        Error {
            operation,
            source,
            path: path.into(),
        },
    )
}

pub(crate) fn build_symlink(
    source: io::Error,
    from_path: impl Into<PathBuf>,
    to_path: impl Into<PathBuf>,
) -> io::Error {
    let kind = source.kind();
    io::Error::new(
        kind,
        SourceDestError {
            source,
            from_path: from_path.into(),
            to_path: to_path.into(),
        },
    )
}

/// Details extracted from this crate's path-aware error wrapper.
#[derive(Clone, Copy, Debug)]
pub struct PathErrorDetails<'a> {
    /// The original I/O error before adding path context.
    pub original_io_error: &'a io::Error,
    /// The path associated with the failed operation.
    ///
    /// For symlink errors, this is the link path rather than the target path.
    pub path: &'a Path,
}

/// Return details wrapped by this crate's path-aware error.
///
/// Ideally, this can also strip path from `fs_err` errors. However, `fs_err`
/// doesn't expose its errors as public types to downcast and inspect. So this
/// function would return `None` for `fs_err` and plain [`io::Error`].
pub fn path_error_details(err: &io::Error) -> Option<PathErrorDetails<'_>> {
    let inner = err.get_ref()?;

    if let Some(err) = inner.downcast_ref::<Error>() {
        return Some(PathErrorDetails {
            original_io_error: &err.source,
            path: &err.path,
        });
    }

    if let Some(err) = inner.downcast_ref::<SourceDestError>() {
        return Some(PathErrorDetails {
            original_io_error: &err.source,
            path: &err.to_path,
        });
    }

    None
}

/// Return true if `err` means path resolution was blocked by a symlink or reparse point.
pub fn is_symlink_traversal_error(err: &io::Error) -> bool {
    let err = path_error_details(err).map_or(err, |details| details.original_io_error);
    is_symlink_traversal_raw_error(err)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn is_symlink_traversal_raw_error(err: &io::Error) -> bool {
    err.raw_os_error() == Some(libc::ELOOP)
}

#[cfg(target_os = "windows")]
fn is_symlink_traversal_raw_error(err: &io::Error) -> bool {
    err.raw_os_error() == Some(ERROR_CANT_RESOLVE_FILENAME as i32)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn is_symlink_traversal_raw_error(_err: &io::Error) -> bool {
    false
}

#[derive(Debug)]
struct Error {
    operation: &'static str,
    source: io::Error,
    path: PathBuf,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.operation.is_empty() {
            write!(f, "{}: {}", self.path.display(), &self.source)
        } else {
            write!(
                f,
                "{} `{}`: {}",
                self.operation,
                self.path.display(),
                &self.source
            )
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug)]
struct SourceDestError {
    source: io::Error,
    from_path: PathBuf,
    to_path: PathBuf,
}

impl fmt::Display for SourceDestError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "failed to symlink file from {} to {}: {}",
            self.from_path.display(),
            self.to_path.display(),
            &self.source
        )
    }
}

impl std::error::Error for SourceDestError {}

#[cfg(test)]
mod tests {
    use std::io;

    #[test]
    fn display_inlines_source_error() {
        let err = super::build(
            io::Error::other("inner error"),
            super::WRITE_FILE,
            "dir/file_1",
        );

        let message = err.to_string();
        assert_eq!(message, "failed to write to file `dir/file_1`: inner error");

        let message = format!("{:#}", anyhow::Error::new(err));
        assert_eq!(message, "failed to write to file `dir/file_1`: inner error");
        assert_eq!(message.matches(':').count(), 1, "{message}");
    }

    #[test]
    fn source_dest_display_matches_fs_err_symlink() {
        let err = super::build_symlink(io::Error::other("inner error"), "target", "dir/link");

        let message = err.to_string();
        assert_eq!(
            message,
            "failed to symlink file from target to dir/link: inner error"
        );

        let message = format!("{:#}", anyhow::Error::new(err));
        assert_eq!(
            message,
            "failed to symlink file from target to dir/link: inner error"
        );
        assert_eq!(message.matches(':').count(), 1, "{message}");
    }

    #[test]
    fn path_error_details_returns_single_path_details() {
        let err = super::build(
            io::Error::from_raw_os_error(2),
            super::READ_LINK,
            "dir/link",
        );

        let details = super::path_error_details(&err).expect("path error should expose details");
        assert_eq!(details.original_io_error.raw_os_error(), Some(2));
        assert_eq!(details.path, std::path::Path::new("dir/link"));
    }

    #[test]
    fn path_error_details_returns_source_dest_details() {
        let err = super::build_symlink(io::Error::from_raw_os_error(17), "target", "dir/link");

        let details =
            super::path_error_details(&err).expect("source/dest error should expose details");
        assert_eq!(details.original_io_error.raw_os_error(), Some(17));
        assert_eq!(details.path, std::path::Path::new("dir/link"));
    }

    #[test]
    fn path_error_details_ignores_plain_io_error() {
        let err = io::Error::from_raw_os_error(2);

        assert!(super::path_error_details(&err).is_none());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn symlink_traversal_error_checks_unix_loop_error() {
        let plain = io::Error::from_raw_os_error(libc::ELOOP);
        assert!(super::is_symlink_traversal_error(&plain));

        let wrapped = super::build(plain, super::SYMLINK_METADATA, "link/file");
        assert!(super::is_symlink_traversal_error(&wrapped));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn symlink_traversal_error_checks_windows_loop_error() {
        let plain = io::Error::from_raw_os_error(super::ERROR_CANT_RESOLVE_FILENAME as i32);
        assert!(super::is_symlink_traversal_error(&plain));

        let wrapped = super::build(plain, super::SYMLINK_METADATA, "link/file");
        assert!(super::is_symlink_traversal_error(&wrapped));
    }
}
