/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use once_cell::sync::Lazy;
use parking_lot::RwLock;
use tracing::metadata::Kind;
use tracing::Level;

use crate::RuntimeCallsite;
use crate::StaticBox;

/// Information needed to create a callsite.
///
/// See also `tracing::Metadata::new`.
#[derive(Debug)]
pub struct CallsiteInfo {
    pub name: String,
    pub target: String,
    pub level: Level,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub module_path: Option<String>,
    pub field_names: Vec<String>,
}

impl Default for CallsiteInfo {
    fn default() -> Self {
        Self {
            name: String::new(),
            target: String::new(),
            level: Level::INFO,
            file: None,
            line: None,
            module_path: None,
            field_names: Vec::new(),
        }
    }
}

/// Span or Event in type system.
pub trait KindType: Send + Sync + Unpin + 'static {
    /// Converts the type to the tracing `Kind` variable.
    fn kind() -> Kind;

    /// The hashmap used to store callsites.
    fn static_map() -> &'static LazyMap<Self>
    where
        Self: Sized;
}

pub type CallsiteKey = (usize, usize);
type LazyMap<K> = Lazy<RwLock<HashMap<CallsiteKey, StaticBox<RuntimeCallsite<K>>>>>;

/// The "Event" kind.
#[derive(Copy, Clone, Debug)]
pub struct EventKindType;

/// The "Span" kind.
#[derive(Copy, Clone, Debug)]
pub struct SpanKindType;

impl KindType for EventKindType {
    fn kind() -> Kind {
        Kind::EVENT
    }

    fn static_map() -> &'static LazyMap<Self> {
        &DYNAMIC_EVENT_CALLSITES
    }
}

impl KindType for SpanKindType {
    fn kind() -> Kind {
        Kind::SPAN
    }

    fn static_map() -> &'static LazyMap<Self> {
        &DYNAMIC_SPAN_CALLSITES
    }
}

/// Collection of dynamically allocated Callsites for spans.
static DYNAMIC_SPAN_CALLSITES: LazyMap<SpanKindType> = Lazy::new(Default::default);

/// Collection of dynamically allocated Callsites for events.
static DYNAMIC_EVENT_CALLSITES: LazyMap<EventKindType> = Lazy::new(Default::default);
