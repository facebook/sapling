/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::hash_map;
use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::Error;
use anyhow::Result;
use crossbeam::channel::Sender;
use thiserror::Error;
use tracing::instrument;
use types::Key;

use crate::scmstore::attrs::StoreAttrs;
use crate::scmstore::value::StoreValue;

pub(crate) struct CommonFetchState<T: StoreValue> {
    /// Requested keys for which at least some attributes haven't been found.
    pub pending: HashSet<Key>,

    /// Which attributes were requested
    pub request_attrs: T::Attrs,

    /// All attributes which have been found so far
    pub found: HashMap<Key, T>,

    pub found_tx: Sender<Result<(Key, T), KeyFetchError>>,
}

impl<T: StoreValue> CommonFetchState<T> {
    #[instrument(skip(keys))]
    pub(crate) fn new(
        keys: impl Iterator<Item = Key>,
        attrs: T::Attrs,
        found_tx: Sender<Result<(Key, T), KeyFetchError>>,
    ) -> Self {
        Self {
            pending: keys.collect(),
            request_attrs: attrs,
            found: HashMap::new(),
            found_tx,
        }
    }

    pub(crate) fn pending_len(&self) -> usize {
        self.pending.len()
    }

    #[instrument(skip(self))]
    pub(crate) fn pending<'a>(
        &'a self,
        fetchable: T::Attrs,
        with_computable: bool,
    ) -> impl Iterator<Item = (&'a Key, T::Attrs)> + 'a {
        self.pending.iter().filter_map(move |key| {
            let actionable = self.actionable(key, fetchable, with_computable);
            if actionable.any() {
                Some((key, actionable))
            } else {
                None
            }
        })
    }

    #[instrument(skip(self, value))]
    pub(crate) fn found(&mut self, key: Key, value: T) -> bool {
        use hash_map::Entry::*;
        match self.found.entry(key.clone()) {
            Occupied(mut entry) => {
                tracing::debug!("merging into previously fetched attributes");
                // Combine the existing and newly-found attributes, overwriting existing attributes with the new ones
                // if applicable (so that we can re-use this function to replace in-memory files with mmap-ed files)
                let available = entry.get_mut();
                let new = value | std::mem::take(available);

                if new.attrs().has(self.request_attrs) {
                    self.found.remove(&key);
                    self.pending.remove(&key);
                    let new = new.mask(self.request_attrs);
                    let _ = self.found_tx.send(Ok((key, new)));
                    return true;
                } else {
                    *available = new;
                }
            }
            Vacant(entry) => {
                if value.attrs().has(self.request_attrs) {
                    self.pending.remove(&key);
                    let value = value.mask(self.request_attrs);
                    let _ = self.found_tx.send(Ok((key, value)));
                    return true;
                } else {
                    entry.insert(value);
                }
            }
        };

        return false;
    }

    #[instrument(skip(self, errors))]
    pub(crate) fn results(mut self, errors: FetchErrors) {
        // Combine and collect errors
        let mut incomplete = errors.fetch_errors;
        for key in self.pending.into_iter() {
            self.found.remove(&key);
            incomplete.entry(key).or_insert_with(Vec::new);
        }

        for (key, _) in self.found.iter_mut() {
            // Don't return errors for keys we eventually found.
            incomplete.remove(key);
        }

        for (key, errors) in incomplete {
            let _ = self
                .found_tx
                .send(Err(KeyFetchError::KeyedError { key, errors }.into()));
        }

        for err in errors.other_errors {
            let _ = self.found_tx.send(Err(KeyFetchError::Other(err)));
        }
    }

    #[instrument(level = "trace", skip(self))]
    pub(crate) fn actionable(
        &self,
        key: &Key,
        fetchable: T::Attrs,
        with_computable: bool,
    ) -> T::Attrs {
        if fetchable.none() {
            return T::Attrs::NONE;
        }

        let available = self.found.get(key).map_or(T::Attrs::NONE, |f| f.attrs());
        let (available, fetchable) = if with_computable {
            (available.with_computable(), fetchable.with_computable())
        } else {
            (available, fetchable)
        };
        let missing = self.request_attrs - available;
        let actionable = missing & fetchable;
        actionable
    }
}

#[derive(Debug, Error)]
pub enum KeyFetchError {
    #[error("Key fetch failed: {key}: {errors:?}")]
    KeyedError { key: Key, errors: Vec<Error> },
    #[error(transparent)]
    Other(Error),
}

pub(crate) struct FetchErrors {
    /// Errors encountered for specific keys
    pub(crate) fetch_errors: HashMap<Key, Vec<Error>>,

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

    #[instrument(level = "error", skip(self))]
    pub(crate) fn keyed_error(&mut self, key: Key, err: Error) {
        self.fetch_errors
            .entry(key)
            .or_insert_with(Vec::new)
            .push(err);
    }

    #[instrument(level = "error", skip(self))]
    pub(crate) fn other_error(&mut self, err: Error) {
        self.other_errors.push(err);
    }
}

pub struct FetchResults<T> {
    iterator: Box<dyn Iterator<Item = Result<(Key, T), KeyFetchError>>>,
}

impl<T> IntoIterator for FetchResults<T> {
    type Item = Result<(Key, T), KeyFetchError>;
    type IntoIter = Box<dyn Iterator<Item = Result<(Key, T), KeyFetchError>>>;

    fn into_iter(self) -> Self::IntoIter {
        self.iterator
    }
}

impl<T> FetchResults<T> {
    pub fn new(iterator: Box<dyn Iterator<Item = Result<(Key, T), KeyFetchError>>>) -> Self {
        FetchResults { iterator }
    }

    pub fn consume(self) -> (HashMap<Key, T>, HashMap<Key, Vec<Error>>, Vec<Error>) {
        let mut found = HashMap::new();
        let mut missing = HashMap::new();
        let mut errors = vec![];
        for result in self {
            match result {
                Ok((key, value)) => {
                    found.insert(key, value);
                }
                Err(err) => match err {
                    KeyFetchError::KeyedError { key, errors } => {
                        missing.insert(key.clone(), errors);
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
                    KeyFetchError::KeyedError { key, mut errors } => {
                        if let Some(err) = errors.pop() {
                            return Err(err);
                        } else {
                            missing.push(key.clone());
                        }
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
