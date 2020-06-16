/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobstore::BlobstoreGetData;
use fbthrift::compact_protocol;
use mononoke_types::BlobstoreBytes;
use packblob_thrift::StorageEnvelope;
use std::convert::{TryFrom, TryInto};

// new type so can implement conversions
pub(crate) struct PackEnvelope(pub packblob_thrift::StorageEnvelope);

impl TryFrom<BlobstoreBytes> for PackEnvelope {
    type Error = Error;

    fn try_from(bytes: BlobstoreBytes) -> Result<Self, Error> {
        let t: StorageEnvelope = compact_protocol::deserialize(bytes.as_bytes().as_ref())?;
        Ok(PackEnvelope(t))
    }
}

impl Into<BlobstoreBytes> for PackEnvelope {
    fn into(self) -> BlobstoreBytes {
        let data = compact_protocol::serialize(&self.0);
        BlobstoreBytes::from_bytes(data)
    }
}

impl TryFrom<BlobstoreGetData> for PackEnvelope {
    type Error = Error;

    fn try_from(blob: BlobstoreGetData) -> Result<Self, Error> {
        blob.into_bytes().try_into()
    }
}

impl Into<BlobstoreGetData> for PackEnvelope {
    fn into(self) -> BlobstoreGetData {
        Into::<BlobstoreBytes>::into(self).into()
    }
}
