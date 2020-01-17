/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Errors used by the crate
//!
//! See [`Error`] for the main type.

use std::fmt;
use std::io;
use std::path::Path;

// Error design goals:
// - Callsites can test whether an error is caused by data corruption or other
//   issues (for example, permission or resource issues).
//   This is important because it allows callsites (including within the
//   crate like RotateLog) to decide whether to remove the bad data and
//   try auto recovery.
// - The library can change error structure internals. That means accesses
//   to the error object are via public methods instead of struct or enum
//   fields. `Error` is the only opaque public error type.
// - Compatible with std Error. Therefore anyhow::Error is supported too.

/// Represents all possible errors that can occur when using indexedlog.
pub struct Error {
    // Boxing makes `Result<Error, _>` smaller.
    inner: Box<Inner>,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Default)]
struct Inner {
    sources: Vec<Box<dyn std::error::Error + Send + Sync + 'static>>,
    messages: Vec<String>,
    is_corruption: bool,
    io_error_kind: Option<io::ErrorKind>,
}

impl Error {
    /// Return `true` if the error is considered as (filesystem) data
    /// corruption.
    ///
    /// Application can use this information to decide whether to try the
    /// expensive `repair` API.
    ///
    /// Data corruption is in general when the data can be confidently read,
    /// but does not meet integrity expectation. Note: a "file not found" error
    /// is a piece of information that is "confidently read", because it's
    /// unexpected to get a different error by using different users, or
    /// process settings. So "file not found" is not always a corruption.
    ///
    /// For example, those are data corruption errors:
    /// - XxHash checksum does not match the data
    /// - Some data says "read this file in 100..200 byte range", but logically,
    ///   the file only has 150 bytes.
    /// - A byte is expected to only be 1 or 2. But it turns out to be 0.
    /// - Both file "a" and "b" are expected to exist in a directory, but "a"
    ///   can be found, while "b" does not exist.
    ///
    /// Those are not data corruption:
    /// - Cannot open a file due to permission issues or exceeding the file
    ///   descriptor limit.
    /// - Programming errors and API misuse. For example, a user-provided
    ///   index function says `data[5..10] is the index key` while `data`
    /// - Both file "a" and "b" are expected to exist in a directory, but "a"
    ///   can be found, while "b" cannot be opened due to permission issues.
    ///
    /// It's expected that data corruption can *only* happen when the managed
    /// files or directories are changed without using this crate.  For example,
    /// a hard reboot, deleting lock files while multiple processes are
    /// attempting to write, deleting or changing files in some ways. Issues
    /// like "disk is full", "permission errors", "process killed at random
    /// time" are expected to not cause data corruption (but only data loss).
    pub fn is_corruption(&self) -> bool {
        self.inner.is_corruption
    }

    pub(crate) fn io_error_kind(&self) -> io::ErrorKind {
        self.inner.io_error_kind.unwrap_or(io::ErrorKind::Other)
    }

    // Following methods are used by this crate only.
    // External code should not construct or modify `Error`.

    pub(crate) fn message(mut self, message: impl ToString) -> Self {
        self.inner.messages.push(message.to_string());
        self
    }

    pub(crate) fn source(self, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.source_dyn(Box::new(source))
    }

    fn source_dyn(mut self, source: Box<dyn std::error::Error + Send + Sync + 'static>) -> Self {
        // Inherit the data corruption flag.
        if let Some(err) = source.downcast_ref::<Error>() {
            if err.is_corruption() {
                self = self.mark_corruption();
            }
        }

        self.inner.sources.push(source);
        self
    }

    pub(crate) fn mark_corruption(mut self) -> Self {
        self.inner.is_corruption = true;
        self
    }

    pub(crate) fn blank() -> Self {
        Error {
            inner: Default::default(),
        }
    }

    /// A ProgrammingError that breaks some internal assumptions.
    /// For example, passing an invalid parameter to an API.
    #[inline(never)]
    pub(crate) fn programming(message: impl ToString) -> Self {
        Self::blank().message(format!("ProgrammingError: {}", message.to_string()))
    }

    /// A data corruption error with path.
    ///
    /// If there is an [`IOError`], use [`IoResultExt::context`] instead.
    #[inline(never)]
    pub(crate) fn corruption(path: &Path, message: impl ToString) -> Self {
        let message = format!("{:?}: {}", path, message.to_string());
        Self::blank().mark_corruption().message(message)
    }

