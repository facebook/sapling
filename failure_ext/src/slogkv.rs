/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use super::{Compat, Error, Fail};
use futures::future::SharedError;

pub struct SlogKVError(pub Error);

impl slog::KV for SlogKVError {
    fn serialize(
        &self,
        _record: &slog::Record<'_>,
        serializer: &mut dyn slog::Serializer,
    ) -> slog::Result {
        let err = &self.0;

        serializer.emit_str(Error.to_str(), &format!("{}", err))?;
        serializer.emit_str(Backtrace.to_str(), &format!("{:#?}", err.backtrace()))?;

        let mut fail = err.as_fail();
        while let Some(cause) = cause_workaround(fail) {
            serializer.emit_str(Cause.to_str(), &format!("{}", cause))?;
            fail = cause;
        }
        serializer.emit_str(RootCause.to_str(), &format!("{:#?}", fail))?;

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
fn cause_workaround(fail: &dyn Fail) -> Option<&dyn Fail> {
    let mut cause = fail.cause()?;
    if let Some(shared) = cause.downcast_ref::<SharedError<Compat<Error>>>() {
        cause = shared.get_ref().as_fail();
    }
    Some(cause)
}
