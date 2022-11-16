/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;

use serde::Serialize;
use tracing::span::Attributes;
use tracing::span::Id;
use tracing::span::Record;
use tracing::Event;
use tracing::Level;
use tracing::Metadata;
use tracing::Subscriber;
use wasm_bindgen::prelude::*;

#[derive(Serialize, Clone)]
struct JsTracingSubscriber {
    #[serde(skip)]
    level: Level,
    spans: Arc<Mutex<Vec<MetadataMap>>>,
    log: Arc<Mutex<Vec<(Action, u32)>>>,
}

#[derive(Serialize)]
enum Action {
    Enter,
    Exit,
    Event,
}

#[derive(Serialize)]
#[serde(transparent)]
struct MetadataMap(BTreeMap<&'static str, Cow<'static, str>>);

struct FieldVisitor<'a> {
    out: &'a mut MetadataMap,
}

impl MetadataMap {
    fn from_metadata(metadata: &Metadata<'static>) -> Self {
        let mut map = BTreeMap::new();
        map.insert("name", metadata.name().into());
        if let Some(module_path) = metadata.module_path() {
            map.insert("module_path", module_path.into());
        }
        if let Some(line) = metadata.line() {
            map.insert("line", line.to_string().into());
        }
        Self(map)
    }
}

impl Subscriber for JsTracingSubscriber {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() >= &self.level
    }
    fn new_span(&self, span: &Attributes<'_>) -> Id {
        let id = self.push_metadata(span.metadata(), None);
        Id::from_u64(id as _)
    }
    fn record(&self, span_id: &Id, span_values: &Record<'_>) {
        let id = (span_id.into_u64() - 1) as usize;
        let mut spans = self.spans.lock().unwrap();
        let mut visitor = FieldVisitor {
            out: &mut spans[id],
        };
        span_values.record(&mut visitor);
    }
    fn record_follows_from(&self, span_id: &Id, follows_id: &Id) {
        let _ = (span_id, follows_id);
    }
    fn event(&self, event: &Event<'_>) {
        let id = self.push_metadata(event.metadata(), Some(event));
        self.push_log(Action::Event, id);
    }
    fn enter(&self, span_id: &Id) {
        self.push_log(Action::Enter, span_id.into_u64() as _);
    }
    fn exit(&self, span_id: &Id) {
        self.push_log(Action::Exit, span_id.into_u64() as _);
    }
}

impl JsTracingSubscriber {
    fn new(level: Level) -> Self {
        Self {
            level,
            spans: Default::default(),
            log: Default::default(),
        }
    }

    fn push_metadata(&self, metadata: &Metadata<'static>, event: Option<&Event>) -> usize {
        let mut spans = self.spans.lock().unwrap();
        let mut map = MetadataMap::from_metadata(metadata);
        let mut visitor = FieldVisitor { out: &mut map };
        if let Some(event) = event {
            event.record(&mut visitor);
        }
        spans.push(map);
        spans.len()
    }

    fn push_log(&self, action: Action, id: usize) {
        let mut log = self.log.lock().unwrap();
        log.push((action, id as _))
    }

    fn to_jsvalue(&self) -> JsValue {
        serde_wasm_bindgen::to_value(self).unwrap()
    }
}

impl<'a> FieldVisitor<'a> {
    fn record(&mut self, field: &tracing::field::Field, value: impl ToString) {
        let key = field.name();
        let value = value.to_string();
        self.out.0.insert(key, value.into());
    }
}

impl<'a> tracing::field::Visit for FieldVisitor<'a> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.record(field, format!("{:?}", value));
    }
    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.record(field, value)
    }
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.record(field, value)
    }
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.record(field, value)
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.record(field, value)
    }
    fn record_error(
        &mut self,
        field: &tracing::field::Field,
        value: &(dyn std::error::Error + 'static),
    ) {
        self.record(field, value)
    }
}

/// Calls f() with tracing captured. Returns trace.
#[wasm_bindgen]
pub fn withTracing(f: &js_sys::Function, level: Option<u32>) -> Result<JsValue, JsValue> {
    let this = JsValue::NULL;
    let level: Level = level.map(u32_to_level).unwrap_or(Level::TRACE);
    let subscriber = JsTracingSubscriber::new(level);
    // Ideally we can return f() as a second return value. But it seems tricky to do so
    // with the current version of wasm-bindgen (0.2.83).
    let _ = tracing::subscriber::with_default(subscriber.clone(), || f.call0(&this))?;
    let trace = subscriber.to_jsvalue();
    Ok(trace)
}

fn u32_to_level(v: u32) -> Level {
    match v {
        0 => Level::ERROR,
        1 => Level::WARN,
        2 => Level::INFO,
        3 => Level::DEBUG,
        _ => Level::TRACE,
    }
}
