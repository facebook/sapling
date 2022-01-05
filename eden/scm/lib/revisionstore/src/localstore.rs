/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Common method shared between all mercurial stores

use std::ops::Deref;
use std::path::Path;

use anyhow::Result;

use crate::types::StoreKey;

/// Defines the behavior of the datapack code when encountering blobs that are externally stored.
/// This is typically found for LFS pointers stored in packfiles.
#[derive(Debug, PartialEq, Copy, Clone)]
pub enum ExtStoredPolicy {
    /// The datapack code will return it
    Use,
    /// The datapack code will pretend the blob isn't present
    Ignore,
}

pub trait StoreFromPath {
    /// Builds a Store from a filepath. The default implementation panics.
    fn from_path(_path: &Path, _extstored: ExtStoredPolicy) -> Result<Self>
    where
        Self: Sized;
}

pub trait LocalStore: Send + Sync {
    /// Returns all the keys that aren't present in this `Store`.
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>>;

    /// Test whether this `Store` contains a specific key.
    fn contains(&self, key: &StoreKey) -> Result<bool> {
        Ok(self.get_missing(&[key.clone()])?.is_empty())
    }
}

/// All the types that can `Deref` into a `Store` implements `Store`.
impl<T: LocalStore + ?Sized, U: Deref<Target = T> + Send + Sync> LocalStore for U {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        T::get_missing(self, keys)
    }
}
