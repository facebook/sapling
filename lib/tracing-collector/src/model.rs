// Copyright (c) Facebook, Inc. and its affiliates.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::ops::DerefMut;

/// Collected tracing data.
///
/// This is a struct that is designed to:
/// - support serialize and deserialize, in a relatively compat way.
/// - support getting data from tokio/tracing APIs.
/// - support getting data from non-tokio/tracing APIs (ex. Python bindings).
/// - support rendering into chrome trace format.
/// - not coupled with tokio/tracing data structures.
#[derive(Serialize, Deserialize)]
pub struct TracingData {
    /// Interned strings (so they can be referred by StringId).
    strings: InternedStrings,

    /// Spans or Events (so they can be referred by EspanId).
    espans: Vec<Espan>,

    /// EnterSpan/ExitSpan/TriggerEvent events with timestamp and thread
    /// information.
    eventus: Vec<Eventus>,

    /// Start time.
    start: std::time::SystemTime,

    /// Default process ID.
    default_process_id: u64,

    /// Default thread ID.
    default_thread_id: u64,

    /// Relative start time (so other timestamps can use relative form).
    #[serde(skip, default = "std::time::Instant::now")]
    relative_start: std::time::Instant,
}

#[derive(Serialize, Deserialize, Default)]
struct InternedStrings(IndexSet<String>);

impl InternedStrings {
    /// Convert a string to an id.
    fn id(&mut self, s: impl ToString) -> StringId {
        let (id, _existed) = self.0.insert_full(s.to_string());
        StringId(id as u64)
    }

    /// Convert an id to a string
    fn get(&self, id: StringId) -> &str {
        match self.0.get_index(id.0 as usize) {
            Some(s) => s,
            None => "<missing>",
        }
    }
}

/// Span or Event.
#[derive(Serialize, Deserialize)]
struct Espan {
    /// Key-value metadata.
    meta: IndexMap<StringId, StringId>,
}

#[derive(Serialize, Deserialize)]
struct Eventus {
    action: Action,
    timestamp: RelativeTime,
    espan_id: EspanId,
    process_id: u64,
    thread_id: u64,
}

#[derive(Serialize, Deserialize)]
pub enum Action {
    EnterSpan,
    ExitSpan,
    Event,
}

#[derive(Clone, Copy, Deserialize, Eq, Hash, PartialEq, PartialOrd, Serialize)]
struct StringId(u64);

#[derive(Clone, Copy, Deserialize, Eq, Hash, PartialEq, PartialOrd, Serialize)]
pub struct EspanId(pub u64);

#[derive(Clone, Copy, Deserialize, Eq, Hash, PartialEq, PartialOrd, Serialize)]
struct RelativeTime(u64);

impl TracingData {
    pub fn new() -> TracingData {
        THREAD_ID.with(|thread_id| TracingData {
            start: std::time::SystemTime::now(),
            relative_start: std::time::Instant::now(),
            default_process_id: unsafe { libc::getpid() } as u64,
            default_thread_id: *thread_id,
            strings: Default::default(),
            espans: Default::default(),
            eventus: Default::default(),
        })
    }

    /// Push an `Eventus` at the current timestamp.
    fn push_eventus(&mut self, action: Action, espan_id: EspanId) {
        let timestamp = self.now_micros();
        let mut thread_id = THREAD_ID.with(|thread_id| *thread_id);
        if thread_id == self.default_thread_id {
            thread_id = 0;
        }
        let eventus = Eventus {
            action,
            timestamp,
            espan_id,
            process_id: 0, // special value: use `self.process_id`.
            thread_id,
        };
        self.eventus.push(eventus)
    }

    /// Get the current relative time, in microseconds.
    fn now_micros(&self) -> RelativeTime {
        RelativeTime(
            std::time::Instant::now()
                .duration_since(self.relative_start)
                .as_micros() as u64,
        )
    }
}

thread_local! {
    pub static THREAD_ID: u64 = loop {
        #[cfg(target_os = "linux")]
        {
            break unsafe { libc::syscall(libc::SYS_gettid) as u64 };
        }
        #[cfg(target_os = "macos")]
        {
            #[link(name = "pthread")]
            extern "C" {
                fn pthread_threadid_np(
                    thread: libc::pthread_t,
                    thread_id: *mut libc::uint64_t,
                ) -> libc::c_int;
            }
            let mut thread_id = 0;
            unsafe { pthread_threadid_np(0, &mut thread_id) };
            break thread_id;
        }
        #[cfg(windows)]
        {
            break unsafe { winapi::um::processthreadsapi::GetCurrentThreadId() as u64 };
        }
        #[allow(unreachable_code)]
        {
            break 0;
        }
    };
}

