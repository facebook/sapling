/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::io;
use std::ops::Deref;
use std::ops::DerefMut;
use std::rc::Rc;
use std::sync::atomic;
use std::sync::atomic::AtomicU64;

pub use indexmap::IndexMap;
pub use indexmap::IndexSet;
use serde::Deserialize;
use serde::Serialize;
use tracing::callsite::Identifier;

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

    /// Map from base collector ids to tracing data ids.
    // using u64 instead of tracing::span::id because the latter is not Serialize
    id_map: HashMap<u64, EspanId>,

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

    /// The first [`EspanId`] that maps to `espans[0]`.
    /// This is useful as a sanity check about valid [`EspanId`]s.
    espan_id_offset: EspanId,

    /// Number of EnterSpan or Event actions per callsite.
    ///
    /// This is useful to limit repetitive calls from the Rust tracing APIs.
    /// Callsite is a concept that exists in the Rust tracing APIs, not in
    /// the TracingData. Unlike calls to TracingData APIs, they will have
    /// different EspanId, but same callsite.
    #[serde(skip)]
    pub(crate) callsite_entered: HashMap<Identifier, usize>,

    /// For testing purpose.
    /// - 0: Use real clock.
    /// - Non-zero: Use a faked clock.
    #[serde(skip, default = "Default::default")]
    test_clock_step: u64,

    /// Maximum reference count of a span.
    #[serde(skip, default = "default_max_span_ref_count")]
    pub(crate) max_span_ref_count: usize,
}

fn default_max_span_ref_count() -> usize {
    1000
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

    /// Metadata from the tracing eco-system.
    #[serde(skip)]
    metadata: Option<&'static tracing::Metadata<'static>>,

    /// Reference count of the span.
    #[serde(skip, default = "Default::default")]
    ref_count: usize,
}

#[derive(Serialize, Deserialize, Clone)]
struct Eventus {
    action: Action,
    timestamp: RelativeTime,
    espan_id: EspanId,
    process_id: u64,
    thread_id: u64,
}

#[derive(Serialize, Clone, Copy, Deserialize)]
pub enum Action {
    EnterSpan,
    ExitSpan,
    Event,
}

#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize,
    Eq,
    Hash,
    PartialEq,
    PartialOrd,
    Serialize
)]
struct StringId(u64);

#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize,
    Eq,
    Hash,
    PartialEq,
    PartialOrd,
    Serialize
)]
pub struct EspanId(pub u64);

#[derive(
    Clone,
    Copy,
    Default,
    Deserialize,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize
)]
struct RelativeTime(u64);

impl TracingData {
    pub fn new() -> TracingData {
        THREAD_ID.with(|thread_id| TracingData {
            start: std::time::SystemTime::now(),
            relative_start: std::time::Instant::now(),
            default_process_id: unsafe { libc::getpid() } as u64,
            default_thread_id: *thread_id,
            strings: Default::default(),
            id_map: Default::default(),
            espans: Default::default(),
            eventus: Default::default(),
            espan_id_offset: next_espan_id_offset(),
            callsite_entered: Default::default(),
            test_clock_step: match std::env::var("TRACING_DATA_FAKE_CLOCK") {
                Ok(clock) => clock.parse::<u64>().unwrap_or(0),
                Err(_) => 0,
            },
            max_span_ref_count: default_max_span_ref_count(),
        })
    }

    pub fn set_max_span_ref_count(&mut self, value: usize) {
        self.max_span_ref_count = value;
    }

    /// Push an `Eventus` at the current timestamp.
    /// Return `true` if the [`Eventus`] was pushed.
    /// Return `false` if `espan_id` is invalid.
    fn push_eventus(&mut self, action: Action, espan_id: EspanId) -> bool {
        let espan_index = match self.get_espan_index(espan_id) {
            // Ignore invalid EspanId.
            None => return false,
            Some(index) => index,
        };
        if self.espans[espan_index].ref_count >= self.max_span_ref_count {
            // Exceed ref-count limit.
            return false;
        }
        if matches!(action, Action::ExitSpan | Action::Event) {
            // Increase ref count of the span.
            //
            // Do not increase on EnterSpan, making it harder to have unmatched
            // events (i.e. Enter without Exit).
            self.espans[espan_index].ref_count += 1;
        }
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
        self.eventus.push(eventus);
        true
    }

    /// Get the current relative time, in microseconds.
    fn now_micros(&self) -> RelativeTime {
        if self.test_clock_step == 0 {
            RelativeTime(
                std::time::Instant::now()
                    .duration_since(self.relative_start)
                    .as_micros() as u64,
            )
        } else {
            RelativeTime(self.eventus.last().map_or(0, |e| e.timestamp.0) + self.test_clock_step)
        }
    }

    /// Fetch a `Espan`. Does some minimal `EspanId` validation.
    /// Return `None` if the `Espan` is unknown to this [`TracingData`].
    fn get_espan(&self, id: EspanId) -> Option<&Espan> {
        if id < self.espan_id_offset {
            None
        } else {
            self.espans.get((id.0 - self.espan_id_offset.0) as usize)
        }
    }

    /// Similar to `get_espan`. But returns an index of `espans` instead.
    /// This is useful for mutating both `self.espans` and `self.strings`
    /// (returning `&mut Espan` from `&mut self` prevents modifications
    /// to `self.strings`).
    fn get_espan_index(&self, id: EspanId) -> Option<usize> {
        if id < self.espan_id_offset || id.0 > self.espan_id_offset.0 + self.espans.len() as u64 {
            None
        } else {
            Some((id.0 - self.espan_id_offset.0) as usize)
        }
    }

    pub(crate) fn insert_id_mapping(&mut self, trace_id: &tracing::span::Id, espan_id: EspanId) {
        self.id_map.insert(trace_id.into_u64(), espan_id);
    }

    pub(crate) fn get_espan_id_from_trace(
        &mut self,
        trace_id: &tracing::span::Id,
    ) -> Option<EspanId> {
        self.id_map.get(&trace_id.into_u64()).cloned()
    }
}

/// Used for new TracingData
static PROCESS_ESPAN_ID_FIRST: AtomicU64 = AtomicU64::new(0);

/// Next `espan_id_offset` that can be used in new [`TracingData`].
fn next_espan_id_offset() -> EspanId {
    let reserved_spans = 1 << 24;
    let id = PROCESS_ESPAN_ID_FIRST.fetch_add(reserved_spans, atomic::Ordering::SeqCst);
    EspanId(id)
}

