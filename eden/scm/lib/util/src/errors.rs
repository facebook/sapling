/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;

use lazystr::LazyStr;

/// IOResult is a replacement for std::io::Result that forces you to
/// contextualize IO errors using `IOContext`.
pub type IOResult<T> = Result<T, IOError>;

#[derive(Debug, thiserror::Error)]
#[error("{msg}: {source}")]
pub struct IOError {
    msg: String,
    source: std::io::Error,
}

impl IOError {
    pub fn from_path(err: std::io::Error, msg: impl AsRef<str>, path: impl AsRef<Path>) -> IOError {
        IOError {
            msg: format!("{}: '{}'", msg.as_ref(), path.as_ref().display()),
            source: err,
        }
    }

    pub fn to_io_err(&self) -> std::io::Error {
        std::io::Error::new(
            self.source.kind(),
            format!("{}: {}", self.msg, self.source.to_string()),
        )
    }

    pub fn kind(&self) -> std::io::ErrorKind {
        self.source.kind()
    }
}

pub trait IOContext<T> {
    fn io_context(self, msg: impl LazyStr) -> Result<T, IOError>;

    fn path_context(self, msg: impl LazyStr, path: impl AsRef<Path>) -> Result<T, IOError>
    where
        Self: Sized,
    {
        self.io_context(|| format!("{}: '{}'", msg.to_str(), path.as_ref().display()))
    }
}

impl<T> IOContext<T> for std::io::Result<T> {
    fn io_context(self, msg: impl LazyStr) -> Result<T, IOError> {
        self.map_err(|err| IOError {
            msg: msg.to_str().to_string(),
            source: err,
        })
    }
}

impl<T> IOContext<T> for IOResult<T> {
    fn io_context(self, msg: impl LazyStr) -> Result<T, IOError> {
        self.map_err(|err| IOError {
            msg: format!("{}: {}", msg.to_str(), err.msg),
            source: err.source,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_context() {
        let res: std::io::Result<()> = Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists));
        let path: &Path = "/tmp/foo".as_ref();

        let res: IOResult<()> = res.path_context("error flimflamming file", path);

        // Can wrap further with more context.
        let res = res.io_context(|| "flibbertigibbet".to_string());

        let err = res.unwrap_err();
        assert_eq!(
            format!("{}", err),
            "flibbertigibbet: error flimflamming file: '/tmp/foo': entity already exists"
        );
    }
}
