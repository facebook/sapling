/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use super::{Compat, Error};
use futures::future::SharedError;
use std::error::Error as StdError;
use std::ops::Deref;

pub struct SlogKVError(pub Error);

impl slog::KV for SlogKVError {
    fn serialize(
        &self,
        _record: &slog::Record<'_>,
        serializer: &mut dyn slog::Serializer,
    ) -> slog::Result {
        let err = &self.0;

        serializer.emit_str(Error.to_str(), &format!("{}", err))?;
        #[cfg(fbcode)]
        {
            let backtrace = err.backtrace();
            if let std::backtrace::BacktraceStatus::Captured = backtrace.status() {
                serializer.emit_str(Backtrace.to_str(), &backtrace.to_string())?;
            }
        }

        let mut err = err.deref() as &dyn StdError;
        while let Some(cause) = cause_workaround(err) {
            serializer.emit_str(Cause.to_str(), &format!("{}", cause))?;
            err = cause;
        }
        serializer.emit_str(RootCause.to_str(), &format!("{:#?}", err))?;

        Ok(())
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SlogKVErrorKey {
    Error,
    RootCause,
    Backtrace,
    Cause,
}
use crate::SlogKVErrorKey::*;

impl SlogKVErrorKey {
    pub fn to_str(self) -> &'static str {
        match self {
            Error => "error",
            RootCause => "root_cause",
            Backtrace => "backtrace",
            Cause => "cause",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "error" => Some(Error),
            "root_cause" => Some(RootCause),
            "backtrace" => Some(Backtrace),
            "cause" => Some(Cause),
            _ => None,
        }
    }
}

// Like Fail::cause, but handles SharedError whose Fail implementation
// does not return the right underlying error.
pub fn cause_workaround(fail: &dyn StdError) -> Option<&dyn StdError> {
    let mut cause = fail.source()?;
    if let Some(shared) = cause.downcast_ref::<SharedError<Compat<Error>>>() {
        cause = shared.0.deref();
    }
    Some(cause)
}
