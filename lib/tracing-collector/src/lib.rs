// Copyright (c) Facebook, Inc. and its affiliates.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! See [`TracingCollector`] for the main structure.

#![allow(unused_variables)]
#![allow(dead_code)]

use tracing::{
    span::{Attributes, Record},
    Event, Id, Metadata, Subscriber,
};

struct TracingCollector {}

impl Subscriber for TracingCollector {
    fn enabled(&self, metadata: &Metadata) -> bool {
        unimplemented!()
    }

    fn new_span(&self, span: &Attributes) -> Id {
        unimplemented!()
    }

    fn record(&self, span: &Id, values: &Record) {
        unimplemented!()
    }

    fn record_follows_from(&self, span: &Id, follows: &Id) {
        unimplemented!()
    }

    fn event(&self, event: &Event) {
        unimplemented!()
    }

    fn enter(&self, span: &Id) {
        unimplemented!()
    }

    fn exit(&self, span: &Id) {
        unimplemented!()
    }
}
