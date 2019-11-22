/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use std::error::Error as StdError;
use std::fmt::{self, Debug, Display};

mod exttraits;
#[cfg(test)]
mod test;

pub use self::exttraits::*;

/// A wrapper around an error which is the consequence of another error, used to maintain
/// causal chains.
///
/// This is similar in many respects to `anyhow::Context`, except that the intent is to explicitly
/// maintain causal chains for consumption, rather than hiding them away behind a user-palatable
/// message.
///
/// Causal error chains can be extracted via the normal `Fail::iter_causes`/`iter_chain`. They
/// can also be displayed with the `{:#}` alternate formatting style - `{}` only shows the current
/// error.
#[derive(Debug)]
pub struct Chain<ERR> {
    err: ERR,
    cause: Option<Box<dyn StdError + Send + Sync>>,
}

impl<ERR> ChainExt<MarkerChainError, Error> for Chain<ERR>
where
    ERR: ToString + Debug + Send + Sync + 'static,
{
    type Chained = Chain<Error>;

    fn chain_err(self, err: Error) -> Self::Chained {
        Chain::with_fail(err, self)
    }
}

impl<ERR> Chain<ERR> {
    /// A new `Chain` error which has no cause. Useful for wrapping an instance of `Error`
    /// so that implements `std::error::Error`.
    pub fn new(err: ERR) -> Self {
        Chain { err, cause: None }
    }

    pub fn with_result<T, F>(err: ERR, cause: Result<T, F>) -> Result<T, Self>
    where
        F: StdError + Send + Sync + 'static,
    {
        cause.map_err(|cause| Self::with_fail(err, cause))
    }

    /// Chain a new error with an error which implements `std::error::Error` as its cause.
    pub fn with_fail<F>(err: ERR, cause: F) -> Self
    where
        F: StdError + Send + Sync + 'static,
    {
        Chain {
            err,
            cause: Some(Box::new(cause)),
        }
    }

    /// Chain a new error with `anyhow::Error` as its cause.
    pub fn with_error(err: ERR, cause: Error) -> Self {
        Chain {
            err,
            cause: Some(Box::from(cause)),
        }
    }

    pub fn as_err(&self) -> &ERR {
        &self.err
    }

    pub fn into_err(self) -> ERR {
        self.err
    }
}

impl<ERR> From<ERR> for Chain<ERR> {
    fn from(err: ERR) -> Self {
        Chain::new(err)
    }
}

impl<ERR> AsRef<ERR> for Chain<ERR> {
    fn as_ref(&self) -> &ERR {
        &self.err
    }
}

impl<ERR> Display for Chain<ERR>
where
    ERR: ToString + Debug + Send + Sync + 'static,
{
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "{}", self.err.to_string())?;
        if fmt.alternate() {
            let mut cause = self.source();
            while let Some(e) = cause {
                write!(fmt, "\n  caused by: {}", e)?;
                cause = e.source();
            }
        }
        Ok(())
    }
}

impl<ERR> StdError for Chain<ERR>
where
    ERR: ToString + Debug + Send + Sync + 'static,
{
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match &self.cause {
            Some(cause) => Some(&**cause),
            None => None,
        }
    }

    #[cfg(fbcode)]
    fn backtrace(&self) -> Option<&std::backtrace::Backtrace> {
        self.cause
            .as_ref()
            .map(Box::as_ref)
            .and_then(StdError::backtrace)
    }
}
