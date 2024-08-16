/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

/// Used as a struct field to provide a stable identity for debug logging purpose.
///
/// Differences from `VerLink`:
/// - Does not maintain a partial order.
/// - Crates a new `Id` on clone.
///
/// Example uses:
/// - When this type of struct gets created (new or cloned)?
/// - When this type of struct gets dropped?
/// - With custom logging, when an operation happens, what is the identity of this struct?
pub(crate) struct LifecycleId {
    id: usize,
    type_name: &'static str,
}

// Use a non-zero starting id to ease search.
static NEXT_LIFECYCLE_ID: AtomicUsize = AtomicUsize::new(2000);

impl LifecycleId {
    pub(crate) fn new<T>() -> Self {
        let type_name = std::any::type_name::<T>();
        // make it less verbose: "foo::bar::T" -> "T"
        let type_name = type_name.rsplit("::").next().unwrap_or(type_name);
        let id = NEXT_LIFECYCLE_ID.fetch_add(1, Ordering::AcqRel);
        tracing::debug!(type_name = type_name, id = id, "created");
        Self { id, type_name }
    }
}

impl fmt::Debug for LifecycleId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}", self.type_name, self.id)
    }
}

impl Clone for LifecycleId {
    fn clone(&self) -> Self {
        let type_name = self.type_name;
        let id = NEXT_LIFECYCLE_ID.fetch_add(1, Ordering::AcqRel);
        tracing::debug!(type_name = type_name, id = id, from_id = self.id, "cloned");
        Self { id, type_name }
    }
}

impl Drop for LifecycleId {
    fn drop(&mut self) {
        let type_name = self.type_name;
        let id = self.id;
        tracing::debug!(type_name = type_name, id = id, "dropped");
    }
}
