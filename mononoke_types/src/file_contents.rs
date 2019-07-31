// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt::{self, Debug};

use bytes::Bytes;
use failure_ext::{bail_err, chain::*};
use quickcheck::{single_shrinker, Arbitrary, Gen};
use rust_thrift::compact_protocol;

use crate::{
    blob::{Blob, BlobstoreBytes, BlobstoreValue, ContentBlob, ContentMetadataBlob},
    errors::*,
    hash, thrift,
    typed_hash::{ContentId, ContentIdContext, ContentMetadataId},
};

/// An enum representing contents for a file.
#[derive(Clone, Eq, PartialEq)]
pub enum FileContents {
    /// Raw contents of the file
    Bytes(Bytes),
    /// Reference to separate FileContents that need to be joined together to produce this file
    //again.
    Chunked((ContentId, Vec<ContentId>)),
}

impl FileContents {
    pub fn new_bytes<B: Into<Bytes>>(b: B) -> Self {
        FileContents::Bytes(b.into())
    }

    pub(crate) fn from_thrift(fc: thrift::FileContents) -> Result<Self> {
        match fc {
            thrift::FileContents::Bytes(bytes) => Ok(FileContents::Bytes(bytes.into())),
            thrift::FileContents::Chunked(chunked) => {
                let content_id = ContentId::from_thrift(chunked.content_id);

                let chunks: Result<Vec<_>> = chunked
                    .chunks
                    .into_iter()
                    .map(ContentId::from_thrift)
                    .collect();

                Ok(FileContents::Chunked((content_id?, chunks?)))
            }
            thrift::FileContents::UnknownField(x) => bail_err!(ErrorKind::InvalidThrift(
                "FileContents".into(),
                format!("unknown file contents field: {}", x)
            )),
        }
    }

    pub fn size(&self) -> usize {
        match *self {
            FileContents::Bytes(ref bytes) => bytes.len(),
            FileContents::Chunked(_) => unimplemented!(), // NOTE: Fixed later in this stack.
        }
    }

    /// Whether this starts with a particular string.
    #[inline]
    pub fn starts_with(&self, needle: &[u8]) -> bool {
        match self {
            FileContents::Bytes(b) => b.starts_with(needle),
            FileContents::Chunked(_) => unimplemented!(), // NOTE: Fixed later in this stack.
        }
    }

    pub fn into_bytes(self) -> Bytes {
        match self {
            FileContents::Bytes(bytes) => bytes,
            FileContents::Chunked(_) => unimplemented!(), // NOTE: Fixed later in this stack.
        }
    }

    pub fn as_bytes(&self) -> &Bytes {
        match self {
            FileContents::Bytes(bytes) => &bytes,
            FileContents::Chunked(_) => unimplemented!(), // NOTE: Fixed later in this stack.
        }
    }

    pub(crate) fn into_thrift(self) -> thrift::FileContents {
        match self {
            // TODO (T26959816) -- allow Thrift to represent binary as Bytes
            FileContents::Bytes(bytes) => thrift::FileContents::Bytes(bytes.to_vec()),
            FileContents::Chunked((content_id, chunks)) => {
                let content_id = content_id.into_thrift();
                let chunks = chunks.into_iter().map(ContentId::into_thrift).collect();
                let chunked = thrift::ChunkedFileContents { content_id, chunks };
                thrift::FileContents::Chunked(chunked)
            }
        }
    }

    pub fn from_encoded_bytes(encoded_bytes: Bytes) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(encoded_bytes.as_ref())
            .chain_err(ErrorKind::BlobDeserializeError("FileContents".into()))?;
        Self::from_thrift(thrift_tc)
    }
}

impl BlobstoreValue for FileContents {
    type Key = ContentId;

    fn into_blob(self) -> ContentBlob {
        let id = match &self {
            FileContents::Bytes(bytes) => {
                let mut context = ContentIdContext::new();
                context.update(&bytes);
                context.finish()
            }
            FileContents::Chunked((content_id, _)) => *content_id,
        };

        let thrift = self.into_thrift();
        let data = compact_protocol::serialize(&thrift);

        Blob::new(id, data)
    }

    fn from_blob(blob: ContentBlob) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(blob.data().as_ref())
            .chain_err(ErrorKind::BlobDeserializeError("FileContents".into()))?;
        Self::from_thrift(thrift_tc)
    }
}