// -------- Integration with "tokio/tracing" --------

// Matches `tracing::Subscriber` APIs.
impl TracingData {
    /// Matches `tracing::Subscriber::new_span`.
    pub fn new_span(&mut self, attributes: &tracing::span::Attributes) -> tracing::span::Id {
        self.push_espan(attributes).into()
    }

    /// Matches `tracing::Subscriber::record`.
    pub fn record(&mut self, id: &tracing::span::Id, values: &tracing::span::Record) {
        let id: EspanId = id.clone().into();
        let meta = &mut self.espans[id.0 as usize].meta;
        let mut visitor = FieldVisitor::new(&mut self.strings, meta);
        values.record(&mut visitor)
    }

    /// Matches `tracing::Subscriber::record_follows_from`.
    pub fn record_follows_from(&mut self, id: &tracing::span::Id, follows: &tracing::span::Id) {
        // TODO: Implement this.
    }

    /// Matches `tracing::Subscriber::event`.
    pub fn event(&mut self, event: &tracing::event::Event) {
        let id = self.push_espan(event);
        self.push_eventus(Action::Event, id);
    }

    /// Matches `tracing::Subscriber::enter`.
    pub fn enter(&mut self, id: &tracing::span::Id) {
        let id = id.clone().into();
        self.push_eventus(Action::EnterSpan, id);
    }

    /// Matches `tracing::Subscriber::exit`.
    pub fn exit(&mut self, id: &tracing::span::Id) {
        let id = id.clone().into();
        self.push_eventus(Action::ExitSpan, id);
    }

    /// Push a Span or Event. Return its Id.
    fn push_espan(&mut self, espan: &impl EspanLike) -> EspanId {
        let mut meta = IndexMap::with_capacity(3);
        if let Some(parent_id) = espan.parent_id() {
            meta.insert(self.strings.id("parent"), self.strings.id(parent_id.0));
        }

        espan.record_values(&mut self.strings, &mut meta);

        let espan = Espan { meta };

        let result = EspanId(self.espans.len() as u64);
        self.espans.push(espan);
        result.into()
    }
}

// Id type conversions - EspanId can be 0 while tracing::span::Id cannot.

impl From<tracing::span::Id> for EspanId {
    fn from(id: tracing::span::Id) -> EspanId {
        EspanId(id.into_u64() - 1)
    }
}

impl From<EspanId> for tracing::span::Id {
    fn from(id: EspanId) -> tracing::span::Id {
        tracing::span::Id::from_u64(id.0 + 1)
    }
}

// The only way to get data out from [`tracing::field::ValueSet`] is to
// implement a [`tracing::field::Visit`].
//
// This `Visit` just converts everything to string.
/// Extract content from [`tracing::field::ValueSet`] to key-value strings.
struct FieldVisitor<'a> {
    strings: &'a mut InternedStrings,
    meta: &'a mut IndexMap<StringId, StringId>,
}

impl<'a> FieldVisitor<'a> {
    pub fn new(
        strings: &'a mut InternedStrings,
        meta: &'a mut IndexMap<StringId, StringId>,
    ) -> Self {
        Self { strings, meta }
    }
}

impl<'a> FieldVisitor<'a> {
    fn record(&mut self, field: &tracing::field::Field, value: impl ToString) {
        let key = self.strings.id(field.name());
        let value = self.strings.id(value.to_string());
        self.meta.insert(key, value);
    }
}

impl<'a> tracing::field::Visit for FieldVisitor<'a> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.record(field, format!("{:?}", value));
    }
    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        // NOTE: Maybe consider doing '+' here?
        self.record(field, value)
    }
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        // NOTE: Maybe consider doing '+' here?
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

/// Common methods exposed by [`tracing::Span`] and [`tracing::Event`]
trait EspanLike {
    /// Optional. Parent [`EspanId`].
    fn parent_id(&self) -> Option<EspanId>;

