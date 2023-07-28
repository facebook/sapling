/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use bytes::Bytes;
use git_hash::oid;
use git_hash::ObjectId;
use git_object::Object;
use git_object::ObjectRef;
use git_object::WriteTo;

/// The type of items that can be present in a Git packfile. Note that this
/// does not contain OffsetDelta and RefDelta for now. Those will be generated
/// by the packing library when applicable.
/// See: https://fburl.com/1yaui1um
#[allow(dead_code)]
pub struct PackfileItem {
    object: Object,
    hash: ObjectId,
}

impl PackfileItem {
    /// Creates a new packfile item from the object bytes and the hash of the Git object.
    #[allow(dead_code)]
    pub fn new(object_bytes: Bytes, hash_bytes: Bytes) -> anyhow::Result<Self> {
        let object = ObjectRef::from_loose(object_bytes.as_ref())
            .map_err(|e| anyhow::anyhow!("Failed to parse packfile item: {}", e))?
            .into();
        let hash = oid::try_from_bytes(hash_bytes.as_ref())
            .context("Failed to convert packfile item hash to Git Object ID")?
            .into();
        Ok(Self { object, hash })
    }

    /// The uncompressed size of the Git object contained within the pack item.
    #[allow(dead_code)]
    pub fn size(&self) -> usize {
        self.object.size()
    }

    /// The 20-byte SHA1 hash (ObjectId) of the Git object contained within the
    /// pack item.
    #[allow(dead_code)]
    pub fn hash(&self) -> &git_hash::oid {
        self.hash.as_ref()
    }
}