    /// An error with a path that is not a data corruption.
    ///
    /// If there is an [`IOError`], use [`IoResultExt::context`] instead.
    #[inline(never)]
    pub(crate) fn path(path: &Path, message: impl ToString) -> Self {
        let message = format!("{:?}: {}", path, message.to_string());
        Self::blank().message(message)
    }

    /// Wrap a dynamic stdlib error.
    #[inline(never)]
    pub(crate) fn wrap(
        err: Box<dyn std::error::Error + Send + Sync + 'static>,
        message: impl LazyToString,
    ) -> Self {
        Self::blank()
            .message(message.to_string_costly())
            .source_dyn(err)
    }
}

/// Create an Error from a message.
impl From<&str> for Error {
    fn from(s: &str) -> Self {
        Self::blank().message(s)
    }
}

/// Create an Error from a message and another error.
impl<E: std::error::Error + Send + Sync + 'static> From<(&str, E)> for Error {
    fn from(s: (&str, E)) -> Self {
        Self::blank().message(s.0).source(s.1)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut lines = Vec::new();
        for message in &self.inner.messages {
            lines.push(message.to_string());
        }
        if !self.inner.sources.is_empty() {
            lines.push(format!("Caused by {} errors:", self.inner.sources.len()));
            for source in &self.inner.sources {
                lines.push(indent(format!("{}", source), 2, '-'));
            }
        }
        write!(f, "{}", lines.join("\n"))
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut lines = Vec::new();
        for message in &self.inner.messages {
            lines.push(message.to_string());
        }
        if self.is_corruption() {
            lines.push("(This error is considered as a data corruption)".to_string())
        }
        if !self.inner.sources.is_empty() {
            lines.push(format!("Caused by {} errors:", self.inner.sources.len()));
            for source in &self.inner.sources {
                lines.push(indent(format!("{:?}", source), 2, '-'));
            }
        }
        write!(f, "{}", lines.join("\n"))
    }
}

fn indent(s: String, spaces: usize, first_line_prefix: char) -> String {
    if spaces == 0 {
        s
    } else {
        format!(
            "{}{}{}",
            first_line_prefix,
            " ".repeat(spaces - 1),
            s.lines()
                .collect::<Vec<_>>()
                .join(&format!("\n{}", " ".repeat(spaces)))
        )
    }
}

pub(crate) trait ResultExt<T> {
    /// Mark the error as data corruption.
    fn corruption(self) -> Self;

    /// Add a string message as context.
    fn context<S: LazyToString>(self, message: S) -> Self;

    /// Add an error source.
    fn source<E: std::error::Error + Send + Sync + 'static>(self, source: E) -> Self;
}

impl<T> ResultExt<T> for Result<T> {
    fn corruption(self) -> Self {
        self.map_err(|err| err.mark_corruption())
    }

    fn context<S: LazyToString>(self, message: S) -> Self {
        self.map_err(|err| err.message(message.to_string_costly()))
    }

    fn source<E: std::error::Error + Send + Sync + 'static>(self, source: E) -> Self {
        self.map_err(|err| err.source(source))
    }
}

impl std::error::Error for Error {
    // This 'Error' type is designed to be opaque (internal states are
    // private, including inner errors), and takes responsibility
    // of displaying a -chain- tree of errors. So it might be desirable
    // not implementing `source` here, and expose public APIs for all
    // use-needs.
}

pub(crate) trait IoResultExt<T> {
    /// Wrap [`io::Result`] in [`Result`] with extra context about filesystem
    /// path and the operation name.
    ///
    /// Mark InvalidData and UnexpectedEof as data corruption automatically.
    ///
    /// Consider using [`ResultExt::corruption`] to mark the error as data
    /// corruption if appropriate.
    fn context<TS: LazyToString>(self, path: &Path, message: TS) -> Result<T>;

    /// Wrap an infallible Result. For example, writing to memory.
    fn infallible(self) -> Result<T>;
}