    /// Write key-value map to `strings` and `meta` that are not coupled with
    /// `tokio/tracing`.
    fn record_values(&self, strings: &mut InternedStrings, meta: &mut IndexMap<StringId, StringId>);
}

impl EspanLike for tracing::span::Attributes<'_> {
    fn parent_id(&self) -> Option<EspanId> {
        self.parent().cloned().map(Into::into)
    }

    fn record_values(
        &self,
        strings: &mut InternedStrings,
        meta: &mut IndexMap<StringId, StringId>,
    ) {
        record_tracing_metadata(self.metadata(), strings, meta);
        let mut visitor = FieldVisitor::new(strings, meta);
        self.record(&mut visitor)
    }
}

impl EspanLike for tracing::Event<'_> {
    fn parent_id(&self) -> Option<EspanId> {
        self.parent().cloned().map(Into::into)
    }

    fn record_values(
        &self,
        strings: &mut InternedStrings,
        meta: &mut IndexMap<StringId, StringId>,
    ) {
        record_tracing_metadata(self.metadata(), strings, meta);
        let mut visitor = FieldVisitor::new(strings, meta);
        self.record(&mut visitor)
    }
}

/// Write static key-value data (`tracing::Metadata`) to `output_meta`.
fn record_tracing_metadata(
    tracing_metadata: &tracing::Metadata<'static>,
    output_strings: &mut InternedStrings,
    output_meta: &mut IndexMap<StringId, StringId>,
) {
    output_meta.insert(
        output_strings.id("name"),
        output_strings.id(tracing_metadata.name()),
    );
    if let Some(module_path) = tracing_metadata.module_path() {
        output_meta.insert(
            output_strings.id("module_path"),
            output_strings.id(module_path),
        );
    }
    if let Some(line) = tracing_metadata.line() {
        output_meta.insert(
            output_strings.id("line"),
            output_strings.id(format!("{}", line)),
        );
    }
}

// -------- APIs for non-"tokio/tracing" use-cases --------

impl TracingData {
    /// Record a new [`Espan`] that can be used afterwards.
    ///
    /// If `reuse_espan_id` is not empty, and matches `key_values`,
    /// `reuse_espan_id` will be returned instead.
    pub fn add_espan(
        &mut self,
        key_values: &[(&str, &str)],
        reuse_espan_id: Option<EspanId>,
    ) -> EspanId {
        if let Some(reuse_espan_id) = reuse_espan_id {
            if let Some(orig_espan) = self.espans.get(reuse_espan_id.0 as usize) {
                if orig_espan
                    .meta
                    .iter()
                    .map(|(k, v)| (self.strings.get(*k), self.strings.get(*v)))
                    .cmp(key_values.iter().cloned())
                    == std::cmp::Ordering::Equal
                {
                    // Espan can be reused.
                    return reuse_espan_id;
                }
            }
        }

        let mut meta = IndexMap::with_capacity(key_values.len());

        for (key, value) in key_values {
            meta.insert(
                self.strings.id(key.to_string()),
                self.strings.id(value.to_string()),
            );
        }

        let espan = Espan { meta };

        let result = EspanId(self.espans.len() as u64);
        self.espans.push(espan);
        result.into()
    }

    /// Edit key-value data to an existing [`Espan`].
    pub fn edit_espan<S1: ToString, S2: ToString>(
        &mut self,
        id: EspanId,
        key_values: impl IntoIterator<Item = (S1, S2)>,
    ) {
        if let Some(espan) = self.espans.get_mut(id.0 as usize) {
            for (key, value) in key_values {
                espan.meta.insert(
                    self.strings.id(key.to_string()),
                    self.strings.id(value.to_string()),
                );
            }
        }
    }

    /// Record a new "Action" about an [`EspanId`].
    pub fn add_action(&mut self, espan_id: EspanId, action: Action) {
        self.push_eventus(action, espan_id);
    }

    /// Mark `new_span_id` as following `old_span_id`.
    pub fn set_follows_from(&mut self, old_span_id: EspanId, new_span_id: EspanId) {
        // TODO: Implement this.
    }
}

// -------- Convert to Trace Event format (Chrome Trace) --------

/// Zero-copy `serde_json::Value` alternative.
#[derive(Serialize)]
#[serde(untagged)]
enum RefValue<'a> {
    Str(&'a str),
    Int(u64),
    Map(IndexMap<&'a str, RefValue<'a>>),
}

