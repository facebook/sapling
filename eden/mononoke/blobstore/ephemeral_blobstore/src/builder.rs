/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Ephemeral Blobstore Builder

use std::sync::Arc;

use blobstore::Blobstore;
use mononoke_types::RepositoryId;
use sql_construct::SqlConstruct;
use sql_ext::SqlConnections;
use std::time::Duration;

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
        blobstore: Arc<dyn Blobstore>,
        initial_bubble_lifespan: Duration,
        bubble_expiration_grace: Duration,
    ) -> RepoEphemeralBlobstore {
        RepoEphemeralBlobstore::new(
            repo_id,
            self.connections,
            blobstore,
            initial_bubble_lifespan,
            bubble_expiration_grace,
        )
    }
}
