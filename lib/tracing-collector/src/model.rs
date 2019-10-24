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
