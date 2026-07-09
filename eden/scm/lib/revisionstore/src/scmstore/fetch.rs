/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use progress_model::ProgressBar;
use progress_model::Registry;
use slex::Items;
use slex::ItemsWriter;
use types::FetchContext;
use types::Key;
use types::errors::KeyedError;
use types::errors::SharedError;

use crate::scmstore::attrs::StoreAttrs;
use crate::scmstore::value::StoreValue;

pub(crate) struct PendingValue<T> {
    pub(crate) value: T,
    pub(crate) count: usize,
}
/// Per-store counter that bounds the number of items a store can deliver across
/// the lifetime of the process. Once the limit is exceeded, every subsequent
/// item delivered through `CommonFetchState` is converted to an error so the
/// operation aborts, no matter which code path or caller drove the fetch.
///
/// The default (`limit == 0`) disables the guard.
#[derive(Clone, Default)]
pub struct MaxFetchCount {
    inner: Arc<MaxFetchCountInner>,
}

#[derive(Default)]
struct MaxFetchCountInner {
    // 0 disables the guard.
    limit: u64,
    err_msg: String,
    count: AtomicU64,
}

impl MaxFetchCount {
    pub fn new(limit: u64, err_msg: String) -> Self {
        Self {
            inner: Arc::new(MaxFetchCountInner {
                limit,
                err_msg,
                count: AtomicU64::new(0),
            }),
        }
    }

    /// Increment the counter; returns `Err(message)` once the limit is
    /// exceeded so callers must handle the abort case. Returns `Ok(())` if no
    /// limit is set or while still under the limit.
    pub fn try_increment(&self) -> Result<(), String> {
        if self.inner.limit == 0 {
            return Ok(());
        }
        if self.inner.count.fetch_add(1, Ordering::Relaxed) + 1 > self.inner.limit {
            return Err(self.inner.err_msg.clone());
        }
        Ok(())
    }
}

pub(crate) fn fan_out_cloned<T: Clone>(value: T, count: usize, mut cb: impl FnMut(T)) {
    debug_assert!(count > 0);
    for value in std::iter::repeat_with(|| value.clone()).take(count - 1) {
        cb(value);
    }
    cb(value);
}

pub(crate) type FetchItems<T> = Items<(Key, T), KeyFetchError>;
pub(crate) type FetchItemsWriter<T> = ItemsWriter<(Key, T), KeyFetchError>;

pub(crate) struct CommonFetchState<'a, T: StoreValue + Send + 'static> {
    /// Requested keys for which at least some attributes haven't been found.
    pub pending: HashMap<Key, PendingValue<T>>,

    /// Which attributes were requested
    pub request_attrs: T::Attrs,

    pub results: &'a mut FetchItemsWriter<T>,

    pub fctx: FetchContext,

    bar: Arc<ProgressBar>,

    max_fetch_count: MaxFetchCount,
}