thread_local! {
    #[allow(clippy::never_loop)]
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
                    thread_id: *mut u64,
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
    pub fn new_span(&mut self, attributes: &tracing::span::Attributes) -> EspanId {
        self.push_espan(attributes)
    }

    /// Matches `tracing::Subscriber::record`.
    pub fn record(&mut self, id: &tracing::span::Id, values: &tracing::span::Record) {
        let id: EspanId = id.clone().into();
        if let Some(espan_index) = self.get_espan_index(id) {
            let meta = &mut self.espans[espan_index].meta;
            let mut visitor = FieldVisitor::new(&mut self.strings, meta);
            values.record(&mut visitor)
        }
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

        let metadata = Some(espan.metadata());
        let espan = Espan {
            meta,
            metadata,
            ref_count: 0,
        };

        let result = EspanId(self.espans.len() as u64 + self.espan_id_offset.0);
        self.espans.push(espan);
        result
    }

    /// Rewrite `moudle_path` and `line` information so they stay stable
    /// across tests.
    #[cfg(test)]
    pub(crate) fn fixup_module_lines_for_tests(&mut self) {
        // buck tests can change the crate name to "<crate>_unittest"
        let module_path = self.strings.id("<mod>");
        let line = self.strings.id("<line>");
        for espan in self.espans.iter_mut() {
            let meta = &mut espan.meta;
            meta.insert(self.strings.id("module_path"), module_path);
            meta.insert(self.strings.id("line"), line);
        }
    }

    /// Count the logged events for test usecases.
    #[cfg(test)]
    pub(crate) fn eventus_len_for_tests(&self) -> usize {
        self.eventus.len()
    }
}

/// Display implementation to display a Span's metadata
pub(crate) struct DisplayEspan<'a>(&'a InternedStrings, &'a Espan, &'static str);

