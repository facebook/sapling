/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use slog::Drain;
use slog::Level;
use slog::Never;
use slog::OwnedKVList;
use slog::Record;

use crate::context::ObservabilityContext;

pub struct DynamicLevelDrain<D> {
    inner: D,
    observability_context: ObservabilityContext,
}

impl<D> DynamicLevelDrain<D> {
    pub fn new(inner: D, observability_context: ObservabilityContext) -> Self {
        Self {
            inner,
            observability_context,
        }
    }

    fn current_level(&self) -> Level {
        self.observability_context.get_logging_level()
    }
}

impl<D: Drain<Ok = (), Err = Never>> Drain for DynamicLevelDrain<D> {
    type Ok = ();
    type Err = Never;

    fn log(&self, record: &Record, values: &OwnedKVList) -> Result<Self::Ok, Self::Err> {
        if record.level().is_at_least(self.current_level()) {
            self.inner.log(record, values)
        } else {
            Ok(())
        }
    }
}
