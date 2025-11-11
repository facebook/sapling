/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! # id20store-trait
//!
//! Trait to make Id20Store provide more objects (commits, trees, blobs)
//! without actually storing the objects.
//!
//! Provide interfaces to:
//! - Extend EagerRepo's SHA1-like object store to answer more Id20 lookups.
//!
//! Currently mainly used by `virtual-repo` to construct synthetic repos.

use format_util::strip_sha1_header;
use minibytes::Bytes;
use types::Id20;
use types::SerializationFormat;

/// Extends the Id20Store's object store.
pub trait Id20StoreExtension: Send + Sync + 'static {
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
        strip_sha1_header(&data, self.format()).ok()
    }

    /// Used by `get_content` default implementation.
    /// This should match EagerRepo's format.
    fn format(&self) -> SerializationFormat;

    /// The name of the extension.
    fn name(&self) -> &'static str;
}
