// Copyright 2004-present Facebook. All Rights Reserved.

#![deny(warnings)]

// Missing bits from failure git
use std::fmt;

pub use failure;
pub use failure_derive;

mod slogkv;
pub use crate::slogkv::{SlogKVError, SlogKVErrorKey};

pub mod chain;

pub mod prelude {
    pub use crate::chain::{self, Chain, ChainExt};
    pub use failure::{self, Error, Fail, ResultExt};
    pub use failure_derive::*;

    pub use super::{
        AsFail, FutureFailureErrorExt, FutureFailureExt, Result, StreamFailureErrorExt,
        StreamFailureExt,
    };
}

pub use failure::{
    _core, bail, err_msg, AsFail, Backtrace, Causes, Compat, Context, Error, Fail, ResultExt, SyncFailure,
};
pub use failure_derive::*;

#[macro_use]
mod macros;
mod context_futures;
mod context_streams;
pub use crate::context_futures::{FutureFailureErrorExt, FutureFailureExt};
pub use crate::context_streams::{StreamFailureErrorExt, StreamFailureExt};

pub type Result<T> = ::std::result::Result<T, Error>;

pub struct DisplayChain<'a>(&'a Error);

impl<'a> From<&'a Error> for DisplayChain<'a> {
    fn from(e: &'a Error) -> Self {
        DisplayChain(e)
    }
}

impl<'a> fmt::Display for DisplayChain<'a> {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        let e = self.0;
        writeln!(fmt, "Error: {}", e)?;
        for c in e.iter_chain().skip(1) {
            writeln!(fmt, "Caused by: {}", c)?;
        }
        Ok(())
    }
}

// Dummy use of derive Fail to avoid warning on #[macro_use] for failure_derive
#[derive(Debug, Fail)]
#[fail(display = "")]
struct _Dummy;