impl<'a> fmt::Display for DisplayEspan<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let strings = self.0;
        let mut padding = self.2;
        for (&key_id, &value_id) in self.1.meta.iter() {
            let key = strings.get(key_id);
            if key == "module_path" || key == "line" {
                // They are included in the static metadata.
                continue;
            }
            if key == "name" {
                write!(f, "{}{}", padding, strings.get(value_id))?;
            } else {
                write!(f, "{}{}={}", padding, key, strings.get(value_id))?;
            }
            padding = " ";
        }
        Ok(())
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

    /// Obtain the static metadata.
    fn metadata(&self) -> &'static tracing::Metadata<'static>;
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

    fn metadata(&self) -> &'static tracing::Metadata<'static> {
        tracing::span::Attributes::metadata(self)
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

    fn metadata(&self) -> &'static tracing::Metadata<'static> {
        tracing::Event::metadata(self)
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
    /// Record a new `Espan` that can be used afterwards.
    ///
    /// If `reuse_espan_id` is not empty, and matches `key_values`,
    /// `reuse_espan_id` will be returned instead.
    pub fn add_espan(
        &mut self,
        key_values: &[(&str, &str)],
        reuse_espan_id: Option<EspanId>,
    ) -> EspanId {
        if let Some(reuse_espan_id) = reuse_espan_id {
            if let Some(orig_espan) = self.get_espan(reuse_espan_id) {
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

        let metadata = None;
        let espan = Espan {
            meta,
            metadata,
            ref_count: 0,
        };

        let result = EspanId(self.espans.len() as u64 + self.espan_id_offset.0);
        self.espans.push(espan);
        result
    }

    /// Edit key-value data to an existing `Espan`.
    pub fn edit_espan<S1: ToString, S2: ToString>(
        &mut self,
        id: EspanId,
        key_values: impl IntoIterator<Item = (S1, S2)>,
    ) {
        if let Some(espan_index) = self.get_espan_index(id) {
            let espan = &mut self.espans[espan_index];
            for (key, value) in key_values {
                espan.meta.insert(
                    self.strings.id(key.to_string()),
                    self.strings.id(value.to_string()),
                );
            }
        }
    }

    /// Record a new "Action" about an [`EspanId`].
    pub fn add_action(&mut self, espan_id: EspanId, action: Action) -> bool {
        self.push_eventus(action, espan_id)
    }

    /// Mark `new_span_id` as following `old_span_id`.
    pub fn set_follows_from(&mut self, old_span_id: EspanId, new_span_id: EspanId) {
        // TODO: Implement this.
    }

    /// Return the process id that created this [`TracingData`].
    pub fn process_id(&self) -> u64 {
        self.default_process_id
    }
}

// -------- Merge multiple TracingData --------

impl TracingData {
    /// Merge multiple [`TracingData`]s into one [`TracingData`].
    pub fn merge(list: Vec<TracingData>) -> TracingData {
        if list.is_empty() {
            return TracingData::new();
        }

        let start = list.iter().map(|t| t.start).min().unwrap(); // list.len >= 1
        let relative_start = list.iter().map(|t| t.relative_start).min().unwrap();
        let test_clock_step = list.iter().map(|t| t.test_clock_step).min().unwrap();
        let default_process_id = unsafe { libc::getpid() } as u64;
        let default_thread_id = THREAD_ID.with(|thread_id| *thread_id);
        let mut strings = InternedStrings::default();
        let mut espans = Vec::with_capacity(list.iter().map(|t| t.espans.len()).sum());
        let mut eventus = Vec::with_capacity(list.iter().map(|t| t.eventus.len()).sum());
        let espan_id_offset = next_espan_id_offset();

        for data in list {
            let espan_offset = espans.len() as u64 + espan_id_offset.0;
            let time_offset = data.start.duration_since(start).unwrap().as_micros() as u64;

            // Add Espans (and strings as a side effect)
            for espan in data.espans.iter() {
                let meta = espan
                    .meta
                    .iter()
                    .map(|(key_id, value_id)| {
                        let key = data.strings.get(*key_id);
                        let value = data.strings.get(*value_id);
                        (strings.id(key), strings.id(value))
                    })
                    .collect();
                let metadata = None;
                espans.push(Espan {
                    meta,
                    metadata,
                    ref_count: espan.ref_count,
                });
            }

            // Add Eventus
            for Eventus {
                action,
                timestamp,
                espan_id,
                process_id,
                thread_id,
            } in data.eventus.iter()
            {
                let action = *action;
                let timestamp = RelativeTime(timestamp.0 + time_offset);
                let espan_id = EspanId(espan_id.0 + espan_offset - data.espan_id_offset.0);
                let process_id = match *process_id {
                    0 => data.default_process_id,
                    v => v,
                };
                let thread_id = match *thread_id {
                    0 => data.default_thread_id,
                    v => v,
                };
                eventus.push(Eventus {
                    action,
                    timestamp,
                    espan_id,
                    process_id,
                    thread_id,
                });
            }
        }

        // Sort by timestamp.
        eventus.sort_by(|e1, e2| e1.timestamp.cmp(&e2.timestamp));

        TracingData {
            start,
            id_map: HashMap::new(),
            strings,
            espans,
            eventus,
            espan_id_offset,
            default_process_id,
            default_thread_id,
            relative_start,
            callsite_entered: Default::default(),
            test_clock_step,
            max_span_ref_count: default_max_span_ref_count(),
        }
    }
}

// -------- Convert to Trace Event format (Chrome Trace) --------

/// Zero-copy `serde_json::Value` alternative.
#[derive(Serialize)]
#[serde(untagged)]
pub enum RefValue<'a> {
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
    ({ $( $k:ident : $v:expr_2021, )* }) => {{
        #[allow(unused_mut)]
        let mut obj = IndexMap::new();
        $( obj.insert(stringify!($k), object!($v)); )*
        $crate::model::RefValue::Map(obj)
    }};
    ($v: expr_2021) => { RefValue::from($v) };
}

impl TracingData {
    /// Write "Trace Event" format that can be viewed by Chrome "about:tracing".
    ///
    /// See [tracing](https://github.com/catapult-project/catapult/tree/master/tracing).
    pub fn write_trace_event_json(
        &self,
        out: &mut dyn io::Write,
        other_data: HashMap<String, String>,
    ) -> Result<(), serde_json::Error> {
        serde_json::to_writer(out, &self.trace_event(other_data))
    }

    /// Return a serializable object that if serialized in JSON, matches
    /// the "Trace Event" format.
    pub fn trace_event<'a>(
        &'a self,
        other_data: HashMap<String, String>,
    ) -> impl serde::Serialize + 'a {
        // FEATURE: "Trace Event" supports a lot of things. Features to consider:
        // - Handle async events (set "id" to espan_id, and use async phase names).
        // - Translate Espan::follower_ids to "Flow Events" (if follower_ids get used).
        // - Using "Metadata Events" to add names to threads.

        // Extract string from espan.meta.
        fn extract<'a>(
            strings: &'a InternedStrings,
            espan: &'a Espan,
            name: &str,
        ) -> Option<&'a str> {
            let meta = &espan.meta;
            if let Some((key_id, _)) = strings.0.get_full(name) {
                let key_id = StringId(key_id as u64);
                if let Some(value_id) = meta.get(&key_id) {
                    return Some(strings.get(*value_id));
                }
            }
            None
        }

        // Calculate JSON objects in a streaming way to reduce memory usage.
        let trace_event_iter = self.eventus.iter().map(move |eventus| {
            // EspanId recorded in eventus should be verified.
            let espan = self.get_espan(eventus.espan_id).unwrap();
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
            let pid = match eventus.process_id {
                0 => self.default_process_id,
                v => v,
            };
            let tid = match eventus.thread_id {
                0 => self.default_thread_id,
                v => v,
            };
            let mut obj = object!({
                name: extract(&self.strings, espan, "name").unwrap_or("(unnamed)"),
                cat: extract(&self.strings, espan, "cat").unwrap_or("default"),
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

        /// Top-level object used by the "Trace Event" format.
        #[allow(non_snake_case)]
        #[derive(Serialize)]
        pub struct Trace<'b, I: Iterator<Item = RefValue<'b>>> {
            #[serde(serialize_with = "serialize_iter")]
            traceEvents: RefCell<I>,
            displayTimeUnit: &'static str,
            otherData: HashMap<String, String>,
        }

        Trace {
            traceEvents: RefCell::new(trace_event_iter),
            displayTimeUnit: "ms",
            otherData: other_data,
        }
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

// -------- ASCII output --------

/// Options used to control behavior of writing ASCII graph.
#[derive(Default)]
#[non_exhaustive]
pub struct AsciiOptions {
    /// Hide a "Duration Span" if a span takes less than the specified
    /// microseconds.
    pub min_duration_micros_to_hide: u64,

    /// Hide a "Duration Span" if it is less than the specified
    /// percentage of the parent's duration.
    pub min_duration_parent_percentage_to_hide: u8,

    /// Show a "Duration Span" if it was hidden by the above rules
    /// but it is more than the specified percentage of the parent's
    /// duration. Not effective if it's 0.
    pub min_duration_parent_percentage_to_show: u8,
}

/// Spans that form a Tree. Internal used by write_ascii functions.
#[derive(Default)]
struct RawTreeSpan {
    // None: Root Span. Otherwise non-root span.
    espan_id: Option<EspanId>,
    start_time: u64,
    duration: u64,
    children: Vec<RawTreeSpanId>,
    call_count: usize,
    is_event: bool,
}
type RawTreeSpanId = usize;

impl RawTreeSpan {
    /// Whether the current [`RawTreeSpan`] covers another [`RawTreeSpan`] timestamp-wise.
    fn covers(&self, other: &RawTreeSpan) -> bool {
        if self.is_incomplete() {
            self.start_time <= other.start_time
        } else {
            self.end_time() >= other.end_time() && self.start_time <= other.start_time
        }
    }

    /// End time (inaccurate if this is a merged span, i.e. call_count > 1).
    fn end_time(&self) -> u64 {
        self.start_time + self.duration
    }

    /// Is this span considered interesting (should it be printed)?
    fn is_interesting(&self, opts: &AsciiOptions, parent: Option<&RawTreeSpan>) -> bool {
        if self.call_count == 0 {
            return false;
        }

        if let Some(parent) = parent {
            // Special case: Parent is root (which does not have a duration). Show the span.
            if parent.espan_id.is_none() {
                return true;
            }

            // "to_show" conditions
            if opts.min_duration_parent_percentage_to_show != 0 && parent.is_interesting(opts, None)
            {
                if self.duration
                    >= (parent.duration * opts.min_duration_parent_percentage_to_show as u64) / 100
                {
                    return true;
                }
            }

            // "to_hide" conditions
            if self.duration
                < (parent.duration * opts.min_duration_parent_percentage_to_hide as u64) / 100
            {
                return false;
            }
        }

        // "to_hide" conditions
        self.duration >= opts.min_duration_micros_to_hide
    }

    /// A very long, impractical `duration` that indicates an incomplete span
    /// that has started but not ended.
    const fn incomplete_duration() -> u64 {
        1 << 63
    }

    fn is_incomplete(&self) -> bool {
        self.duration >= Self::incomplete_duration()
    }
}

struct Row {
    columns: Vec<String>,
}

struct Rows {
    rows: Vec<Row>,
    column_alignments: Vec<Alignment>,
    column_min_widths: Vec<usize>,
    column_max_widths: Vec<usize>,
}

enum Alignment {
    Left,
    Right,
}

impl fmt::Display for Rows {
    /// Render rows with aligned columns.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let column_count = self.rows.iter().map(|r| r.columns.len()).max().unwrap_or(0);
        let column_widths: Vec<usize> = (0..column_count)
            .map(|i| {
                self.rows
                    .iter()
                    .map(|r| r.columns.get(i).map_or(0, |s| s.len()))
                    .max()
                    .unwrap_or(0)
                    .max(self.column_min_widths.get(i).cloned().unwrap_or(0))
                    .min(self.column_max_widths.get(i).cloned().unwrap_or(usize::MAX))
            })
            .collect();
        for row in self.rows.iter() {
            for (i, cell) in row.columns.iter().enumerate() {
                let width = column_widths[i];
                let pad = " ".repeat(width.max(cell.len()) - cell.len());
                let mut content = match self.column_alignments.get(i).unwrap_or(&Alignment::Left) {
                    Alignment::Left => cell.clone() + &pad,
                    Alignment::Right => pad + cell,
                };
                if i + 1 == row.columns.len() {
                    content = content.trim_end().to_string();
                };
                if !content.is_empty() {
                    if i != 0 {
                        // Separator
                        write!(f, " ")?;
                    }
                    write!(f, "{}", content)?;
                }
            }
            write!(f, "\n")?;
        }
        Ok(())
    }
}

impl TracingData {
    /// Generate ASCII output.
    pub fn ascii(&self, opts: &AsciiOptions) -> String {
        let eventus_by_pid_tid = self.eventus_group_by_pid_tid();

        // Handle (pid, tid) one by one.
        let mut out = String::new();
        for ((pid, tid), eventus) in eventus_by_pid_tid.iter() {
            if self.test_clock_step > 0 {
                out += "Process _ Thread _ Start Time _:\n"
            } else {
                out += &format!(
                    "Process {} Thread {} Start Time {:?}:\n",
                    pid, tid, self.start,
                )
            };
            out += &self.ascii_single_thread(eventus, opts);
            out += "\n";
        }
        out
    }

    fn eventus_group_by_pid_tid(&self) -> IndexMap<(u64, u64), Vec<&Eventus>> {
        // Group by (pid, tid).
        let mut eventus_by_pid_tid = IndexMap::<(u64, u64), Vec<&Eventus>>::new();
        for e in self.eventus.iter() {
            let pid = match e.process_id {
                0 => self.default_process_id,
                v => v,
            };
            let tid = match e.thread_id {
                0 => self.default_thread_id,
                v => v,
            };
            eventus_by_pid_tid.entry((pid, tid)).or_default().push(e);
        }
        eventus_by_pid_tid
    }

    /// Generate ASCII call graph for a single thread.
    fn ascii_single_thread(&self, eventus_list: &[&Eventus], opts: &AsciiOptions) -> String {
        let tree_spans = self.build_tree_spans(eventus_list);
        let tree_spans = self.merge_tree_spans(tree_spans, opts);
        let rows = self.render_tree_spans(tree_spans, opts);
        rows.to_string()
    }

    /// Scan `Eventus` list to reconstruct the call graph.
    fn build_tree_spans(&self, eventus_list: &[&Eventus]) -> Vec<RawTreeSpan> {
        // For example, eventus_list like:
        // (`+`: Enter, `-`: Exit, Number: SpanId)
        //
        //   +1 +1 -1 +2 +3 -3 -2 -1 +3 -3
        //
        // forms the following tree:
        //
        //   <root>
        //    |- span 1
        //    |   |- span 1
        //    |   |- span 2
        //    |       |- span 3
        //    |- span 3
        //
        // It's possible to replace "2" and "3" with "1" and the shape of the
        // tree should remain unchanged.

        // Build up some indexes to help analyze spans.
        //
        // `Eid` is the index in `eventus_list` passed in.
        type Eid = usize;

        /// Find out the matched ExitSpan for an EnterSpan.
        ///
        /// Note: a same function can reuse a same SpanId and be called
        /// recursively.
        #[derive(Default)]
        struct EnterExitMatcher {
            /// EnterSpan actions that are not yet matched.
            unmatched: Vec<Eid>,

            /// EnterSpan Eid -> ExitSpan Eid.
            matched: IndexMap<Eid, Eid>,
        }

        impl EnterExitMatcher {
            /// Attempt to find the Eid of ExitSpan matching an EnterSpan.
            fn find_matched_exit_eid(&self, enter_eid: Eid) -> Option<Eid> {
                self.matched.get(&enter_eid).cloned()
            }

            /// Process a [`Eventus`]. Must be called in timestamp order.
            fn process(&mut self, action: Action, eid: Eid) {
                match action {
                    Action::EnterSpan => {
                        self.unmatched.push(eid);
                    }
                    Action::ExitSpan => {
                        if let Some(enter_eid) = self.unmatched.pop() {
                            self.matched.insert(enter_eid, eid);
                        }
                    }
                    Action::Event => {}
                }
            }
        }

        let mut enter_exit_matchers = IndexMap::<EspanId, EnterExitMatcher>::new();
        for (eid, e) in eventus_list.iter().enumerate() {
            enter_exit_matchers
                .entry(e.espan_id)
                .or_default()
                .process(e.action, eid);
        }

        // To make the Rust borrowck happy, use another Vec for all TreeSpans,
        // and refer to other TreeSpans using Vec indexes.
        // A dummy root is created, so the root is unique. That makes it a bit
        // easier to handle.
        let mut tree_spans = vec![RawTreeSpan::default()];

        // Keep a stack of TreeSpans to figure out parents.
        let mut stack: Vec<RawTreeSpanId> = vec![0];

        // Append a span to the list. Attach it to the "correct" parent span.
        let mut append = |tree_span| {
            // Find a suitable parent span. Pop parent spans
            // if this span does not fit in it.
            //
            // But, always keep the (dummy) root parent span.
            let parent_id = loop {
                let parent_id = *stack.last().unwrap();
                let parent = &tree_spans[parent_id];
                if parent.covers(&tree_span) {
                    break parent_id;
                } else if stack.len() == 1 {
                    // Use the root span as parent.
                    break 0;
                } else {
                    stack.pop();
                }
            };

            // Record the new RawTreeSpan and record parent-child
            // relationship.
            let id = tree_spans.len();
            tree_spans.push(tree_span);
            stack.push(id);
            tree_spans[parent_id].children.push(id);
        };

        // Scan through the `Eventus` list. For any `EnterSpan` action, try
        // to find the matching `ExitSpan` action and create a span with a
        // proper parent.
        for (eid, e) in eventus_list.iter().enumerate() {
            let span_id = e.espan_id;
            match e.action {
                Action::EnterSpan => {
                    // Find the matching ExitSpan.
                    // The [`EnterExitMatcher`] should always exist.
                    let matcher = &enter_exit_matchers[&span_id];
                    let tree_span = if let Some(end_eid) = matcher.find_matched_exit_eid(eid) {
                        // `end_eid` points to the matched ExitSpan.
                        let end = eventus_list[end_eid];
                        // `eventus_list` should be sorted in time.
                        // So this is guaranteed.
                        assert!(end_eid >= eid);
                        assert!(end.timestamp >= e.timestamp);

                        RawTreeSpan {
                            espan_id: Some(span_id),
                            start_time: e.timestamp.0,
                            duration: end.timestamp.0 - e.timestamp.0,
                            children: Vec::new(),
                            call_count: 1,
                            is_event: false,
                        }
                    } else {
                        // No matched ExitSpan. Still create a RawTreeSpan
                        // so it shows up.
                        RawTreeSpan {
                            espan_id: Some(span_id),
                            start_time: e.timestamp.0,
                            duration: RawTreeSpan::incomplete_duration(),
                            children: Vec::new(),
                            call_count: 1,
                            is_event: false,
                        }
                    };

                    append(tree_span);
                }
                Action::ExitSpan => {
                    // Handled in EnterSpan. Therefore do nothing here.
                }
                Action::Event => {
                    // This is similar to EnterSpan + ExitSpan immediately.
                    let tree_span = RawTreeSpan {
                        espan_id: Some(span_id),
                        start_time: e.timestamp.0,
                        duration: 0,
                        children: Vec::new(),
                        call_count: 1,
                        is_event: true,
                    };
                    append(tree_span);
                }
            }
        }

        tree_spans
    }

    /// Merge multiple similar spans into one larger span.
    fn merge_tree_spans(
        &self,
        tree_spans: Vec<RawTreeSpan>,
        opts: &AsciiOptions,
    ) -> Vec<RawTreeSpan> {
        // For example,
        //
        //   <root>
        //    |- span 1
        //    |   |- span 2
        //    |   |- span 3
        //    |   |- span 2
        //    |   |- span 3
        //    |   |- span 2
        //    |- span 2
        //
        // might be rewritten into:
        //
        //   <root>
        //    |- span 1
        //    |   |- span 2 (x 3)
        //    |   |- span 3 (x 2)
        //    |- span 2

        struct Context<'a> {
            this: &'a TracingData,
            opts: &'a AsciiOptions,
            tree_spans: Vec<RawTreeSpan>,
        }

        /// Check children of tree_spans[id] recursively.
        fn visit(ctx: &mut Context, id: usize) {
            type RawTreeSpanId = usize;
            // Treat spans with the same metadata as same spans.
            // So different EspanIds can still be merged.
            let mut meta_to_id = IndexMap::<Vec<(StringId, StringId)>, RawTreeSpanId>::new();
            let child_ids: Vec<RawTreeSpanId> = ctx.tree_spans[id].children.to_vec();
            for child_id in child_ids {
                // Do not try to merge this child span if itself, or any of the
                // grand children is interesting. But some of the grand children
                // might be merged. So go visit them.
                if ctx.tree_spans[child_id].is_interesting(ctx.opts, Some(&ctx.tree_spans[id])) || {
                    ctx.tree_spans[child_id].children.iter().any(|&id| {
                        ctx.tree_spans[id].is_interesting(ctx.opts, Some(&ctx.tree_spans[child_id]))
                    })
                } {
                    visit(ctx, child_id);
                    continue;
                }

                // Otherwise, attempt to merge the child span.
                if let Some(espan_id) = ctx.tree_spans[child_id].espan_id {
                    if let Some(espan) = ctx.this.get_espan(espan_id) {
                        let meta: Vec<(StringId, StringId)> =
                            espan.meta.iter().map(|(&k, &v)| (k, v)).collect();
                        let existing_child_id: RawTreeSpanId =
                            *meta_to_id.entry(meta).or_insert(child_id);
                        if existing_child_id != child_id {
                            let duration = ctx.tree_spans[child_id].duration;
                            assert_eq!(ctx.tree_spans[child_id].call_count, 1);
                            ctx.tree_spans[child_id].call_count -= 1;
                            let merged = &mut ctx.tree_spans[existing_child_id];
                            merged.call_count += 1;
                            merged.duration += duration;
                        }
                    }
                }
            }
        }

        let mut context = Context {
            this: self,
            opts,
            tree_spans,
        };

        visit(&mut context, 0);

        context.tree_spans
    }

    /// Render one `RawTreeSpan` into `Rows`.
    fn render_tree_spans(&self, tree_spans: Vec<RawTreeSpan>, opts: &AsciiOptions) -> Rows {
        struct Context<'a> {
            this: &'a TracingData,
            opts: &'a AsciiOptions,
            tree_spans: Vec<RawTreeSpan>,
            rows: Vec<Row>,
        }

        /// Extract value from espan.meta.
        fn extract<'a>(ctx: &'a Context, espan: &'a Espan, name: &'a str) -> &'a str {
            let meta = &espan.meta;
            if let Some((key_id, _)) = ctx.this.strings.0.get_full(name) {
                let key_id = StringId(key_id as u64);
                if let Some(value_id) = meta.get(&key_id) {
                    return ctx.this.strings.get(*value_id);
                }
            }
            ""
        }

        /// Render RawTreeSpan to rows.
        fn render_span(ctx: &mut Context, id: usize, mut indent: usize, first_row_ch: char) {
            let tree_span = &ctx.tree_spans[id];
            if let Some(espan_id) = tree_span.espan_id {
                let this = ctx.this;
                let strings = &this.strings;
                let espan = match ctx.this.get_espan(espan_id) {
                    Some(espan) => espan,
                    None => return,
                };
                let name = extract(ctx, espan, "name");
                let source_location = {
                    let module_path = extract(ctx, espan, "module_path");
                    let line = extract(ctx, espan, "line");
                    if module_path.is_empty() {
                        let cat = extract(ctx, espan, "cat");
                        if cat.is_empty() {
                            String::new()
                        } else {
                            format!("({})", cat)
                        }
                    } else if line.is_empty() {
                        module_path.to_string()
                    } else {
                        format!("{} line {}", module_path, line)
                    }
                };
                let start = tree_span.start_time / 1000;
                let duration = if tree_span.is_event {
                    "0".to_string()
                } else if tree_span.is_incomplete() {
                    format!(
                        "?{}",
                        (ctx.this.now_micros().0 - tree_span.start_time) / 1000,
                    )
                } else {
                    // Use milliseconds. This is consistent with traceprof.
                    format!("+{}", tree_span.duration / 1000)
                };
                let mut call_count = if tree_span.call_count > 1 {
                    format!(" ({} times)", tree_span.call_count)
                } else {
                    assert!(tree_span.call_count > 0);
                    String::new()
                };
                if espan.ref_count >= ctx.this.max_span_ref_count {
                    call_count += " (truncated)"
                }

                let first_row = Row {
                    columns: vec![
                        start.to_string(),
                        duration,
                        format!(
                            "{}{} {}{}",
                            " ".repeat(indent),
                            first_row_ch,
                            name,
                            call_count
                        ),
                        source_location,
                    ],
                };
                ctx.rows.push(first_row);

                // Extra metadata (other than name, module_path and line)
                let extra_meta: Vec<(&str, &str)> = espan
                    .meta
                    .iter()
                    .map(|(&key, &value)| (strings.get(key), strings.get(value)))
                    .filter(|(key, value)| {
                        *key != "name" && *key != "module_path" && *key != "line" && *key != "cat"
                    })
                    .collect();
                if first_row_ch == '\\' {
                    indent += 1;
                }
                for (i, (key, value)) in extra_meta.iter().enumerate() {
                    let value = if value.len() > 32 {
                        format!("{}...", &value[..30])
                    } else {
                        value.to_string()
                    };
                    let row = Row {
                        columns: vec![
                            String::new(),
                            String::new(),
                            format!("{}| - {} = {}", " ".repeat(indent), key, value),
                            format!(":"),
                        ],
                    };
                    ctx.rows.push(row);
                }
            }
        }

        /// Visit a span and its children recursively.
        fn visit(ctx: &mut Context, id: usize, indent: usize, ch: char) {
            // Print out this span.
            render_span(ctx, id, indent, ch);

            // Figure out children to visit.
            let child_ids: Vec<usize> = ctx.tree_spans[id]
                .children
                .iter()
                .cloned()
                .filter(|&id| {
                    ctx.tree_spans[id].is_interesting(ctx.opts, Some(&ctx.tree_spans[id]))
                })
                .collect();

            // Preserve a straight line if there is only one child:
            //
            //   | foo ('bar' is the only child)
            //   | bar  <- case 1
            //
            // Increase indent if there are multi-children (case 2),
            // or it's already not a straight line (case 3):
            //
            //   | foo ('bar1' and 'bar2' are children)
            //    \ bar1     <- case 2
            //     | bar1.1  <- case 3
            //     | bar1.2  <- case 1
            //    \ bar2     <- case 2
            //     \ bar2.1  <- case 2
            //     \ bar2.2  <- case 2
            //
            let (indent, ch) = if child_ids.len() >= 2 {
                // case 2
                (indent + 1, '\\')
            } else if ch == '\\' {
                // case 3
                (indent + 1, '|')
            } else {
                // case 1
                (indent, '|')
            };

            for id in child_ids {
                visit(ctx, id, indent, ch)
            }
        }

        let mut context = Context {
            this: self,
            opts,
            tree_spans,
            rows: vec![Row {
                columns: ["Start", "Dur.ms", "| Name", "Source"]
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            }],
        };

        // Visit the root RawTreeSpan.
        visit(&mut context, 0, 0, '|');

        let column_alignments = vec![
            Alignment::Right, // start time
            Alignment::Right, // duration
            Alignment::Left,  // graph, name
            Alignment::Left,  // module, line number
        ];

        let column_min_widths = vec![4, 4, 20, 0];
        let column_max_widths = vec![20, 20, 80, 80];

        Rows {
            rows: context.rows,
            column_alignments,
            column_min_widths,
            column_max_widths,
        }
    }
}

