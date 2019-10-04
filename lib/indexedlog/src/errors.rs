// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Errors used by the crate
//!
//! See [`Error`] for the main type.

use std::fmt;

// Error design goals:
// - Callsites can test whether an error is caused by data corruption or other
//   issues (for example, permission or resource issues).
//   This is important because it allows callsites (including within the
//   crate like RotateLog) to decide whether to remove the bad data and
//   try auto recovery.
// - The library can change error structure internals. That means accesses
//   to the error object are via public methods instead of struct or enum
//   fields. `Error` is the only opaque public error type.
// - Compatible with std Error. Therefore failure::Error is supported too.

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
}

impl Error {
    pub fn is_corruption(&self) -> bool {
        self.inner.is_corruption
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

pub(crate) trait LazyToString {
    fn to_string_costly(&self) -> String;
}

// &'static str is cheap.
impl LazyToString for &'static str {
    fn to_string_costly(&self) -> String {
        self.to_string()
    }
}

impl<F: Fn() -> String> LazyToString for F {
    fn to_string_costly(&self) -> String {
        self()
    }
}

// XXX: Follwing are legacy types and functions that should be remvoed.

use failure::Fail;
use std::borrow::Cow;
use std::path::Path;

#[derive(Fail, Debug)]
#[fail(display = "{:?}: range {}..{} failed checksum check", path, start, end)]
pub struct ChecksumError {
    pub path: String,
    pub start: u64,
    pub end: u64,
}

#[derive(Fail, Debug)]
#[fail(display = "{}: {}", _0, _1)]
pub struct PathDataError(pub String, pub Cow<'static, str>);

#[derive(Fail, Debug)]
#[fail(display = "ProgrammingError: {}", _0)]
pub struct ProgrammingError(pub Cow<'static, str>);

#[derive(Fail, Debug)]
#[fail(display = "DataError: {}", _0)]
pub struct DataError(pub Cow<'static, str>);

#[derive(Fail, Debug)]
#[fail(display = "ParameterError: {}", _0)]
pub struct ParameterError(pub Cow<'static, str>);

#[inline(never)]
pub(crate) fn parameter_error(msg: impl Into<Cow<'static, str>>) -> failure::Error {
    ParameterError(msg.into()).into()
}

#[inline(never)]
pub(crate) fn data_error(msg: impl Into<Cow<'static, str>>) -> failure::Error {
    DataError(msg.into()).into()
}

#[inline(never)]
pub(crate) fn path_data_error(
    path: impl AsRef<Path>,
    msg: impl Into<Cow<'static, str>>,
) -> failure::Error {
    PathDataError(path.as_ref().to_string_lossy().to_string(), msg.into()).into()
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
}
