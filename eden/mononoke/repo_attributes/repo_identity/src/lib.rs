/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Repo Identity.
//!
//! Stores the id and name of the repository.
use std::hash::Hash;

use mononoke_types::RepositoryId;

/// Repository identity information.
#[facet::facet]
#[derive(Hash, PartialEq, Eq)]
pub struct RepoIdentity {
    /// The ID of the repository.
    id: RepositoryId,

    /// The name of the repository.
    name: String,
}

impl RepoIdentity {
    /// Construct a new RepoIdentity.
    pub fn new(id: RepositoryId, name: String) -> RepoIdentity {
        RepoIdentity { id, name }
    }

    /// The ID of the repository.
    pub fn id(&self) -> RepositoryId {
        self.id
    }

    /// The name of the repository.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }
}