impl From<u64> for RefValue<'_> {
    fn from(v: u64) -> Self {
        RefValue::Int(v)
    }
}

impl<'a> From<&'a str> for RefValue<'a> {
    fn from(v: &'a str) -> Self {
        RefValue::Str(v)
    }
}

impl<'a> From<IndexMap<&'a str, RefValue<'a>>> for RefValue<'a> {
    fn from(v: IndexMap<&'a str, RefValue<'a>>) -> Self {
        RefValue::Map(v)
    }
}

impl<'a> RefValue<'a> {
    fn insert(&mut self, name: &'a str, value: impl Into<RefValue<'a>>) {
        if let RefValue::Map(obj) = self {
            obj.insert(name, value.into());
        }
    }
}

macro_rules! object {
    ({ $( $k:ident : $v:expr, )* }) => {{
        #[allow(unused_mut)]
        let mut obj = IndexMap::new();
        $( obj.insert(stringify!($k), object!($v)); )*
        $crate::model::RefValue::Map(obj)
    }};
    ($v: expr) => { RefValue::from($v) };
}

impl TracingData {
    /// Write "Trace Event" format that can be viewed by Chrome "about:tracing".
    ///
    /// See https://github.com/catapult-project/catapult/tree/master/tracing.
    pub fn write_trace_event_json(
        &self,
        out: &mut dyn io::Write,
        other_data: HashMap<String, String>,
    ) -> Result<(), serde_json::Error> {
        // FEATURE: "Trace Event" supports a lot of things. Features to consider:
        // - Handle async events (set "id" to espan_id, and use async phase names).
        // - Translate Espan::follower_ids to "Flow Events" (if follower_ids get used).
        // - Using "Metadata Events" to add names to threads.

        // Extract string from espan.meta.
        let extract = |espan: &Espan, name: &str| -> Option<&str> {
            let meta = &espan.meta;
            if let Some((key_id, _)) = self.strings.0.get_full(name) {
                let key_id = StringId(key_id as u64);
                if let Some(value_id) = meta.get(&key_id) {
                    return Some(self.strings.get(*value_id));
                }
            }
            None
        };

        // Calculate JSON objects in a streaming way to reduce memory usage.
        let trace_event_iter = self.eventus.iter().map(|eventus| {
            let espan = &self.espans[eventus.espan_id.0 as usize];
            let ph = match eventus.action {
                Action::Event => "i",     // Instant Event
                Action::EnterSpan => "B", // Duration Event: Begin
                Action::ExitSpan => "E",  // Duration Event: End
            };
            let args: IndexMap<&str, RefValue> = espan
                .meta
                .iter()
                .filter(|(k, _v)| {
                    let s = self.strings.get(**k);
                    s != "name" && s != "cat"
                })
                .map(|(k, v)| (self.strings.get(*k), self.strings.get(*v).into()))
                .collect();
            let pid = match eventus.process_id as u64 {
                0 => self.default_process_id,
                v => v,
            };
            let tid = match eventus.thread_id {
                0 => self.default_thread_id,
                v => v,
            };
            let mut obj = object!({
                name: extract(espan, "name").unwrap_or("(unnamed)"),
                cat: extract(espan, "cat").unwrap_or("default"),
                ts: eventus.timestamp.0,
                pid: pid,
                tid: tid,
                ph: ph,
                args: args,
            });
            if ph == "i" {
                // Add "s": "p" (scope: process) for Instant Events.
                obj.insert("s", "p");
            }
            obj
        });

        #[allow(non_snake_case)]
        #[derive(Serialize)]
        struct Trace<'a, I: Iterator<Item = RefValue<'a>>> {
            #[serde(serialize_with = "serialize_iter")]
            traceEvents: RefCell<I>,
            displayTimeUnit: &'static str,
            otherData: HashMap<String, String>,
        }
        let trace = Trace {
            traceEvents: RefCell::new(trace_event_iter),
            displayTimeUnit: "ms",
            otherData: other_data,
        };

        serde_json::to_writer(out, &trace)
    }
}

fn serialize_iter<S, V, I>(iter: &RefCell<I>, s: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
    V: Serialize,
    I: Iterator<Item = V>,
{
    let mut iter = iter.borrow_mut();
    s.collect_seq(iter.deref_mut())
}
