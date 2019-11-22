/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![cfg_attr(fbcode, feature(backtrace))]
#![deny(warnings)]

use std::error::Error as StdError;
use std::fmt::{self, Debug, Display};

pub use failure;
pub use failure_derive;

mod slogkv;
pub use crate::slogkv::{cause_workaround as cause, SlogKVError, SlogKVErrorKey};

mod convert;
pub use self::convert::convert;

pub mod chain;

pub mod prelude {
    pub use crate::chain::{self, Chain, ChainExt};
    pub use crate::{
        FutureFailureErrorExt, FutureFailureExt, StreamFailureErrorExt, StreamFailureExt,
    };
    pub use anyhow::{Context, Error, Result};
}

pub use anyhow::{bail, format_err, Error, Result};

// Anyhow's macros work with both fmt messages and error values the same. We
// temporarily re-export under our old failure macro names to ease migration,
// but these will be removed in favor of plain bail and ensure.
#[doc(hidden)]
pub use anyhow::{bail as bail_err, bail as bail_msg};
pub use anyhow::{ensure as ensure_err, ensure as ensure_msg};

// Temporary immitation of failure's API to ease migration.
#[doc(hidden)]
pub use anyhow::Context as ResultExt;

// Temporary immitation of failure's API to ease migration.
#[doc(hidden)]
pub fn err_msg<D>(msg: D) -> Error
where
    D: Display + Debug + Send + Sync + 'static,
{
    Error::msg(msg)
}

// Deprecated.
#[doc(hidden)]
pub use failure::Fail;

#[macro_use]
mod macros;
mod context_futures;
mod context_streams;
pub use crate::context_futures::{FutureFailureErrorExt, FutureFailureExt};
pub use crate::context_streams::{StreamFailureErrorExt, StreamFailureExt};

pub struct DisplayChain<'a>(&'a Error);

impl<'a> From<&'a Error> for DisplayChain<'a> {
    fn from(e: &'a Error) -> Self {
        DisplayChain(e)
    }
}

impl Display for DisplayChain<'_> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let e = self.0;
        writeln!(fmt, "Error: {}", e)?;
        for c in e.chain().skip(1) {
            writeln!(fmt, "Caused by: {}", c)?;
        }
        Ok(())
    }
}

// Temporary immitation of failure::Compat<T> to ease migration.
pub struct Compat<T>(pub T);

impl StdError for Compat<Error> {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.0.source()
    }
    #[cfg(fbcode)]
    fn backtrace(&self) -> Option<&std::backtrace::Backtrace> {
        Some(self.0.backtrace())
    }
}

impl Display for Compat<Error> {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.0, formatter)
    }
}

impl Debug for Compat<Error> {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        Debug::fmt(&self.0, formatter)
    }
}