// -------- Serde-serializable Spans Output --------

#[derive(Serialize)]
pub struct TreeSpan<S: Eq + Hash> {
    #[serde(flatten)]
    pub meta: IndexMap<S, S>,
    pub start: u64,
    pub duration: Option<u64>,
    pub children: Vec<RawTreeSpanId>,
    pub is_event: bool,
}

impl<'a, S: From<&'a str> + Eq + Hash + 'a> TreeSpan<S> {
    fn from_tree_span(data: &'a TracingData, span: RawTreeSpan) -> TreeSpan<S> {
        let meta = match span.espan_id {
            None => Default::default(),
            Some(espan_id) => {
                if let Some(espan) = data.get_espan(espan_id) {
                    espan
                        .meta
                        .iter()
                        .map(|(k, v)| {
                            (S::from(data.strings.get(*k)), S::from(data.strings.get(*v)))
                        })
                        .collect()
                } else {
                    Default::default()
                }
            }
        };
        TreeSpan {
            meta,
            start: span.start_time,
            duration: if span.is_incomplete() {
                None
            } else {
                Some(span.duration)
            },
            children: span.children,
            is_event: span.is_event,
        }
    }
}

/// Contains parsed spans for a single thread.
/// Unlike flat span events (enter, exit), the spans form a tree.
#[derive(Serialize)]
#[serde(transparent)]
pub struct TreeSpans<S: Eq + Hash> {
    pub spans: Vec<TreeSpan<S>>,
}

