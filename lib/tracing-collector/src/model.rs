// Copyright (c) Facebook, Inc. and its affiliates.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};

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
    // FIXME: Implement THREAD_ID
    pub static THREAD_ID: u64 = 0;
}

// -------- Integration with "tokio/tracing" --------

// Matches `tracing::Subscriber` APIs.
impl TracingData {
    /// Matches `tracing::Subscriber::new_span`.
    pub fn new_span(&mut self, attributes: &tracing::span::Attributes) -> tracing::span::Id {
        unimplemented!()
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
        unimplemented!()
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
