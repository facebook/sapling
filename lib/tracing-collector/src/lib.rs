// Copyright (c) Facebook, Inc. and its affiliates.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! See [`TracingCollector`] for the main structure.

#![allow(unused_variables)]
#![allow(dead_code)]

pub mod model;
pub use model::TracingData;

use parking_lot::Mutex;
use std::sync::Arc;
use tracing::{
    span::{Attributes, Record},
    Event, Id, Level, Metadata, Subscriber,
};

/// A `tokio/tracing` subscriber that collects tracing data to [`TracingData`].
/// [`TracingData`] is independent from `tokio/tracing`. See its docstring for
/// more details.
struct TracingCollector {
    level: Level,
    data: Arc<Mutex<TracingData>>,
}

impl TracingCollector {
    pub fn new(data: Arc<Mutex<TracingData>>, level: Level) -> Self {
        Self { level, data }
    }
}

impl Subscriber for TracingCollector {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= &self.level
    }

    fn new_span(&self, span: &Attributes) -> Id {
        let mut data = self.data.lock();
        data.new_span(span)
    }

    fn record(&self, span: &Id, values: &Record) {
        let mut data = self.data.lock();
        data.record(span, values)
    }

    fn record_follows_from(&self, span: &Id, follows: &Id) {
        let mut data = self.data.lock();
        data.record_follows_from(span, follows)
    }

    fn event(&self, event: &Event) {
        let mut data = self.data.lock();
        data.event(event)
    }

    fn enter(&self, span: &Id) {
        let mut data = self.data.lock();
        data.enter(span)
    }

    fn exit(&self, span: &Id) {
        let mut data = self.data.lock();
        data.exit(span)
    }
}
