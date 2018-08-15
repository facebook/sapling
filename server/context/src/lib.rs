// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate scuba_ext;
extern crate slog;
extern crate tracing;

use scuba_ext::ScubaSampleBuilder;
use slog::Logger;
use tracing::TraceContext;

#[derive(Debug, Clone)]
pub struct CoreContext<T> {
    pub session: T,
    pub logger: Logger,
    pub scuba: ScubaSampleBuilder,
    pub trace: TraceContext,
}

impl<T> CoreContext<T> {
    pub fn session(&self) -> &T {
        &self.session
    }
    pub fn logger(&self) -> &Logger {
        &self.logger
    }
    pub fn scuba(&self) -> &ScubaSampleBuilder {
        &self.scuba
    }
    pub fn trace(&self) -> &TraceContext {
        &self.trace
    }
}
