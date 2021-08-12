/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Ephemeral Blobstore Builder

use chrono::Duration as ChronoDuration;
use mononoke_types::RepositoryId;
use repo_blobstore::RepoBlobstore;
use sql_construct::SqlConstruct;
use sql_ext::SqlConnections;

use crate::store::RepoEphemeralBlobstore;

/// Ephemeral Blobstore Builder.
pub struct RepoEphemeralBlobstoreBuilder {
    /// Database used to manage the ephemeral blobstore metadata.
    connections: SqlConnections,
}

impl SqlConstruct for RepoEphemeralBlobstoreBuilder {
    const LABEL: &'static str = "ephemeral_blobstore";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-ephemeral-blobstore.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl RepoEphemeralBlobstoreBuilder {
    pub fn build(
        self,
        repo_id: RepositoryId,
        repo_blobstore: RepoBlobstore,
        initial_bubble_lifespan: ChronoDuration,
        bubble_expiration_grace: ChronoDuration,
    ) -> RepoEphemeralBlobstore {
        RepoEphemeralBlobstore::new(
            repo_id,
            self.connections,
            repo_blobstore,
            initial_bubble_lifespan,
            bubble_expiration_grace,
        )
    }
}
