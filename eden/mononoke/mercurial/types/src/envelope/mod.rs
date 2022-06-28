/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// Containers for Mercurial data, stored in the blob store.

mod changeset_envelope;
mod file_envelope;
mod manifest_envelope;

pub use self::changeset_envelope::HgChangesetEnvelope;
pub use self::changeset_envelope::HgChangesetEnvelopeMut;
pub use self::file_envelope::HgFileEnvelope;
pub use self::file_envelope::HgFileEnvelopeMut;
pub use self::manifest_envelope::HgManifestEnvelope;
pub use self::manifest_envelope::HgManifestEnvelopeMut;

use blobstore::BlobstoreGetData;
use mononoke_types::BlobstoreBytes;

use bytes::Bytes;

#[derive(Clone, Debug)]
pub struct HgEnvelopeBlob(Bytes);

impl From<BlobstoreBytes> for HgEnvelopeBlob {
    #[inline]
    fn from(bytes: BlobstoreBytes) -> HgEnvelopeBlob {
        HgEnvelopeBlob(bytes.into_bytes())
    }
}

impl From<HgEnvelopeBlob> for BlobstoreBytes {
    #[inline]
    fn from(blob: HgEnvelopeBlob) -> BlobstoreBytes {
        BlobstoreBytes::from_bytes(blob.0)
    }
}

impl From<BlobstoreGetData> for HgEnvelopeBlob {
    #[inline]
    fn from(blob_val: BlobstoreGetData) -> HgEnvelopeBlob {
        HgEnvelopeBlob(blob_val.into_raw_bytes())
    }
}

impl From<HgEnvelopeBlob> for BlobstoreGetData {
    #[inline]
    fn from(blob: HgEnvelopeBlob) -> BlobstoreGetData {
        BlobstoreBytes::from(blob).into()
    }
}
