/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;
use bytes::Bytes;

use mercurial_types::HgNodeHash;
use mercurial_types::HgParents;
use mononoke_api::errors::MononokeError;
use revisionstore_types::Metadata;

use super::repo::HgRepoContext;

/// Trait describing the common interface for working with Mercurial's
/// blob-like types, such as files and trees.
#[async_trait]
pub trait HgDataContext: Send + Sync + 'static {
    type NodeId;

    /// Get the ID (node hash) of this blob.
    fn node_id(&self) -> Self::NodeId;

    /// Get the blob's parents as a strongly-typed tuple.
    fn parents(&self) -> (Option<Self::NodeId>, Option<Self::NodeId>);

    /// Get the parents as an HgParents enum.
    fn hg_parents(&self) -> HgParents;

    /// Fetch the blob content.
    async fn content(&self) -> Result<(Bytes, Metadata), MononokeError>;
}

/// Trait describing a typed identifier for a blob-like type.
/// Typical examples include manifest and filenode hashes.
/// This trait allows constructing `HgDataContext` from these
/// identifiers in a generic and type-safe way.
#[async_trait]
pub trait HgDataId: Send + Sync + 'static {
    type Context: HgDataContext;

    /// Convert a HgNodeHash (which could represent any kind
    /// of Mercurial ID) into this specific ID type.
    fn from_node_hash(hash: HgNodeHash) -> Self;

    /// Load a context for this blob from the repo.
    async fn context(self, repo: HgRepoContext) -> Result<Option<Self::Context>, MononokeError>;
}
