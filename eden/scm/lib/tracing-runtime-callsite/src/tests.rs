/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::*;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use regex::Regex;
use std::fmt;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::AcqRel;
use std::sync::Arc;
use tracing::span::Attributes;
use tracing::span::Record;
use tracing::Callsite;
use tracing::Event;
use tracing::Id;
use tracing::Level;
use tracing::Metadata;
use tracing::Subscriber;

#[test]
fn test_callsite_span() {
    let callsite = create_callsite::<SpanKindType, _>((11, 0), || CallsiteInfo {
        name: "foo".to_string(),
        target: "bar".to_string(),
        level: Level::ERROR,
        file: Some("a.rs".to_string()),
        line: Some(10),
        module_path: Some("z::a".to_string()),
        field_names: vec!["x".to_string(), "y".to_string(), "z".to_string()],
    });
    assert_eq!(
        d(callsite.metadata()),
        "Metadata { name: foo, target: bar, level: Level(Error), module_path: z::a, location: a.rs:10, fields: {x, y, z}, callsite: _, kind: Kind(Span) }"
    );
    assert_eq!(callsite.identifier(), callsite.metadata().callsite());
    let log = capture(|| {
        let span = callsite.create_span(&[None, None, None]);
        span.record("y", &"yyy2");
        span.in_scope(|| {});
        let span = callsite.create_span(&[Some(Box::new("foo")), None, Some(Box::new(123))]);
        span.record("x", &123);
        span.in_scope(|| {});
    });
    assert_eq!(
        log,
        [
            "new_span(Attributes { metadata: Metadata { name: foo, target: bar, level: Level(Error), module_path: z::a, location: a.rs:10, fields: {x, y, z}, callsite: _, kind: Kind(Span) }, values: ValueSet { callsite: _ }, parent: Current } = 1",
            "record(Id(1), Record { values: ValueSet { y: yyy2, callsite: _ } })",
            "enter(Id(1))",
            "exit(Id(1))",
            "new_span(Attributes { metadata: Metadata { name: foo, target: bar, level: Level(Error), module_path: z::a, location: a.rs:10, fields: {x, y, z}, callsite: _, kind: Kind(Span) }, values: ValueSet { x: foo, z: 123, callsite: _ }, parent: Current } = 2",
            "record(Id(2), Record { values: ValueSet { x: 123, callsite: _ } })",
            "enter(Id(2))",
            "exit(Id(2))"
        ]
    );
}

#[test]
fn test_callsite_event() {
    let callsite = create_callsite::<EventKindType, _>((22, 0), || CallsiteInfo {
        name: "foo".to_string(),
        level: Level::ERROR,
        field_names: vec!["x".to_string(), "y".to_string(), "z".to_string()],
        ..Default::default()
    });
    assert_eq!(
        d(callsite.metadata()),
        "Metadata { name: foo, target: , level: Level(Error), fields: {x, y, z}, callsite: _, kind: Kind(Event) }"
    );
    assert_eq!(callsite.identifier(), callsite.metadata().callsite());
    let log = capture(|| {
        callsite.create_event(&[None, None, None]);
        callsite.create_event(&[Some(Box::new(12)), None, Some(Box::new("zz"))]);
        callsite.create_event(&[Some(Box::new("15"))]);
    });
    assert_eq!(
        log,
        [
            "event(Event { fields: ValueSet { callsite: _ }, metadata: Metadata { name: foo, target: , level: Level(Error), fields: {x, y, z}, callsite: _, kind: Kind(Event) }, parent: Current })",
            "event(Event { fields: ValueSet { x: 12, z: zz, callsite: _ }, metadata: Metadata { name: foo, target: , level: Level(Error), fields: {x, y, z}, callsite: _, kind: Kind(Event) }, parent: Current })",
            "event(Event { fields: ValueSet { x: 15, callsite: _ }, metadata: Metadata { name: foo, target: , level: Level(Error), fields: {x, y, z}, callsite: _, kind: Kind(Event) }, parent: Current })"
        ]
    );
}

#[test]
fn test_callsite_reuse() {
    let callsite1 = create_callsite::<EventKindType, _>((33, 1), CallsiteInfo::default);
    let callsite2 = create_callsite::<EventKindType, _>((33, 1), CallsiteInfo::default);
    assert_eq!(callsite1 as *const _, callsite2 as *const _);
}

#[test]
fn test_intern() {
    use crate::Intern;
    let s1 = "abc".intern();
    let s2 = "abc".to_string().intern();
    assert_eq!(s1.as_ptr(), s2.as_ptr());
}

/// Capture logs about tracing.
fn capture(f: impl FnOnce()) -> Vec<String> {
    // Prevent races since tests run in multiple threads.
    let _locked = THREAD_LOCK.lock();
    let sub = TestSubscriber::default();
    let out = sub.out.clone();
    tracing::subscriber::with_default(sub, f);
    let out = out.lock();
    out.clone()
}

/// Subscriber that captures calls to a string.
#[derive(Default)]
struct TestSubscriber {
    id: AtomicU64,
    out: Arc<Mutex<Vec<String>>>,
}
impl TestSubscriber {
    fn log(&self, s: String) {
        self.out.lock().push(normalize(&s));
    }
}
impl Subscriber for TestSubscriber {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }
    fn new_span(&self, span: &Attributes) -> Id {
        let id = self.id.fetch_add(1, AcqRel) + 1;
        self.log(format!("new_span({:?} = {}", span, id));
        Id::from_u64(id)
    }
    fn record(&self, span: &Id, values: &Record) {
        self.log(format!("record({:?}, {:?})", span, values));
    }
    fn event(&self, event: &Event) {
        self.log(format!("event({:?})", event));
    }
    fn enter(&self, span: &Id) {
        self.log(format!("enter({:?})", span));
    }
    fn exit(&self, span: &Id) {
        self.log(format!("exit({:?})", span));
    }
    fn record_follows_from(&self, span: &Id, follows: &Id) {
        self.log(format!("record_follows_from({:?}, {:?})", span, follows));
    }
}

/// Debug format with some normalization.
fn d<T: fmt::Debug>(t: T) -> String {
    let s = format!("{:?}", t);
    normalize(&s)
}

fn normalize(s: &str) -> String {
    // Change "Identifier(...)" to "_". It has dynamic pointer.
    IDENTIFIER_RE.replace_all(&s, "_").replace('"', "")
}

static THREAD_LOCK: Lazy<Mutex<()>> = Lazy::new(Default::default);
static IDENTIFIER_RE: Lazy<Regex> = Lazy::new(|| Regex::new("Identifier\\([^)]*\\)").unwrap());