impl<'a, T: StoreValue + Send + 'static + std::fmt::Debug> CommonFetchState<'a, T> {
    pub(crate) fn new(
        keys: impl IntoIterator<Item = Key>,
        attrs: T::Attrs,
        results: &'a mut FetchItemsWriter<T>,
        fctx: FetchContext,
        bar: Arc<ProgressBar>,
        max_fetch_count: MaxFetchCount,
    ) -> Self {
        let keys = keys.into_iter();
        let (lower_bound, upper_bound) = keys.size_hint();
        let mut pending =
            HashMap::<Key, PendingValue<T>>::with_capacity(upper_bound.unwrap_or(lower_bound));
        for key in keys {
            pending
                .entry(key)
                .and_modify(|pending| pending.count += 1)
                .or_insert_with(|| PendingValue {
                    value: T::default(),
                    count: 1,
                });
        }

        Self {
            pending,
            request_attrs: attrs,
            results,
            fctx,
            bar,
            max_fetch_count,
        }
    }

    fn send_found_impl(
        max_fetch_count: &MaxFetchCount,
        results: &mut FetchItemsWriter<T>,
        key: Key,
        value: T,
    ) {
        match max_fetch_count.try_increment() {
            Ok(()) => {
                results.push_item((key, value));
            }
            Err(message) => {
                results.push_error(KeyFetchError::MaxFetchCountExceeded(message));
            }
        }
    }

    fn fan_out_found(
        max_fetch_count: &MaxFetchCount,
        results: &mut FetchItemsWriter<T>,
        key: Key,
        value: T,
        count: usize,
    ) {
        fan_out_cloned((key, value), count, |(key, value)| {
            Self::send_found_impl(max_fetch_count, results, key, value);
        });
    }

    pub(crate) fn unique_keys(&self) -> Vec<Key> {
        self.pending.keys().cloned().collect()
    }

    pub(crate) fn pending_len(&self) -> usize {
        self.pending.values().map(|pending| pending.count).sum()
    }

    pub(crate) fn progress_bar(&self) -> Arc<ProgressBar> {
        Arc::clone(&self.bar)
    }

    pub(crate) fn pending<'b>(
        &'b self,
        fetchable: T::Attrs,
        with_computable: bool,
    ) -> impl Iterator<Item = (&'b Key, &'b T)> + 'b {
        self.pending.iter().filter_map(move |(key, pending)| {
            let actionable = self.actionable(key, fetchable, with_computable);
            if actionable.any() {
                Some((key, &pending.value))
            } else {
                None
            }
        })
    }

    // Combine `pending()` and `found()` into a single operation. This allows the caller
    // to avoid copying the keys returned by `pending()`.
    pub(crate) fn iter_pending(
        &mut self,
        fetchable: T::Attrs,
        with_computable: bool,
        mut cb: impl FnMut(&Key) -> Option<T>,
    ) {
        let request_attrs = self.request_attrs;
        let ignore_result = self.fctx.mode().ignore_result();
        let max_fetch_count = &self.max_fetch_count;
        let results = &mut *self.results;
        self.pending.retain(|key, pending| {
            let actionable = Self::actionable_attrs(
                request_attrs,
                pending.value.attrs(),
                fetchable,
                with_computable,
            );

            if actionable.any() {
                if let Some(value) = cb(key) {
                    let new = value | std::mem::take(&mut pending.value);

                    // Check if the newly fetched attributes fulfill all what was originally requested.
                    if new.attrs().has(request_attrs) {
                        if !ignore_result {
                            let new = new.mask(request_attrs);
                            Self::fan_out_found(
                                max_fetch_count,
                                results,
                                key.clone(),
                                new,
                                pending.count,
                            );
                        }

                        // This key has been fulfilled - don't retain it.
                        return false;
                    } else {
                        // Not fulfilled yet - update value with new attributes.
                        pending.value = new;
                    }
                }
            }

            // No change - retain value in `pending`.
            true
        });
    }

    pub(crate) fn found(&mut self, key: Key, value: T) -> bool {
        if let Some(pending) = self.pending.get_mut(&key) {
            // Combine the existing and newly-found attributes, overwriting existing attributes with the new ones
            // if applicable (so that we can reuse this function to replace in-memory files with mmap-ed files)
            let new = value | std::mem::take(&mut pending.value);

            if new.attrs().has(self.request_attrs) {
                let count = pending.count;
                self.pending.remove(&key);

                if !self.fctx.mode().ignore_result() {
                    let new = new.mask(self.request_attrs);
                    Self::fan_out_found(&self.max_fetch_count, self.results, key, new, count);
                }
                self.bar.increase_position(count as u64);

                return true;
            } else {
                pending.value = new;
            }
        } else {
            tracing::warn!(?key, "found something but key is already done");
        }

        false
    }

    // Propagate errors to the result channel. report_missing controls whether we report errors for
    // remaining pending items that we did not see a specific error for. This is set to `false` when
    // we get "overall" errors that abort the operation early, potentially leaving all the items
    // pending.
    pub(crate) fn results(&mut self, errors: FetchErrors, report_missing: bool) {
        // Only emit keyed errors for items that are stuck in pending.
        // We may have, for example, gotten an error fetching a key from CAS, but then succeeded in
        // fetching it from SLAPI. In that case, `fetch_errors` contains the CAS error, but the
        // requested item won't be in `pending` since it was satisfied via SLAPI.
        let mut incomplete = errors.fetch_errors;
        for (key, pending) in self.pending.drain() {
            let count = pending.count;
            match incomplete.remove(&key) {
                Some(err) => {
                    Self::fan_out_keyed_error(self.results, key, err, count);
                    continue;
                }
                None => {
                    if !report_missing {
                        continue;
                    }

                    if self.fctx.mode().is_local() {
                        fan_out_cloned(key, count, |key| {
                            self.results.push_error(KeyFetchError::NotFoundLocally(key));
                        });
                    } else {
                        // Should not happen normally since `incomplete` should contain the specific error we got from server.
                        fan_out_cloned(key, count, |key| {
                            self.results
                                .push_error(KeyFetchError::KeyedError(KeyedError(
                                    key,
                                    anyhow!("server did not provide content"),
                                )));
                        });
                    }
                }
            }
        }

        for err in errors.other_errors {
            self.results.push_error(KeyFetchError::Other(err));
        }
    }

    fn fan_out_keyed_error(
        results: &mut FetchItemsWriter<T>,
        key: Key,
        err: SharedError,
        count: usize,
    ) {
        fan_out_cloned((key, err), count, |(key, err)| {
            results.push_error(KeyFetchError::KeyedError(KeyedError(
                key,
                err.into_anyhow(),
            )));
        });
    }

    pub(crate) fn actionable(
        &self,
        key: &Key,
        fetchable: T::Attrs,
        with_computable: bool,
    ) -> T::Attrs {
        if fetchable.none() {
            return T::Attrs::NONE;
        }

        self.pending.get(key).map_or(T::Attrs::NONE, |pending| {
            Self::actionable_attrs(
                self.request_attrs,
                pending.value.attrs(),
                fetchable,
                with_computable,
            )
        })
    }

    fn actionable_attrs(
        // What the original fetch() request wants to fetch.
        requested: T::Attrs,
        // What is already available for this key.
        available: T::Attrs,
        // What the current data source is able to provide.
        fetchable: T::Attrs,
        // Whether we want to consider which attributes are computable.
        with_computable: bool,
    ) -> T::Attrs {
        let (available, fetchable) = if with_computable {
            (available.with_computable(), fetchable.with_computable())
        } else {
            (available, fetchable)
        };
        let missing = requested - available;

        missing & fetchable
    }
}

