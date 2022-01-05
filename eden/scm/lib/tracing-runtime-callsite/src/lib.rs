/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Create tracing Callsites dynamically.
//!
//! The Rust tracing crate provides macros to define Callsite and FieldSet
//! as static global variables. That serves Rust's usecases well but is
//! problematic for Python's use-cases where there are no `'static`.
//!
//! This crate is to fill the gap so Python (or other dynamic language) can
//! register Callsites on the fly to satisfy tracing's interface.

mod array;
mod callsite_info;
mod intern_str;
mod runtime_callsite;
mod static_box;

#[cfg(test)]
mod tests;

pub use callsite_info::CallsiteInfo;
pub use callsite_info::CallsiteKey;
pub use callsite_info::EventKindType;
pub use callsite_info::KindType;
pub use callsite_info::SpanKindType;
pub(crate) use intern_str::Intern;
pub use runtime_callsite::RuntimeCallsite;
pub(crate) use static_box::StaticBox;

/// Create a callsite at runtime on demand.
///
/// The `id` is used to de-duplicate callsites so repetitive calls to a function
/// reuses a single callsite. In CPython the `id` could be `id(func.__code__)`
/// for functions, or `id(sys.intern(name_str))` for spans defined by (const)
/// string names, or a combination `(frame.f_code, frame.f_lineno)`.
///
/// If the `id` was already taken, return the previously created callsite.
/// Otherwise create and return it.
///
/// The returned callsite has a `create_span` API to create spans if `K` is
/// `SpanKindType`.
pub fn create_callsite<K: KindType, F: FnOnce() -> CallsiteInfo>(
    id: CallsiteKey,
    func: F,
) -> &'static RuntimeCallsite<K> {
    let callsites = K::static_map().read();
    if let Some(callsite) = callsites.get(&id) {
        let callsite: &'static RuntimeCallsite<K> = callsite.static_ref();
        return callsite;
    }
    // func() might call create_callsite! Release the lock to avoid deadlock.
    drop(callsites);

    let info = func();

    let mut callsites = K::static_map().write();
    callsites
        .entry(id)
        .or_insert_with(|| RuntimeCallsite::<K>::new(info))
        .static_ref()
}

/// Release dynamic callsites so they don't count as memory leaks.
///
/// This is unsafe because all the previous `'static` lifetime references
/// will become invalid. Make sure nobody use the references before calling
/// this! In particular, all work related tracing would have been done
/// at this point.
pub unsafe fn release_callsites() {
    use std::sync::atomic::Ordering::Release;
    static_box::UNSAFE_ALLOW_DROP.store(true, Release);
    EventKindType::static_map().write().clear();
    SpanKindType::static_map().write().clear();
    intern_str::INTERNED_STRINGS.write().clear();
    static_box::UNSAFE_ALLOW_DROP.store(false, Release);
}
