/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use slog::{Drain, Level, Never, OwnedKVList, Record};

use crate::context::ObservabilityContext;

pub struct DynamicLevelDrain<'a, D> {
    inner: D,
    observability_context: &'a ObservabilityContext,
}

impl<'a, D> DynamicLevelDrain<'a, D> {
    pub fn new(inner: D, observability_context: &'a ObservabilityContext) -> Self {
        Self {
            inner,
            observability_context,
        }
    }

    fn current_level(&self) -> Level {
        self.observability_context.get_logging_level()
    }
}

impl<D: Drain<Ok = (), Err = Never>> Drain for DynamicLevelDrain<'_, D> {
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
