/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Ephemeral Blobstore Builder

use std::sync::Arc;

use blobstore::{Blobstore, BlobstoreKeySource, BlobstorePutOps};
use chrono::Duration as ChronoDuration;
use sql_construct::SqlConstruct;
use sql_ext::SqlConnections;

use crate::store::EphemeralBlobstore;

/// Ephemeral Blobstore Builder.
pub struct EphemeralBlobstoreBuilder {
    /// Database used to manage the ephemeral blobstore metadata.
    connections: SqlConnections,
}

impl SqlConstruct for EphemeralBlobstoreBuilder {
    const LABEL: &'static str = "ephemeral_blobstore";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-ephemeral-blobstore.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl EphemeralBlobstoreBuilder {
    pub fn build<B: Blobstore + BlobstoreKeySource + BlobstorePutOps + 'static>(
        self,
        blobstore: Arc<B>,
        initial_bubble_lifespan: ChronoDuration,
        bubble_expiration_grace: ChronoDuration,
    ) -> EphemeralBlobstore {
        EphemeralBlobstore::new(
            self.connections,
            blobstore,
            initial_bubble_lifespan,
            bubble_expiration_grace,
        )
    }
}