impl Debug for FileContents {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            FileContents::Bytes(ref bytes) => {
                write!(f, "FileContents::Bytes(length {})", bytes.len())
            }
            FileContents::Chunked((ref content_id, ref chunks)) => write!(
                f,
                "FileContents::Chunked({}, chunks {})",
                content_id,
                chunks.len()
            ),
        }
    }
}

impl Arbitrary for FileContents {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        FileContents::new_bytes(Vec::arbitrary(g))
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        single_shrinker(FileContents::new_bytes(vec![]))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ContentAlias(ContentId);

impl ContentAlias {
    pub fn from_content_id(id: ContentId) -> Self {
        ContentAlias(id)
    }

    pub fn from_bytes(blob: Bytes) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(blob.as_ref())
            .chain_err(ErrorKind::BlobDeserializeError("ContentAlias".into()))?;
        Self::from_thrift(thrift_tc)
    }

    pub fn from_thrift(ca: thrift::ContentAlias) -> Result<Self> {
        match ca {
            thrift::ContentAlias::ContentId(id) => {
                Ok(Self::from_content_id(ContentId::from_thrift(id)?))
            }
            thrift::ContentAlias::UnknownField(x) => bail_err!(ErrorKind::InvalidThrift(
                "ContentAlias".into(),
                format!("unknown content alias field: {}", x)
            )),
        }
    }

    pub fn into_blob(self) -> BlobstoreBytes {
        let alias = thrift::ContentAlias::ContentId(self.0.into_thrift());
        BlobstoreBytes::from_bytes(compact_protocol::serialize(&alias))
    }

    pub fn content_id(&self) -> ContentId {
        self.0
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ContentMetadata {
    pub total_size: u64,
    pub content_id: ContentId,
    pub sha1: Option<hash::Sha1>,
    pub sha256: Option<hash::Sha256>,
    pub git_sha1: Option<hash::GitSha1>,
}

impl ContentMetadata {
    pub fn from_thrift(cab: thrift::ContentMetadata) -> Result<Self> {
        let total_size = if cab.total_size >= 0 {
            cab.total_size as u64
        } else {
            bail_err!(ErrorKind::InvalidThrift(
                "ContentMetadata".into(),
                format!("Missing or negative size field ({})", cab.total_size)
            ));
        };

        let res = ContentMetadata {
            total_size,
            content_id: ContentId::from_thrift(cab.content_id)?,
            sha1: cab.sha1.map(|v| hash::Sha1::from_bytes(&v.0)).transpose()?,
            sha256: cab
                .sha256
                .map(|v| hash::Sha256::from_bytes(&v.0))
                .transpose()?,
            git_sha1: cab
                .git_sha1
                .map(|v| hash::GitSha1::from_bytes(&v.0, "blob", total_size))
                .transpose()?,
        };

        Ok(res)
    }

    fn into_thrift(self) -> thrift::ContentMetadata {
        thrift::ContentMetadata {
            total_size: self.total_size as i64,
            content_id: self.content_id.into_thrift(),
            sha1: self.sha1.map(|sha1| sha1.into_thrift()),
            git_sha1: self.git_sha1.map(|git_sha1| git_sha1.into_thrift()),
            sha256: self.sha256.map(|sha256| sha256.into_thrift()),
        }
    }
}

impl BlobstoreValue for ContentMetadata {
    type Key = ContentMetadataId;

    fn into_blob(self) -> ContentMetadataBlob {
        let id = From::from(self.content_id.clone());
        let thrift = self.into_thrift();
        let data = compact_protocol::serialize(&thrift);
        Blob::new(id, data)
    }

    fn from_blob(blob: ContentMetadataBlob) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(blob.data().as_ref())
            .chain_err(ErrorKind::BlobDeserializeError("ContentMetadata".into()))?;
        Self::from_thrift(thrift_tc)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use quickcheck::quickcheck;

    quickcheck! {
        fn thrift_roundtrip(fc: FileContents) -> bool {
            let thrift_fc = fc.clone().into_thrift();
            let fc2 = FileContents::from_thrift(thrift_fc)
                .expect("thrift roundtrips should always be valid");
            fc == fc2
        }

        fn blob_roundtrip(cs: FileContents) -> bool {
            let blob = cs.clone().into_blob();
            let cs2 = FileContents::from_blob(blob)
                .expect("blob roundtrips should always be valid");
            cs == cs2
        }
    }

    #[test]
    fn bad_thrift() {
        let thrift_fc = thrift::FileContents::UnknownField(-1);
        FileContents::from_thrift(thrift_fc).expect_err("unexpected OK - unknown field");
    }
}
