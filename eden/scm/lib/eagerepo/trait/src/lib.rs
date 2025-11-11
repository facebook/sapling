/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! # eagerepo-trait
//!
//! Trait to make EagerRepo provide more objects (commits, trees, blobs)
//! without actually storing the objects.
//!
//! Provide interfaces to:
//! - Extend EagerRepo's SHA1-like object store to answer more Id20 lookups.
//! - Extend EagerRepo's "dag":
//!   - To "pull_lazy" segments automatically when accessing a special commit hash.
//!   - To resolve lazy commits via"RemoteIdConvertProtocol".
//!
//! Currently mainly used by `virtual-repo` to construct synthetic repos.

use std::sync::Arc;

use dag::protocol::RemoteIdConvertProtocol;
use format_util::git_sha1_deserialize;
use format_util::hg_sha1_deserialize;
use minibytes::Bytes;
use types::Id20;
use types::SerializationFormat;

/// Extends the EagerRepo's object store and commit graph.
pub trait EagerRepoExtension: Send + Sync + 'static {
    /// Get the blob by (faked) SHA1 hash, with the SHA1 prefixes (git object
    /// type & size, or hg p1 p2).
    fn get_sha1_blob(&self, id: Id20) -> Option<Bytes>;

    /// Get the blob by SHA1 hash, without the SHA1 prefixes (git object type &
    /// size, or hg p1 p2). This method is an optimization to avoid full
    /// `get_sha1_blob` overhead.
    fn get_content(&self, id: Id20) -> Option<Bytes> {
        if id.is_null() {
            return Some(Bytes::default());
        }
        // Use `get_sha1_blob` to answer `get_content`.
        let data = self.get_sha1_blob(id)?;
        let content = match self.format() {
            SerializationFormat::Hg => hg_sha1_deserialize(&data).ok()?.0,
            SerializationFormat::Git => git_sha1_deserialize(&data).ok()?.0,
        };
        Some(data.slice_to_bytes(content))
    }

    /// Used by `get_content` default implementation.
    /// This should match EagerRepo's format.
    fn format(&self) -> SerializationFormat;

    /// Useful to support lazy commit hashes. For example,
    /// `virtual-repo` might want to add millions of (lazy) commits as a segment,
    /// by `dag.import_pull_data`. It does not want O(N) complexity specifying the
    /// commit hashes one by one.
    fn get_dag_remote_protocol(&self) -> Option<Arc<dyn RemoteIdConvertProtocol>>;

    /// The name of the extension.
    fn name(&self) -> &'static str;
}