impl<S: Eq + Hash> Deref for TreeSpans<S> {
    type Target = [TreeSpan<S>];
    fn deref(&self) -> &Self::Target {
        &self.spans
    }
}

impl TracingData {
    /// Calculate `RawTreeSpan`s for each `(pid, tid)` pair.
    /// The result is serializable and can be useful for assertions in tests.
    pub fn tree_spans<'a, S: From<&'a str> + Eq + Hash + 'a>(
        &'a self,
    ) -> IndexMap<(u64, u64), TreeSpans<S>> {
        let eventus_by_pid_tid = self.eventus_group_by_pid_tid();

        eventus_by_pid_tid
            .iter()
            .map(|(&pid_tid, eventus_list)| {
                let tree_spans = self.build_tree_spans(eventus_list);
                let tree_spans_with_meta = tree_spans
                    .into_iter()
                    .map(|t| TreeSpan::from_tree_span(self, t))
                    .collect();
                (
                    pid_tid,
                    TreeSpans {
                        spans: tree_spans_with_meta,
                    },
                )
            })
            .collect()
    }
}

// -------- Queries on TreeSpans --------

/// Provides iteration support on TreeSpans.
pub struct TreeWalker<'a, S: Eq + Hash> {
    spans: &'a TreeSpans<S>,

    /// (span index, child index) in the walker tree.
    stack: Vec<(usize, usize)>,

    /// Filter function. If returns `false`, skip spans in `step()`.  The
    /// function can call `TreeWalker::step_out()` once to skip visiting
    /// subtrees.
    filter: Option<Rc<dyn Fn(&mut TreeWalker<S>, &TreeSpan<S>) -> bool>>,
}

