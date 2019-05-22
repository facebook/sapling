// Copyright 2004-present Facebook. All Rights Reserved.

use std::fmt::{self, Debug, Display};

use super::{Backtrace, Error, Fail};

mod exttraits;
#[cfg(test)]
mod test;

pub use self::exttraits::*;

/// A wrapper around an error which is the consequence of another error, used to maintain
/// causal chains.
///
/// This is similar in many respects to `failure::Context`, except that the intent is to explicitly
/// maintain causal chains for consumption, rather than hiding them away behind a user-palatable
/// message.
///
/// Causal error chains can be extracted via the normal `Fail::iter_causes`/`iter_chain`. They
/// can also be displayed with the `{:#}` alternate formatting style - `{}` only shows the current
/// error.
#[derive(Debug)]
pub struct Chain<ERR> {
    err: ERR,
    cause: CauseKind,
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
    /// so that implements `Fail`.
    pub fn new(err: ERR) -> Self {
        Chain {
            err,
            cause: CauseKind::None,
        }
    }

    pub fn with_result<T, F>(err: ERR, cause: Result<T, F>) -> Result<T, Self>
    where
        F: Fail,
    {
        cause.map_err(|cause| Self::with_fail(err, cause))
    }

    /// Chain a new error with an error which implements `Fail` as its cause.
    pub fn with_fail<F>(err: ERR, cause: F) -> Self
    where
        F: Fail,
    {
        Chain {
            err,
            cause: CauseKind::Fail(Box::new(cause)),
        }
    }

    /// Chain a new error with `failure::Error` as its cause.
    pub fn with_error(err: ERR, cause: Error) -> Self {
        Chain {
            err,
            cause: CauseKind::Error(cause),
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

#[derive(Debug)]
enum CauseKind {
    None,
    Fail(Box<dyn Fail>),
    Error(Error),
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
            for c in Fail::iter_causes(self) {
                write!(fmt, "\n  caused by: {}", c)?;
            }
        }
        Ok(())
    }
}

impl<ERR> Fail for Chain<ERR>
where
    ERR: ToString + Debug + Send + Sync + 'static,
{
    fn cause(&self) -> Option<&dyn Fail> {
        match &self.cause {
            CauseKind::None => None,
            CauseKind::Fail(f) => Some(&*f),
            CauseKind::Error(e) => Some(e.as_fail()),
        }
    }

    fn backtrace(&self) -> Option<&Backtrace> {
        match &self.cause {
            CauseKind::None => None,
            CauseKind::Fail(f) => f.backtrace(),
            CauseKind::Error(e) => Some(e.backtrace()),
        }
    }
}
