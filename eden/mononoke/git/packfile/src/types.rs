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
use gix_hash::oid;
use gix_hash::ObjectId;
use gix_object::Object;
use gix_object::ObjectRef;
use gix_object::WriteTo;
use gix_pack::data::output;
use sha1::Digest;
use sha1::Sha1;

/// The type of items that can be present in a Git packfile. Note that this
/// does not contain OffsetDelta and RefDelta for now. Those will be generated
/// by the packing library when applicable.
/// See: https://fburl.com/1yaui1um
pub struct PackfileItem {
    object: Object,
    hash: ObjectId,
}

impl PackfileItem {
    /// Creates a new packfile item from the raw object bytes of the Git object.
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

    /// The kind of the packfile item. For now all items are Base since we don't delta
    /// objects.
    pub fn kind(&self) -> output::entry::Kind {
        output::entry::Kind::Base(self.object.kind())
    }

    /// The uncompressed size of the Git object contained within the pack item.
    pub fn size(&self) -> usize {
        self.object.size()
    }

    /// The 20-byte SHA1 hash (ObjectId) of the Git object contained within the
    /// pack item.
    pub fn hash(&self) -> &gix_hash::oid {
        self.hash.as_ref()
    }

    /// Zlib encode the raw bytes of the Git object and write it to `out`.
    pub fn write_encoded(&self, out: &mut BytesMut, include_header: bool) -> anyhow::Result<()> {
        let object_bytes = match include_header {
            true => to_vec_bytes(&self.object)?,
            false => to_vec_bytes_without_header(&self.object)?,
        };
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

impl TryFrom<PackfileItem> for output::Entry {
    type Error = anyhow::Error;

    fn try_from(value: PackfileItem) -> Result<Self, Self::Error> {
        let id = value.hash().into();
        let decompressed_size = value.size();
        let kind = value.kind();
        let mut encoded_bytes = BytesMut::new();
        // No need to include the loose format header since the objects are
        // provided a different header format when being included in packfiles.
        value.write_encoded(&mut encoded_bytes, false)?;
        let compressed_data = encoded_bytes.freeze().to_vec();
        let entry = Self {
            id,
            kind,
            decompressed_size,
            compressed_data,
        };
        Ok(entry)
    }
}

/// Free function responsible for writing only the Git object data to a Vec
/// without including the loose format headers
pub(crate) fn to_vec_bytes_without_header(git_object: &Object) -> anyhow::Result<Vec<u8>> {
    let mut object_bytes = Vec::new();
    git_object.write_to(object_bytes.by_ref())?;
    Ok(object_bytes)
}

/// Free function responsible for writing Git object data to a Vec
/// in loose format
pub(crate) fn to_vec_bytes(git_object: &Object) -> anyhow::Result<Vec<u8>> {
    let mut object_bytes = git_object.loose_header().into_vec();
    git_object.write_to(object_bytes.by_ref())?;
    Ok(object_bytes)
}
