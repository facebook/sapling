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

use std::sync::Arc;

use scuba_ext::ScubaSampleBuilder;
use slog::{Logger, OwnedKV, SendSyncRefUnwindSafeKV};
use tracing::TraceContext;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CoreContext {
    inner: Arc<Inner>,
}

#[derive(Debug, Clone)]
struct Inner {
    session: Uuid,
    logger: Logger,
    scuba: ScubaSampleBuilder,
    // Logging some prod wireproto requests to scribe so that they can be later replayed on
    // shadow tier.
    wireproto_scribe_category: Option<String>,
    trace: TraceContext,
}

impl CoreContext {
    pub fn new(
        session: Uuid,
        logger: Logger,
        scuba: ScubaSampleBuilder,
        wireproto_scribe_category: Option<String>,
        trace: TraceContext,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                session,
                logger,
                scuba,
                wireproto_scribe_category,
                trace,
            }),
        }
    }

    pub fn with_logger_kv<T>(&self, values: OwnedKV<T>) -> Self
    where
        T: SendSyncRefUnwindSafeKV + 'static,
    {
        Self {
            inner: Arc::new(Inner {
                session: self.inner.session.clone(),
                logger: self.inner.logger.new(values),
                scuba: self.inner.scuba.clone(),
                wireproto_scribe_category: self.inner.wireproto_scribe_category.clone(),
                trace: self.inner.trace.clone(),
            }),
        }
    }

    pub fn test_mock() -> Self {
        Self::new(
            Uuid::new_v4(),
            Logger::root(::slog::Discard, o!()),
            ScubaSampleBuilder::with_discard(),
            None,
            TraceContext::default(),
        )
    }

    pub fn session(&self) -> &Uuid {
        &self.inner.session
    }
    pub fn logger(&self) -> &Logger {
        &self.inner.logger
    }
    pub fn scuba(&self) -> &ScubaSampleBuilder {
        &self.inner.scuba
    }
    pub fn wireproto_scribe_category(&self) -> &Option<String> {
        &self.inner.wireproto_scribe_category
    }
    pub fn trace(&self) -> &TraceContext {
        &self.inner.trace
    }
}
