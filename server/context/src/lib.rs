// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate scuba_ext;
#[macro_use]
extern crate slog;
extern crate tracing;
extern crate uuid;

use scuba_ext::ScubaSampleBuilder;
use slog::Logger;
use tracing::TraceContext;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CoreContext {
    pub session: Uuid,
    pub logger: Logger,
    pub scuba: ScubaSampleBuilder,
    // Logging some prod wireproto requests to scribe so that they can be later replayed on
    // shadow tier.
    pub wireproto_scribe_category: Option<String>,
    pub trace: TraceContext,
}

impl CoreContext {
    pub fn test_mock() -> Self {
        Self {
            session: Uuid::new_v4(),
            logger: Logger::root(::slog::Discard, o!()),
            scuba: ScubaSampleBuilder::with_discard(),
            wireproto_scribe_category: None,
            trace: TraceContext::default(),
        }
    }

    pub fn session(&self) -> &Uuid {
        &self.session
    }
    pub fn logger(&self) -> &Logger {
        &self.logger
    }
    pub fn scuba(&self) -> &ScubaSampleBuilder {
        &self.scuba
    }
    pub fn wireproto_scribe_category(&self) -> &Option<String> {
        &self.wireproto_scribe_category
    }
    pub fn trace(&self) -> &TraceContext {
        &self.trace
    }
}