impl<T> IoResultExt<T> for std::io::Result<T> {
    fn context<TS: LazyToString>(self, path: &Path, message: TS) -> Result<T> {
        self.map_err(|err| {
            use std::io::ErrorKind;
            let kind = err.kind().clone();
            let corruption = match kind {
                // For example, try to mmap 200 bytes, but the file
                // only has 100 bytes. This is unlikely caused by
                // non-data-corruption issues.
                ErrorKind::UnexpectedEof | ErrorKind::InvalidData => true,
                _ => false,
            };
            let is_eperm = kind == ErrorKind::PermissionDenied;

            let mut err = Error::blank().source(err).message(format!(
                "{:?}: {}",
                path,
                message.to_string_costly()
            ));
            if corruption {
                err = err.mark_corruption();
            }
            err.inner.io_error_kind = Some(kind);

            // Provide more context for PermissionDenied
            if is_eperm {
                #[cfg(unix)]
                #[allow(clippy::redundant_closure_call)]
                {
                    let add_stat_context = |path: &Path, err: Error| {
                        use std::os::unix::fs::MetadataExt;
                        if let Ok(meta) = path.metadata() {
                            let msg = format!(
                                "stat({:?}) = dev:{} ino:{} mode:0o{:o} uid:{} gid:{} mtime:{}",
                                path,
                                meta.dev(),
                                meta.ino(),
                                meta.mode(),
                                meta.uid(),
                                meta.gid(),
                                meta.mtime()
                            );
                            err.message(msg)
                        } else {
                            err
                        }
                    };
                    err = add_stat_context(path, err);
                    // For EPERM on mkdir, parent directory stat is useful.
                    if let Some(parent) = path.parent() {
                        err = add_stat_context(&parent, err);
                    }
                }
            }

            err
        })
    }

    fn infallible(self) -> Result<T> {
        self.map_err(|err| Error::blank().source(err).message("Unexpected failure"))
    }
}

pub(crate) trait LazyToString {
    fn to_string_costly(&self) -> String;
}

// &'static str is cheap.
impl LazyToString for &'static str {
    fn to_string_costly(&self) -> String {
        (*self).to_string()
    }
}

impl<F: Fn() -> String> LazyToString for F {
    fn to_string_costly(&self) -> String {
        self()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_format() {
        let mut e = Error::blank();

        assert_eq!(format!("{}", &e), "");

        // Attach messages.

        e = e.message("Error Message 1");
        e = e.message("Error Message 2");
        assert_eq!(
            format!("{}", &e),
            r#"Error Message 1
Error Message 2"#
        );

        // Attach error sources.
        e = e.source(Error::blank().message("Inner Error 1"));
        e = e.source(
            Error::blank()
                .message("Inner Error 2")
                .source(Error::blank().message("Nested Error 1")),
        );
        assert_eq!(
            format!("{}", &e),
            r#"Error Message 1
Error Message 2
Caused by 2 errors:
- Inner Error 1
- Inner Error 2
  Caused by 1 errors:
  - Nested Error 1"#
        );

        // Mark as data corruption.
        e = e.mark_corruption();
        assert_eq!(
            format!("{:?}", &e),
            r#"Error Message 1
Error Message 2
(This error is considered as a data corruption)
Caused by 2 errors:
- Inner Error 1
- Inner Error 2
  Caused by 1 errors:
  - Nested Error 1"#
        );
    }

    #[test]
    fn test_result_ext() {
        let result: Result<()> = Err(Error::blank()).corruption();
        assert!(result.unwrap_err().is_corruption());
    }

    #[test]
    fn test_inherit_corruption() {
        assert!(!Error::blank().is_corruption());
        assert!(!Error::blank().source(Error::blank()).is_corruption());
        assert!(Error::blank()
            .source(Error::blank().mark_corruption())
            .is_corruption());
        assert!(Error::blank()
            .source(Error::blank().source(Error::blank().mark_corruption()))
            .is_corruption());
    }

    #[test]
    fn test_io_result_ext() {
        let err = io_result().context(Path::new("a.txt"), "cannot open for reading");
        assert_eq!(
            format!("{}", err.unwrap_err()),
            r#""a.txt": cannot open for reading
Caused by 1 errors:
- io::Error: something wrong happened"#
        );

        let name = "b.txt";
        let err = io_result().context(Path::new(&name), || format!("cannot open {}", &name));
        assert_eq!(
            format!("{}", err.unwrap_err()),
            r#""b.txt": cannot open b.txt
Caused by 1 errors:
- io::Error: something wrong happened"#
        );
    }

    fn io_result() -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "io::Error: something wrong happened",
        ))
    }
}