#[derive(Debug)]
pub enum KeyFetchError {
    // No unexpected error, but key was not in local repo store.
    NotFoundLocally(Key),
    // Unexpected error, including key not found in remote store.
    KeyedError(KeyedError),
    // Per-store fetch budget exceeded; carries the configured abort message.
    // A leaf error (no source) so the printed chain doesn't duplicate it.
    MaxFetchCountExceeded(String),
    Other(Error),
}

// Manual std::error impl to pick a source() for KeyedError.
impl std::error::Error for KeyFetchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::KeyedError(err) => Some(err),
            Self::Other(err) => Some(err.as_ref()),
            Self::NotFoundLocally(_) => None,
            Self::MaxFetchCountExceeded(_) => None,
        }
    }
}

impl fmt::Display for KeyFetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeyedError(KeyedError(key, err)) => {
                write!(f, "key fetch failed {key}: {err:?}")
            }
            Self::MaxFetchCountExceeded(msg) => f.write_str(msg),
            Self::NotFoundLocally(key) => {
                write!(f, "key not in local store and not contacting remote: {key}")
            }
            Self::Other(err) => err.fmt(f),
        }
    }
}

#[derive(Default, Debug)]
pub(crate) struct FetchErrors {
    /// Errors encountered for specific keys
    pub(crate) fetch_errors: HashMap<Key, SharedError>,

    /// Errors encountered that don't apply to a single key
    pub(crate) other_errors: Vec<Error>,
}

