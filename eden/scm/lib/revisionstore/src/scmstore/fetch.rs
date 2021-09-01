/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::{anyhow, Error, Result};
use tracing::instrument;

use types::Key;

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
pub struct FetchResults<T, M> {
    pub complete: HashMap<Key, T>,
    pub incomplete: HashMap<Key, Vec<Error>>,
    pub other_errors: Vec<Error>,
    pub metrics: M,
}

impl<T, M> FetchResults<T, M> {
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

    pub fn metrics(&self) -> &M {
        &self.metrics
    }
}
