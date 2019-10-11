/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use super::Error;

pub struct SlogKVError(pub Error);

impl slog::KV for SlogKVError {
    fn serialize(
        &self,
        _record: &slog::Record<'_>,
        serializer: &mut dyn slog::Serializer,
    ) -> slog::Result {
        let err = &self.0;

        serializer.emit_str(Error.to_str(), &format!("{}", err))?;
        serializer.emit_str(RootCause.to_str(), &format!("{:#?}", err.find_root_cause()))?;
        serializer.emit_str(Backtrace.to_str(), &format!("{:#?}", err.backtrace()))?;

        for c in err.iter_chain().skip(1) {
            serializer.emit_str(Cause.to_str(), &format!("{}", c))?;
        }

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
