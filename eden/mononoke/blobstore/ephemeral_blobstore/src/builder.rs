/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Ephemeral Store Builder

use std::sync::Arc;
use std::time::Duration;

use blobstore::BlobstoreEnumerableWithUnlink;
use metaconfig_types::BubbleDeletionMode;
use mononoke_types::RepositoryId;
use sql_construct::SqlConstruct;
use sql_ext::SqlConnections;

use crate::store::RepoEphemeralStore;

/// Ephemeral Store Builder.
pub struct RepoEphemeralStoreBuilder {
    /// Database used to manage the ephemeral blobstore metadata.
    connections: SqlConnections,
}

impl SqlConstruct for RepoEphemeralStoreBuilder {
    const LABEL: &'static str = "ephemeral_blobstore";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-ephemeral-blobstore.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl RepoEphemeralStoreBuilder {
    pub fn build(
        self,
        repo_id: RepositoryId,
        blobstore: Arc<dyn BlobstoreEnumerableWithUnlink>,
        initial_bubble_lifespan: Duration,
        bubble_expiration_grace: Duration,
        bubble_deletion_mode: BubbleDeletionMode,
    ) -> RepoEphemeralStore {
        RepoEphemeralStore::new(
            repo_id,
            self.connections,
            blobstore,
            initial_bubble_lifespan,
            bubble_expiration_grace,
            bubble_deletion_mode,
        )
    }
}
