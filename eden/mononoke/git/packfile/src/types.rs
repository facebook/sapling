/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::Context;
use bytes::Bytes;
use bytes::BytesMut;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use git_hash::oid;
use git_hash::ObjectId;
use git_object::Object;
use git_object::ObjectRef;
use git_object::WriteTo;
use sha1::Digest;
use sha1::Sha1;

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
    pub fn new(object_bytes: Bytes) -> anyhow::Result<Self> {
        // Get the hash of the Git object bytes
        let mut hasher = Sha1::new();
        hasher.update(&object_bytes);
        let hash_bytes = hasher.finalize();
        // Create the Git object from raw bytes
        let object = ObjectRef::from_loose(object_bytes.as_ref())
            .map_err(|e| anyhow::anyhow!("Failed to parse packfile item: {}", e))?
            .into();
        let hash = oid::try_from_bytes(hash_bytes.as_ref())
            .context("Failed to convert packfile item hash to Git Object ID")?
            .into();
        // Create the packfile item from the object and the hash
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

    /// Zlib encode the raw bytes of the Git object and write it to `out`.
    #[allow(dead_code)]
    pub fn write_encoded(&self, out: &mut BytesMut) -> anyhow::Result<()> {
        let object_bytes = to_vec_bytes(&self.object)?;
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&object_bytes)
            .context("Failure in writing raw Git object data to ZLib buffer")?;
        let compressed_object = encoder
            .finish()
            .context("Failure in ZLib encoding Git object data")?;
        out.extend(&compressed_object);
        Ok(())
    }
}

pub(crate) fn to_vec_bytes(git_object: &Object) -> anyhow::Result<Vec<u8>> {
    let mut object_bytes = git_object.loose_header().into_vec();
    git_object.write_to(object_bytes.by_ref())?;
    Ok(object_bytes)
}
