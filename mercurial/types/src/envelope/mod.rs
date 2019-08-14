// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// Containers for Mercurial data, stored in the blob store.

mod changeset_envelope;
mod file_envelope;
mod manifest_envelope;

pub use self::changeset_envelope::{HgChangesetEnvelope, HgChangesetEnvelopeMut};
pub use self::file_envelope::{HgFileEnvelope, HgFileEnvelopeMut};
pub use self::manifest_envelope::{HgManifestEnvelope, HgManifestEnvelopeMut};

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
