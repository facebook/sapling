// Copyright 2004-present Facebook. All Rights Reserved.

// Missing bits from failure git
use std::fmt;

extern crate failure;
#[macro_use]
extern crate failure_derive;
extern crate futures;
extern crate slog;

pub mod prelude {
    pub use failure::{Error, Fail, ResultExt};

    pub use super::{FutureFailureErrorExt, FutureFailureExt, Result, StreamFailureErrorExt,
                    StreamFailureExt};
}

pub use failure::{_core, err_msg, Backtrace, Causes, Compat, Context, Error, Fail, ResultExt,
                  SyncFailure};
pub use failure_derive::*;

#[macro_use]
mod macros;
mod context_futures;
mod context_streams;
pub use context_futures::{FutureFailureErrorExt, FutureFailureExt};
pub use context_streams::{StreamFailureErrorExt, StreamFailureExt};

pub type Result<T> = ::std::result::Result<T, Error>;

pub struct DisplayChain<'a>(&'a Error);

impl<'a> From<&'a Error> for DisplayChain<'a> {
    fn from(e: &'a Error) -> Self {
        DisplayChain(e)
    }
}

impl<'a> fmt::Display for DisplayChain<'a> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let e = self.0;
        writeln!(fmt, "Error: {}", e)?;
        for c in e.iter_chain().skip(1) {
            writeln!(fmt, "Caused by: {}", c)?;
        }
        Ok(())
    }
}

pub struct SlogKVError(pub Error);

impl slog::KV for SlogKVError {
    fn serialize(&self, _record: &slog::Record, serializer: &mut slog::Serializer) -> slog::Result {
        let err = &self.0;

        serializer.emit_str("error", &format!("{}", err))?;
        serializer.emit_str("root_cause", &format!("{:#?}", err.find_root_cause()))?;
        serializer.emit_str("backtrace", &format!("{:#?}", err.backtrace()))?;

        for c in err.iter_chain().skip(1) {
            serializer.emit_str("cause", &format!("{}", c))?;
        }

        Ok(())
    }
}

// Dummy use of derive Fail to avoid warning on #[macro_use] for failure_derive
#[derive(Debug, Fail)]
#[fail(display = "")]
struct _Dummy;
