// Copyright 2004-present Facebook. All Rights Reserved.

use super::Error;
use slog;

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