impl FetchErrors {
    pub(crate) fn new() -> Self {
        FetchErrors {
            fetch_errors: HashMap::new(),
            other_errors: Vec::new(),
        }
    }

    pub(crate) fn keyed_error(&mut self, key: Key, err: Error) {
        self.fetch_errors
            .entry(key)
            .or_insert_with(|| SharedError::new(err));
    }

    pub(crate) fn other_error(&mut self, err: Error) {
        self.other_errors.push(err);
    }
}

pub struct FetchResults<T: Send + 'static> {
    items: FetchItems<T>,
}

impl<T: Send + 'static> IntoIterator for FetchResults<T> {
    type Item = Result<(Key, T), KeyFetchError>;
    type IntoIter = Box<dyn Iterator<Item = Result<(Key, T), KeyFetchError>> + Send>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.items.into_iter())
    }
}

impl<T: Send + 'static> FetchResults<T> {
    pub fn new(iterator: Box<dyn Iterator<Item = Result<(Key, T), KeyFetchError>> + Send>) -> Self {
        Self::from_items(Items::item_stream(iterator))
    }

    pub(crate) fn empty() -> Self {
        Self::from_items(Items::empty())
    }

    pub(crate) fn from_items(items: FetchItems<T>) -> Self {
        FetchResults { items }
    }

    pub(crate) fn into_items(self) -> FetchItems<T> {
        self.items
    }

    pub(crate) fn from_process(
        should_spawn: bool,
        process: impl FnOnce(&mut FetchItemsWriter<T>) + Send + 'static,
    ) -> Self {
        let active_bar = Registry::main().get_active_progress_bar();
        let items = ItemsWriter::from_process(should_spawn, move |writer| {
            Registry::main().set_active_progress_bar(active_bar);
            process(writer);
        });
        Self::from_items(items)
    }

    pub fn consume(self) -> (HashMap<Key, T>, HashMap<Key, Error>, Vec<Error>) {
        let mut found = HashMap::new();
        let mut missing = HashMap::new();
        let mut errors = vec![];
        for result in self {
            match result {
                Ok((key, value)) => {
                    found.insert(key, value);
                }
                Err(err) => match err {
                    KeyFetchError::KeyedError(KeyedError(key, err)) => {
                        missing.insert(key, err);
                    }
                    KeyFetchError::NotFoundLocally(ref key) => {
                        missing.insert(key.clone(), err.into());
                    }
                    err @ KeyFetchError::MaxFetchCountExceeded(_) => {
                        errors.push(err.into());
                    }
                    KeyFetchError::Other(err) => {
                        errors.push(err);
                    }
                },
            };
        }
        (found, missing, errors)
    }

    /// Return the list of keys which could not be fetched, or any errors encountered
    pub fn missing(self) -> Result<Vec<Key>> {
        // Don't use self.consume here since it pends all the found results in memory, which can be
        // expensive.
        let mut missing = vec![];
        for result in self {
            match result {
                Ok(_) => {}
                Err(err) => match err {
                    KeyFetchError::KeyedError(KeyedError(key, _err)) => {
                        missing.push(key);
                    }
                    KeyFetchError::NotFoundLocally(key) => {
                        missing.push(key);
                    }
                    err @ KeyFetchError::MaxFetchCountExceeded(_) => {
                        return Err(err.into());
                    }
                    KeyFetchError::Other(err) => {
                        return Err(err);
                    }
                },
            };
        }
        Ok(missing)
    }

    /// Return the single requested file if found, or any errors encountered. `Ok(None)`
    /// is returned only in LocalOnly mode (where it is expected for content to not be
    /// available). If remote fetching is enabled and the key wasn't found, an error is
    /// returned (since this is unexpected and indicates a bug or data corruption).
    pub fn single(self) -> Result<Option<T>> {
        let mut first = None;
        for result in self {
            match result {
                Ok((_key, value)) => {
                    if first.is_none() {
                        first = Some(value)
                    }
                }
                Err(KeyFetchError::NotFoundLocally(_)) => continue,
                Err(err) => return Err(err.into()),
            }
        }

        Ok(first)
    }
}

