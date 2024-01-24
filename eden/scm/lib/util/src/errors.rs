/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;
use std::path::Path;

use lazystr::LazyStr;

#[derive(Debug, thiserror::Error)]
#[error("{msg}: {source}")]
pub(crate) struct IOErrorContext {
    msg: String,
    source: std::io::Error,
}

pub fn from_err_msg(source: io::Error, msg: String) -> io::Error {
    let kind = source.kind();
    let error = IOErrorContext { msg, source };
    io::Error::new(kind, error)
}

pub fn from_err_msg_path(
    err: io::Error,
    msg: impl AsRef<str>,
    path: impl AsRef<Path>,
) -> io::Error {
    let msg = format!("{}: '{}'", msg.as_ref(), path.as_ref().display());
    from_err_msg(err, msg)
}

pub trait IOContext<T> {
    fn io_context(self, msg: impl LazyStr) -> io::Result<T>;

    fn path_context(self, msg: impl LazyStr, path: impl AsRef<Path>) -> io::Result<T>
    where
        Self: Sized,
    {
        self.io_context(|| format!("{}: '{}'", msg.to_str(), path.as_ref().display()))
    }
}

impl<T> IOContext<T> for std::io::Result<T> {
    fn io_context(self, msg: impl LazyStr) -> io::Result<T> {
        self.map_err(|err| from_err_msg(err, msg.to_str().to_string()))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_context() {
        let res: std::io::Result<()> = Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists));
        let path: &Path = "/tmp/foo".as_ref();

        let res: io::Result<()> = res.path_context("error flimflamming file", path);

        // Can wrap further with more context.
        let res = res.io_context(|| "flibbertigibbet".to_string());

        let err = res.unwrap_err();
        assert_eq!(
            format!("{}", err),
            "flibbertigibbet: error flimflamming file: '/tmp/foo': entity already exists"
        );
    }
}
