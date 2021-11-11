/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::hash_map;
use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use crossbeam::channel::Sender;
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

    pub found_tx: Sender<FetchResult<T>>,
}

impl<T: StoreValue> CommonFetchState<T> {
    #[instrument(skip(keys))]
    pub(crate) fn new(
        keys: impl Iterator<Item = Key>,
        attrs: T::Attrs,
        found_tx: Sender<FetchResult<T>>,
    ) -> Self {
        Self {
            pending: keys.collect(),
            request_attrs: attrs,
            found: HashMap::new(),
            found_tx,
        }
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
                    let _ = self.found_tx.send(FetchResult::Value((key, new)));
                    return true;
                } else {
                    *available = new;
                }
            }
            Vacant(entry) => {
                if value.attrs().has(self.request_attrs) {
                    self.pending.remove(&key);
                    let value = value.mask(self.request_attrs);
                    let _ = self.found_tx.send(FetchResult::Value((key, value)));
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

        let _ = self.found_tx.send(FetchResult::Finished(FetchFinish {
            incomplete,
            other_errors: errors.other_errors,
        }));
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

#[derive(Debug)]
pub enum FetchResult<T> {
    Value((Key, T)),
    Finished(FetchFinish),
}

#[derive(Debug)]
pub struct FetchFinish {
    pub incomplete: HashMap<Key, Vec<Error>>,
    pub other_errors: Vec<Error>,
}

#[derive(Debug)]
pub struct FetchResults<T> {
    pub complete: HashMap<Key, T>,
    pub incomplete: HashMap<Key, Vec<Error>>,
    pub other_errors: Vec<Error>,
}

impl<T> FetchResults<T> {
    /// Return the list of keys which could not be fetched, or any errors encountered
    pub fn missing(mut self) -> Result<Vec<Key>> {
        if let Some(err) = self.other_errors.pop() {
            return Err(err).into();
        }

        let mut not_found = Vec::new();
        for (key, mut errors) in self.incomplete.drain() {
            if let Some(err) = errors.pop() {
                return Err(err).into();
            }
            not_found.push(key);
        }

        Ok(not_found)
    }

    /// Return the single requested file if found, or any errors encountered
    pub fn single(mut self) -> Result<Option<T>> {
        if let Some(err) = self.other_errors.pop() {
            return Err(err).into();
        }

        for (_key, mut errors) in self.incomplete.drain() {
            if let Some(err) = errors.pop() {
                return Err(err).into();
            } else {
                return Ok(None);
            }
        }

        Ok(Some(
            self.complete
                .drain()
                .next()
                .ok_or_else(|| anyhow!("no results found in either incomplete or complete"))?
                .1,
        ))
    }

    /// Returns a stream of all successful fetches and errors, for compatibility with old scmstore
    pub fn results(self) -> impl Iterator<Item = Result<(Key, T)>> {
        self.complete
            .into_iter()
            .map(Ok)
            .chain(
                self.incomplete
                    .into_iter()
                    .map(|(key, errors)| {
                        if errors.len() > 0 {
                            errors
                        } else {
                            vec![anyhow!("key not found: {}", key)]
                        }
                    })
                    .flatten()
                    .map(Err),
            )
            .chain(self.other_errors.into_iter().map(Err))
    }

    /// Returns a stream of all fetch results, including not found and errors
    pub fn fetch_results(self) -> impl Iterator<Item = (Key, Result<Option<T>>)> {
        self.complete
            .into_iter()
            .map(|(key, item)| (key, Ok(Some(item))))
            .chain(self.incomplete.into_iter().map(|(key, mut errors)| {
                (
                    key,
                    // TODO(meyer): Should we make some VecError type or fan out like in results, above?
                    if let Some(err) = errors.pop() {
                        Err(err)
                    } else {
                        Ok(None)
                    },
                )
            }))
    }
}