#[cfg(test)]
mod tests {
    use std::ops::BitAnd;
    use std::ops::BitOr;
    use std::ops::Not;
    use std::ops::Sub;

    use ::types::errors::NetworkError;
    use ::types::fetch_mode::FetchMode;
    use ::types::testutil::key;
    use anyhow::anyhow;
    use progress_model::ProgressBar;

    use super::*;

    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    struct TestAttrs(u8);

    impl TestAttrs {
        const CONTENT: Self = Self(1);
    }

    impl BitAnd for TestAttrs {
        type Output = Self;

        fn bitand(self, rhs: Self) -> Self::Output {
            Self(self.0 & rhs.0)
        }
    }

    impl BitOr for TestAttrs {
        type Output = Self;

        fn bitor(self, rhs: Self) -> Self::Output {
            Self(self.0 | rhs.0)
        }
    }

    impl Not for TestAttrs {
        type Output = Self;

        fn not(self) -> Self::Output {
            Self(!self.0 & Self::CONTENT.0)
        }
    }

    impl Sub for TestAttrs {
        type Output = Self;

        fn sub(self, rhs: Self) -> Self::Output {
            self & !rhs
        }
    }

    impl StoreAttrs for TestAttrs {
        const NONE: Self = Self(0);

        fn with_computable(&self) -> Self {
            *self
        }
    }

    #[derive(Clone, Debug, Default)]
    struct TestValue {
        attrs: TestAttrs,
        value: u8,
    }

    impl TestValue {
        fn content(value: u8) -> Self {
            Self {
                attrs: TestAttrs::CONTENT,
                value,
            }
        }
    }

    impl BitOr for TestValue {
        type Output = Self;

        fn bitor(self, rhs: Self) -> Self::Output {
            Self {
                attrs: self.attrs | rhs.attrs,
                value: self.value.max(rhs.value),
            }
        }
    }

    impl StoreValue for TestValue {
        type Attrs = TestAttrs;

        fn attrs(&self) -> Self::Attrs {
            self.attrs
        }

        fn mask(self, attrs: Self::Attrs) -> Self {
            Self {
                attrs: self.attrs & attrs,
                value: self.value,
            }
        }
    }