impl<'a, S: Eq + Hash> TreeWalker<'a, S> {
    /// Visit the next span in DFS order.
    ///
    /// This is similar to "step/step into" in a debugger.
    pub fn step(&mut self) -> Option<&'a TreeSpan<S>> {
        'visit_next: loop {
            match self.stack.pop() {
                Some((span_id, child_id)) => {
                    let span = &self.spans.spans[span_id];
                    match span.children.get(child_id) {
                        Some(&child_span_id) => {
                            self.stack.push((span_id, child_id + 1));
                            self.stack.push((child_span_id, 0));
                            let span = &self.spans.spans[child_span_id];
                            if let Some(filter) = self.filter.clone() {
                                if !filter(self, span) {
                                    continue 'visit_next;
                                }
                            }
                            return Some(span);
                        }
                        None => {
                            continue 'visit_next;
                        }
                    }
                }
                None => return None,
            }
        }
    }

    /// Apply a filter function `(walker, span) -> bool`. `step` will skip
    /// returning spans if the filter function returns `false`. The filter
    /// function can also call `walker.step_out()` to skip visiting a subtree.
    pub fn filter(
        mut self,
        func: impl Fn(&mut TreeWalker<S>, &TreeSpan<S>) -> bool + 'static,
    ) -> Self {
        assert!(self.filter.is_none(), "cannot apply multiple filters");
        self.filter = Some(Rc::new(func));
        self
    }

    /// Skip to the parent span. Call this immediately after `next` to avoid
    /// visiting children of the span just returned by `next`.
    ///
    /// This is similar to "step out" in a debugger.
    pub fn step_out(&mut self) {
        let _ = self.stack.pop();
    }
}

impl<'a, S: Eq + Hash> Iterator for TreeWalker<'a, S> {
    type Item = &'a TreeSpan<S>;

    fn next(&mut self) -> Option<Self::Item> {
        self.step()
    }
}

impl<S: Eq + Hash> TreeSpans<S> {
    /// Walk through the tree.
    pub fn walk(&self) -> TreeWalker<S> {
        TreeWalker {
            spans: self,
            stack: vec![(0, 0)],
            filter: None,
        }
    }
}

impl<S: Eq + Hash> TreeSpan<S> {
    pub fn start_millis(&self) -> u64 {
        self.start / 1000
    }

    pub fn duration_millis(&self) -> Option<u64> {
        self.duration.map(|d| d / 1000)
    }
}

// -------- Tests --------

#[cfg(test)]
mod tests {
    use super::*;

    impl TracingData {
        /// Similar to `new`, but use dummy clocks for testing purpose.
        pub fn new_for_test() -> TracingData {
            let mut data = Self::new();
            data.test_clock_step = 2000; // 2 milliseconds
            data
        }
    }

