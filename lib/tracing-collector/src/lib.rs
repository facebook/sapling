/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

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
pub struct TracingCollector {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::instrument;

    #[instrument]
    fn fib(x: u32) -> u32 {
        match x {
            0 | 1 => 1,
            2 => 2,
            _ => fib(x - 1) + fib(x - 2),
        }
    }

    #[test]
    fn test_instrument() {
        let data = TracingData::new_for_test();
        let data = Arc::new(Mutex::new(data));
        let collector = TracingCollector::new(data.clone(), Level::INFO);

        tracing::subscriber::with_default(collector, || fib(5));

        let mut data = data.lock();
        // Replace line numbers so the test is more stable.
        // Also rewrite module name, since the buck test mangles crate name too.
        for id in 0..20 {
            data.edit_espan(
                crate::model::EspanId(id),
                vec![("line", "<line>"), ("module_path", "<mod>")],
            );
        }
        assert_eq!(
            data.ascii(&Default::default()),
            r#"Process _ Thread _:
Start Dur.ms | Name               Source
    2    +34 | fib                <mod> line <line>
             | - x = 5            :
    4    +18  \ fib               <mod> line <line>
               | - x = 4          :
    6    +10   \ fib              <mod> line <line>
                | - x = 3         :
    8     +2    \ fib             <mod> line <line>
                 | - x = 2        :
   12     +2    \ fib             <mod> line <line>
                 | - x = 1        :
   18     +2   \ fib              <mod> line <line>
                | - x = 2         :
   24    +10  \ fib               <mod> line <line>
               | - x = 3          :
   26     +2   \ fib              <mod> line <line>
                | - x = 2         :
   30     +2   \ fib              <mod> line <line>
                | - x = 1         :

"#
        );
    }

    #[test]
    fn test_multi_threads() {
        let data = TracingData::new_for_test();
        let data = Arc::new(Mutex::new(data));
        let collector = TracingCollector::new(data.clone(), Level::INFO);

        tracing::subscriber::with_default(collector, || fib(0));
        let cloned = data.clone();
        let thread = std::thread::spawn(|| {
            let collector = TracingCollector::new(cloned, Level::INFO);
            tracing::subscriber::with_default(collector, || fib(3));
        });
        thread.join().unwrap();

        let cloned = data.clone();
        let thread = std::thread::spawn(|| {
            let collector = TracingCollector::new(cloned, Level::INFO);
            tracing::subscriber::with_default(collector, || fib(2));
        });
        thread.join().unwrap();
        for id in 0..20 {
            data.lock().edit_espan(
                crate::model::EspanId(id),
                vec![("line", "<line>"), ("module_path", "<mod>")],
            );
        }

        assert_eq!(
            data.lock().ascii(&Default::default()),
            r#"Process _ Thread _:
Start Dur.ms | Name               Source
    2     +2 | fib                <mod> line <line>
             | - x = 0            :

Process _ Thread _:
Start Dur.ms | Name               Source
    6    +10 | fib                <mod> line <line>
             | - x = 3            :
    8     +2  \ fib               <mod> line <line>
               | - x = 2          :
   12     +2  \ fib               <mod> line <line>
               | - x = 1          :

Process _ Thread _:
Start Dur.ms | Name               Source
   18     +2 | fib                <mod> line <line>
             | - x = 2            :

"#
        );
    }
}