    fn new_test_state<'a>(
        keys: Vec<Key>,
        writer: &'a mut FetchItemsWriter<TestValue>,
    ) -> CommonFetchState<'a, TestValue> {
        CommonFetchState::new(
            keys,
            TestAttrs::CONTENT,
            writer,
            FetchContext::new(FetchMode::LocalOnly),
            ProgressBar::new("test", 0, "items"),
            MaxFetchCount::default(),
        )
    }

    #[test]
    fn test_duplicate_pending_deduplicates_requests() {
        let duplicate = key("a", "1");
        let other = key("b", "2");
        let mut writer = ItemsWriter::inline();
        let state = new_test_state(
            vec![duplicate.clone(), duplicate.clone(), other.clone()],
            &mut writer,
        );

        let keys = state
            .pending(TestAttrs::CONTENT, false)
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();

        assert_eq!(state.pending_len(), 3);
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&duplicate));
        assert!(keys.contains(&other));
    }

    #[test]
    fn test_iter_pending_preserves_duplicate_results() {
        let duplicate = key("a", "1");
        let mut writer = ItemsWriter::inline();
        {
            let mut state = new_test_state(vec![duplicate.clone(), duplicate.clone()], &mut writer);

            let mut calls = 0;
            state.iter_pending(TestAttrs::CONTENT, false, |_| {
                calls += 1;
                Some(TestValue::content(7))
            });
            assert_eq!(calls, 1);
            assert_eq!(state.pending_len(), 0);
        }

        let results = writer.finish().into_iter().collect::<Vec<_>>();
        assert_eq!(results.len(), 2);
        for result in results {
            let (key, value) = result.unwrap();
            assert_eq!(key, duplicate);
            assert_eq!(value.value, 7);
        }
    }

    #[test]
    fn test_found_preserves_duplicate_results() {
        let duplicate = key("a", "1");
        let mut writer = ItemsWriter::inline();
        {
            let mut state = new_test_state(vec![duplicate.clone(), duplicate.clone()], &mut writer);
            assert!(state.found(duplicate.clone(), TestValue::content(9)));
            assert_eq!(state.pending_len(), 0);
        }

        let results = writer.finish().into_iter().collect::<Vec<_>>();
        assert_eq!(results.len(), 2);
        for result in results {
            let (key, value) = result.unwrap();
            assert_eq!(key, duplicate);
            assert_eq!(value.value, 9);
        }
    }

    #[test]
    fn test_results_preserves_duplicate_local_misses() {
        let duplicate = key("a", "1");
        let mut writer = ItemsWriter::inline();
        {
            let mut state = new_test_state(vec![duplicate.clone(), duplicate.clone()], &mut writer);
            state.results(FetchErrors::new(), true);
        }

        let results = writer.finish().into_iter().collect::<Vec<_>>();
        assert_eq!(results.len(), 2);
        for result in results {
            match result.unwrap_err() {
                KeyFetchError::NotFoundLocally(key) => assert_eq!(key, duplicate),
                err => panic!("unexpected error: {err:?}"),
            }
        }
    }

    #[test]
    fn test_results_preserves_duplicate_keyed_error_tags() {
        let duplicate = key("a", "1");
        let mut writer = ItemsWriter::inline();
        {
            let mut state = new_test_state(vec![duplicate.clone(), duplicate.clone()], &mut writer);
            let mut errors = FetchErrors::new();
            errors.keyed_error(duplicate.clone(), NetworkError::wrap(anyhow!("boom")));

            state.results(errors, true);
        }

        let results = writer.finish().into_iter().collect::<Vec<_>>();
        assert_eq!(results.len(), 2);
        for result in results {
            let err: anyhow::Error = result.unwrap_err().into();
            assert!(::types::errors::is_network_error(&err));
        }
    }

    #[test]
    fn test_error_chain() {
        {
            let inner_err = anyhow!("inner");
            let outer_err = inner_err.context("context");

            let err: &dyn std::error::Error = &KeyFetchError::Other(outer_err);
            assert_eq!(format!("{}", err.source().unwrap()), "context");
            assert_eq!(
                format!("{}", err.source().unwrap().source().unwrap()),
                "inner"
            );
        }

        {
            let err: &dyn std::error::Error =
                &KeyFetchError::KeyedError(KeyedError(Default::default(), anyhow!("one")));
            assert_eq!(
                format!("{}", err.source().unwrap().source().unwrap()),
                "one"
            );
        }

        {
            let err: anyhow::Error =
                KeyFetchError::Other(NetworkError::wrap(anyhow!("foo"))).into();
            assert!(types::errors::is_network_error(&err));
        }
    }

    #[test]
    fn test_max_fetch_count() {
        // Limit of 0 (default) disables the guard.
        let counter = MaxFetchCount::default();
        for _ in 0..10 {
            assert!(counter.try_increment().is_ok());
        }

        // With a limit of 3, the first 3 calls succeed and every subsequent
        // call returns the configured abort message.
        let counter = MaxFetchCount::new(3, "over the limit".to_string());
        assert!(counter.try_increment().is_ok());
        assert!(counter.try_increment().is_ok());
        assert!(counter.try_increment().is_ok());
        for _ in 0..3 {
            assert_eq!(counter.try_increment().unwrap_err(), "over the limit",);
        }
    }

    #[test]
    fn test_max_fetch_count_exceeded_chain_is_a_leaf() {
        // KeyFetchError::MaxFetchCountExceeded must have no `source()` so the
        // top-level Display message is not duplicated under "Caused by:".
        let err = KeyFetchError::MaxFetchCountExceeded("over the limit".to_string());
        assert_eq!(format!("{err}"), "over the limit");
        assert!(std::error::Error::source(&err).is_none());
    }
}