    fn meta<'a>(name: &'a str, module_path: &'a str, line: &'a str) -> Vec<(&'a str, &'a str)> {
        vec![("name", name), ("module_path", module_path), ("line", line)]
    }

    #[test]
    fn test_empty() {
        let data = TracingData::new_for_test();
        assert_eq!(data.ascii(&Default::default()), "");
    }

    #[test]
    fn test_reusable_span() {
        let mut data = TracingData::new_for_test();
        let span_id1 = data.add_espan(&meta("foo", "a.py", "10"), Some(EspanId(0)));
        let span_id2 = data.add_espan(&meta("foo", "a.py", "10"), Some(span_id1)); // reuse
        let span_id3 = data.add_espan(&meta("foo", "a.py", "20"), Some(span_id1)); // not reuse
        let span_id4 = data.add_espan(&meta("foo", "a.py", "10"), Some(span_id3)); // not reuse
        assert_eq!(span_id1, span_id2);
        assert_ne!(span_id1, span_id3);
        assert_ne!(span_id1, span_id4);
    }

    #[test]
    fn test_extra_meta() {
        let mut meta1 = meta("eval", "eval.py", "10");
        meta1.push(("expression", "['+', 1, 2]"));
        meta1.push(("result", "3"));
        let meta2 = meta("refresh", "view.py", "90");

        let mut data = TracingData::new_for_test();
        let span_id1 = data.add_espan(&meta1, None);
        let span_id2 = data.add_espan(&meta2, None);
        data.add_action(span_id1, Action::EnterSpan);
        data.add_action(span_id2, Action::EnterSpan);
        data.add_action(span_id2, Action::ExitSpan);
        data.add_action(span_id1, Action::ExitSpan);
        assert_eq!(
            data.ascii(&Default::default()),
            r#"Process _ Thread _ Start Time _:
Start Dur.ms | Name                       Source
    2     +6 | eval                       eval.py line 10
             | - expression = ['+', 1, 2] :
             | - result = 3               :
    4     +2 | refresh                    view.py line 90

"#
        );

        let mut data = TracingData::new_for_test();
        let span_id1 = data.add_espan(&meta1, None);
        let span_id2 = data.add_espan(&meta2, None);
        data.add_action(span_id2, Action::EnterSpan);
        data.add_action(span_id2, Action::EnterSpan);
        data.add_action(span_id2, Action::ExitSpan);
        data.add_action(span_id1, Action::EnterSpan);
        data.add_action(span_id2, Action::EnterSpan);
        data.add_action(span_id2, Action::ExitSpan);
        data.add_action(span_id1, Action::ExitSpan);
        data.add_action(span_id2, Action::ExitSpan);
        assert_eq!(
            data.ascii(&Default::default()),
            r#"Process _ Thread _ Start Time _:
Start Dur.ms | Name                         Source
    2    +14 | refresh                      view.py line 90
    4     +2  \ refresh                     view.py line 90
    8     +6  \ eval                        eval.py line 10
               | - expression = ['+', 1, 2] :
               | - result = 3               :
   10     +2   | refresh                    view.py line 90

"#
        );
    }

    #[test]
    fn test_tree_span_serialize() {
        let mut data = TracingData::new_for_test();
        let span_id = data.add_espan(&meta("foo", "a.py", "10"), None);
        data.add_action(span_id, Action::EnterSpan);
        data.add_action(span_id, Action::ExitSpan);
        let tree_spans: Vec<_> = data
            .tree_spans::<&str>()
            .into_iter()
            .next()
            .unwrap()
            .1
            .spans;
        assert_eq!(
            serde_json::to_string_pretty(&tree_spans).unwrap(),
            r#"[
  {
    "start": 0,
    "duration": 0,
    "children": [
      1
    ],
    "is_event": false
  },
  {
    "name": "foo",
    "module_path": "a.py",
    "line": "10",
    "start": 2000,
    "duration": 2000,
    "children": [],
    "is_event": false
  }
]"#
        );
    }

    #[test]
    fn test_recursive_single_span() {
        let mut data = TracingData::new_for_test();
        let span_id = data.add_espan(&meta("foo", "a.py", "10"), None);
        data.add_action(span_id, Action::EnterSpan); // span
        data.add_action(span_id, Action::EnterSpan); // +- span
        data.add_action(span_id, Action::EnterSpan); // |  +- span
        data.add_action(span_id, Action::EnterSpan); // |     +- span
        data.add_action(span_id, Action::EnterSpan); // |        +- span
        data.add_action(span_id, Action::ExitSpan); //  |
        data.add_action(span_id, Action::ExitSpan); //  |
        data.add_action(span_id, Action::ExitSpan); //  |
        data.add_action(span_id, Action::ExitSpan); //  |
        data.add_action(span_id, Action::EnterSpan); // +- span
        data.add_action(span_id, Action::ExitSpan); //  |
        data.add_action(span_id, Action::EnterSpan); // +- span
        data.add_action(span_id, Action::EnterSpan); // |  +- span
        data.add_action(span_id, Action::ExitSpan); //  |  |
        data.add_action(span_id, Action::EnterSpan); // |  +- span
        data.add_action(span_id, Action::ExitSpan); //  |
        data.add_action(span_id, Action::ExitSpan);

        assert_eq!(
            data.ascii(&Default::default()),
            r#"Process _ Thread _ Start Time _:
Start Dur.ms | Name               Source
    2    ?34 | foo                a.py line 10
    4    +14  \ foo               a.py line 10
    6    +10   | foo              a.py line 10
    8     +6   | foo              a.py line 10
   10     +2   | foo              a.py line 10
   20     +2  \ foo               a.py line 10
   24    +10  \ foo               a.py line 10
   26     +2   \ foo              a.py line 10
   30     +2   \ foo              a.py line 10

"#
        );

        let opts = AsciiOptions {
            min_duration_micros_to_hide: 4000,
            ..Default::default()
        };
        assert_eq!(
            data.ascii(&opts),
            r#"Process _ Thread _ Start Time _:
Start Dur.ms | Name               Source
    2    ?34 | foo                a.py line 10
    4    +14  \ foo               a.py line 10
    6    +10   | foo              a.py line 10
    8     +6   | foo              a.py line 10
   24    +10  \ foo               a.py line 10
   26     +4   | foo (2 times)    a.py line 10

"#
        );
    }

    #[test]
    fn test_mixed_span_events() {
        let mut data = TracingData::new_for_test();
        let span_id1 = data.add_espan(&meta("foo", "a.py", "10"), None);
        let span_id2 = data.add_espan(&meta("bar", "a.py", "20"), None);

        data.add_action(span_id1, Action::EnterSpan);
        data.add_action(span_id2, Action::Event);
        data.add_action(span_id2, Action::Event);
        data.add_action(span_id1, Action::Event);
        data.add_action(span_id1, Action::ExitSpan);

        let opts = AsciiOptions::default();
        assert_eq!(
            data.ascii(&opts),
            r#"Process _ Thread _ Start Time _:
Start Dur.ms | Name               Source
    2     +8 | foo                a.py line 10
    4      0  \ bar               a.py line 20
    6      0  \ bar               a.py line 20
    8      0  \ foo               a.py line 10

"#
        );
    }

    #[test]
    fn test_merged_spans() {
        let mut data = TracingData::new_for_test();
        let span_id1 = data.add_espan(&meta("foo", "a.py", "10"), None);
        let span_id2 = data.add_espan(&meta("bar", "a.py", "20"), None);
        let opts = AsciiOptions {
            min_duration_micros_to_hide: 3000,
            ..Default::default()
        };

        data.add_action(span_id1, Action::EnterSpan);
        // Those spans should be merged.
        for _ in 0..10000 {
            data.add_action(span_id2, Action::EnterSpan);
            data.add_action(span_id2, Action::ExitSpan);
        }
        // This should not be merged - it has children that take longer than 3ms.
        data.add_action(span_id2, Action::EnterSpan);
        data.add_action(span_id1, Action::EnterSpan);
        data.add_action(span_id1, Action::EnterSpan);
        data.add_action(span_id1, Action::ExitSpan);
        data.add_action(span_id1, Action::ExitSpan);
        data.add_action(span_id2, Action::ExitSpan);
        data.add_action(span_id1, Action::ExitSpan);

        assert_eq!(
            data.ascii(&opts),
            r#"Process _ Thread _ Start Time _:
Start Dur.ms | Name                          Source
    2  +4010 | foo                           a.py line 10
    4  +2000  \ bar (1000 times) (truncated) a.py line 20
 4004     +6  \ foo                          a.py line 10

"#
        );
    }

    #[test]
    fn test_max_span_ref_count() {
        let mut data = TracingData::new_for_test();
        data.set_max_span_ref_count(2000);

        let span_id1 = data.add_espan(&meta("foo", "a.py", "10"), None);
        let span_id2 = data.add_espan(&meta("bar", "a.py", "20"), None);
        data.add_action(span_id1, Action::EnterSpan);
        for _ in 0..5000 {
            data.add_action(span_id2, Action::EnterSpan);
            data.add_action(span_id2, Action::ExitSpan);
        }
        data.add_action(span_id1, Action::ExitSpan);

        let opts = AsciiOptions {
            min_duration_micros_to_hide: 3000,
            ..Default::default()
        };
        assert_eq!(
            data.ascii(&opts),
            r#"Process _ Thread _ Start Time _:
Start Dur.ms | Name                         Source
    2  +8002 | foo                          a.py line 10
    4  +4000 | bar (2000 times) (truncated) a.py line 20

"#
        );
    }

    #[test]
    fn test_incomplete_spans() {
        let mut data = TracingData::new_for_test();
        let span_id1 = data.add_espan(&meta("foo", "a.py", "10"), None);
        let span_id2 = data.add_espan(&meta("bar", "a.py", "20"), None);

        data.add_action(span_id1, Action::EnterSpan); // incomplete
        data.add_action(span_id1, Action::EnterSpan);
        data.add_action(span_id2, Action::EnterSpan);
        data.add_action(span_id2, Action::ExitSpan);
        data.add_action(span_id1, Action::ExitSpan);
        data.add_action(span_id2, Action::EnterSpan); // incomplete
        data.add_action(span_id1, Action::EnterSpan);
        data.add_action(span_id2, Action::EnterSpan);
        data.add_action(span_id2, Action::ExitSpan);
        data.add_action(span_id1, Action::ExitSpan);
        data.add_action(span_id1, Action::EnterSpan); // incomplete
        data.add_action(span_id1, Action::EnterSpan); // incomplete
        data.add_action(span_id1, Action::EnterSpan); // incomplete

        assert_eq!(
            data.ascii(&Default::default()),
            r#"Process _ Thread _ Start Time _:
Start Dur.ms | Name               Source
    2    ?26 | foo                a.py line 10
    4     +6  \ foo               a.py line 10
    6     +2   | bar              a.py line 20
   12    ?16  \ bar               a.py line 20
   14     +6   \ foo              a.py line 10
   16     +2    | bar             a.py line 20
   22     ?6   \ foo              a.py line 10
   24     ?4    | foo             a.py line 10
   26     ?2    | foo             a.py line 10

"#
        );
    }

    #[test]
    fn test_tree_span_filter() {
        let mut data = TracingData::new_for_test();
        let span_id1 = data.add_espan(&meta("foo", "a.py", "10"), None);
        let span_id2 = data.add_espan(&meta("bar", "a.py", "20"), None);

        data.add_action(span_id2, Action::EnterSpan);
        data.add_action(span_id2, Action::EnterSpan);
        data.add_action(span_id2, Action::ExitSpan);
        data.add_action(span_id1, Action::EnterSpan);
        data.add_action(span_id2, Action::EnterSpan);
        data.add_action(span_id1, Action::EnterSpan);
        data.add_action(span_id1, Action::ExitSpan);
        data.add_action(span_id2, Action::ExitSpan);
        data.add_action(span_id1, Action::ExitSpan);
        data.add_action(span_id2, Action::ExitSpan);

        assert_eq!(
            data.ascii(&Default::default()),
            r#"Process _ Thread _ Start Time _:
Start Dur.ms | Name               Source
    2    +18 | bar                a.py line 20
    4     +2  \ bar               a.py line 20
    8    +10  \ foo               a.py line 10
   10     +6   | bar              a.py line 20
   12     +2   | foo              a.py line 10

"#
        );

        let render = |spans: TreeWalker<&str>| -> String {
            spans
                .map(|s| format!("{}@{}", s.meta["name"], s.start / 1000))
                .collect::<Vec<_>>()
                .join(" ")
        };

        let tree = data.tree_spans::<&str>();
        let tree: &TreeSpans<&str> = tree.iter().next().unwrap().1;

        // Walk through every span in DFS order.
        assert_eq!(render(tree.walk()), "bar@2 bar@4 foo@8 bar@10 foo@12");

        // Filter: select "foo".
        assert_eq!(
            render(tree.walk().filter(|_, s| s.meta["name"] == "foo")),
            "foo@8 foo@12"
        );

        // Filter: select "foo" and skips subtrees.
        assert_eq!(
            render(tree.walk().filter(|w, s| {
                let result = s.meta["name"] == "foo";
                if result {
                    w.step_out();
                }
                result
            })),
            "foo@8"
        );
    }

    #[test]
    fn test_column_widths() {
        let mut data = TracingData::new_for_test();
        let span_id1 = data.add_espan(
            &meta(&str::repeat("very long text ", 10), "a.py", "10"),
            None,
        );
        let span_id2 = data.add_espan(&meta("bar", "a.py", "20"), None);
        data.add_action(span_id1, Action::EnterSpan);
        data.add_action(span_id2, Action::EnterSpan);
        data.add_action(span_id2, Action::EnterSpan);
        data.add_action(span_id2, Action::ExitSpan);
        data.add_action(span_id2, Action::ExitSpan);
        data.add_action(span_id2, Action::EnterSpan);
        data.add_action(span_id2, Action::ExitSpan);
        data.add_action(span_id1, Action::ExitSpan);

        assert_eq!(
            data.ascii(&Default::default()),
            r#"Process _ Thread _ Start Time _:
Start Dur.ms | Name                                                                           Source
    2    +14 | very long text very long text very long text very long text very long text very long text very long text very long text very long text very long text  a.py line 10
    4     +6  \ bar                                                                           a.py line 20
    6     +2   | bar                                                                          a.py line 20
   12     +2  \ bar                                                                           a.py line 20

"#
        );
    }

    #[test]
    fn test_invalid_espan_ids() {
        let mut data1 = TracingData::new_for_test();
        let span_id1 = data1.add_espan(&meta("foo", "a.py", "10"), None);

        let mut data2 = TracingData::new_for_test();
        let span_id2 = data2.add_espan(&meta("foo", "a.py", "10"), None);

        assert_ne!(span_id1, span_id2);

        // Mixing EspanIds with incompatible TracingData is detected and ignored.
        assert!(!data1.add_action(span_id2, Action::EnterSpan));
        assert!(!data2.add_action(span_id1, Action::EnterSpan));
        assert_eq!(data1.ascii(&Default::default()), "");
        assert_eq!(data2.ascii(&Default::default()), "");
    }
}
