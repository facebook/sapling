/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fmt;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use crossbeam::channel::Sender;
use types::errors::KeyedError;
use types::fetch_mode::FetchMode;
use types::Key;

use crate::scmstore::attrs::StoreAttrs;
use crate::scmstore::value::StoreValue;

pub(crate) struct CommonFetchState<T: StoreValue> {
    /// Requested keys for which at least some attributes haven't been found.
    pub pending: HashMap<Key, T>,

    /// Which attributes were requested
    pub request_attrs: T::Attrs,

    pub found_tx: Sender<Result<(Key, T), KeyFetchError>>,

    pub mode: FetchMode,
}

impl<T: StoreValue + std::fmt::Debug> CommonFetchState<T> {
    pub(crate) fn new(
        keys: impl IntoIterator<Item = Key>,
        attrs: T::Attrs,
        found_tx: Sender<Result<(Key, T), KeyFetchError>>,
        mode: FetchMode,
    ) -> Self {
        Self {
            pending: keys.into_iter().map(|key| (key, T::default())).collect(),
            request_attrs: attrs,
            found_tx,
            mode,
        }
    }

    pub(crate) fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub(crate) fn pending<'a>(
        &'a self,
        fetchable: T::Attrs,
        with_computable: bool,
    ) -> impl Iterator<Item = (&'a Key, &'a T)> + 'a {
        self.pending.iter().filter_map(move |(key, store_item)| {
            let actionable = self.actionable(key, fetchable, with_computable);
            if actionable.any() {
                Some((key, store_item))
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
        self.pending.retain(|key, available| {
            let actionable = Self::actionable_attrs(
                self.request_attrs,
                available.attrs(),
                fetchable,
                with_computable,
            );

            if actionable.any() {
                if let Some(value) = cb(key) {
                    let new = value | std::mem::take(available);

                    // Check if the newly fetched attributes fulfill all what was originally requested.
                    if new.attrs().has(self.request_attrs) {
                        if !self.mode.ignore_result() {
                            let new = new.mask(self.request_attrs);
                            let _ = self.found_tx.send(Ok((key.clone(), new)));
                        }

                        // This item has been fulfilled - don't retain it.
                        return false;
                    } else {
                        // Not fulfilled yet - update value with new attributes.
                        *available = new;
                    }
                }
            }

            // No change - retain value in `pending`.
            true
        });
    }

    pub(crate) fn found(&mut self, key: Key, value: T) -> bool {
        if let Some(available) = self.pending.get_mut(&key) {
            // Combine the existing and newly-found attributes, overwriting existing attributes with the new ones
            // if applicable (so that we can re-use this function to replace in-memory files with mmap-ed files)
            let new = value | std::mem::take(available);

            if new.attrs().has(self.request_attrs) {
                self.pending.remove(&key);

                if !self.mode.ignore_result() {
                    let new = new.mask(self.request_attrs);
                    let _ = self.found_tx.send(Ok((key, new)));
                }

                return true;
            } else {
                *available = new;
            }
        } else {
            tracing::warn!(?key, "found something but key is already done");
        }

        false
    }

    pub(crate) fn results(self, errors: FetchErrors) {
        // Combine and collect errors
        let mut incomplete = errors.fetch_errors;
        for (key, _value) in self.pending.into_iter() {
            incomplete.entry(key).or_insert_with(|| {
                let msg = if self.mode.is_local() {
                    "not found locally and not contacting server"
                } else if self.mode.is_remote() {
                    // This should really never happen. If a key fails to fetch, it should've been
                    // associated with a keyed error and put in incomplete already.
                    "server did not provide content"
                } else {
                    "server did not provide content"
                };
                anyhow!("{}", msg)
            });
        }

        for (key, error) in incomplete {
            let _ = self
                .found_tx
                .send(Err(KeyFetchError::KeyedError(KeyedError(key, error))));
        }

        for err in errors.other_errors {
            let _ = self.found_tx.send(Err(KeyFetchError::Other(err)));
        }
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

        let available = self.pending.get(key).map_or(T::Attrs::NONE, |f| f.attrs());

        Self::actionable_attrs(self.request_attrs, available, fetchable, with_computable)
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
    KeyedError(KeyedError),
    Other(Error),
}

// Manual std::error impl to pick a source() for KeyedError.
impl std::error::Error for KeyFetchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Other(err) => Some(err.as_ref()),
            Self::KeyedError(err) => Some(err),
        }
    }
}

impl fmt::Display for KeyFetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Other(err) => err.fmt(f),
            Self::KeyedError(KeyedError(key, err)) => {
                write!(f, "Key fetch failed {}: {:?}", key, err)
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct FetchErrors {
    /// Errors encountered for specific keys
    pub(crate) fetch_errors: HashMap<Key, Error>,

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
        self.fetch_errors.entry(key).or_insert(err);
    }

    pub(crate) fn multiple_keyed_error(
        &mut self,
        keys: Vec<Key>,
        msg: &'static str,
        source_err: Error,
    ) {
        for key in keys {
            self.fetch_errors
                .entry(key)
                .or_insert(anyhow!("{msg}: {source_err}"));
        }
    }

    pub(crate) fn other_error(&mut self, err: Error) {
        self.other_errors.push(err);
    }
}

pub struct FetchResults<T> {
    iterator: Box<dyn Iterator<Item = Result<(Key, T), KeyFetchError>> + Send>,
}

impl<T> IntoIterator for FetchResults<T> {
    type Item = Result<(Key, T), KeyFetchError>;
    type IntoIter = Box<dyn Iterator<Item = Result<(Key, T), KeyFetchError>> + Send>;

    fn into_iter(self) -> Self::IntoIter {
        self.iterator
    }
}

impl<T> FetchResults<T> {
    pub fn new(iterator: Box<dyn Iterator<Item = Result<(Key, T), KeyFetchError>> + Send>) -> Self {
        FetchResults { iterator }
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
                        missing.push(key.clone());
                    }
                    KeyFetchError::Other(err) => {
                        return Err(err);
                    }
                },
            };
        }
        Ok(missing)
    }

    /// Return the single requested file if found, or any errors encountered
    pub fn single(self) -> Result<Option<T>> {
        let mut first = None;
        for result in self {
            let (_, value) = result?;
            if first.is_none() {
                first = Some(value)
            }
        }

        Ok(first)
    }
}

#[cfg(test)]
mod tests {
    use ::types::errors::NetworkError;
    use anyhow::anyhow;

    use super::*;

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
}
