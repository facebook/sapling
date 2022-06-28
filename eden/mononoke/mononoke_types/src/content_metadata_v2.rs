/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use fbthrift::compact_protocol;
use quickcheck::Arbitrary;
use quickcheck::Gen;

use crate::blob::Blob;
use crate::blob::BlobstoreValue;
use crate::blob::ContentMetadataV2Blob;
use crate::errors::ErrorKind;
use crate::hash;
use crate::thrift;
use crate::thrift_field;
use crate::typed_hash::ContentId;
use crate::typed_hash::ContentMetadataV2Id;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ContentMetadataV2 {
    pub content_id: ContentId,
    pub total_size: u64,
    pub sha1: hash::Sha1,
    pub sha256: hash::Sha256,
    pub git_sha1: hash::RichGitSha1,
    pub is_binary: bool,
    pub is_ascii: bool,
    pub is_utf8: bool,
    pub ends_in_newline: bool,
    pub newline_count: u64,
    pub first_line: Option<String>,
}

impl ContentMetadataV2 {
    pub fn from_thrift(metadata: thrift::ContentMetadataV2) -> Result<Self> {
        let newline_count = thrift_field!(ContentMetadataV2, metadata, newline_count)?;
        let newline_count: u64 = newline_count.try_into()?;

        let total_size = thrift_field!(ContentMetadataV2, metadata, total_size)?;
        let total_size: u64 = total_size.try_into()?;

        let res = ContentMetadataV2 {
            newline_count,
            total_size,
            sha1: hash::Sha1::from_bytes(&thrift_field!(ContentMetadataV2, metadata, sha1)?.0)?,
            sha256: hash::Sha256::from_bytes(
                &thrift_field!(ContentMetadataV2, metadata, sha256)?.0,
            )?,
            git_sha1: hash::RichGitSha1::from_bytes(
                &thrift_field!(ContentMetadataV2, metadata, git_sha1)?.0,
                "blob",
                total_size,
            )?,
            content_id: ContentId::from_thrift(thrift_field!(
                ContentMetadataV2,
                metadata,
                content_id
            )?)?,
            is_binary: thrift_field!(ContentMetadataV2, metadata, is_binary)?,
            is_ascii: thrift_field!(ContentMetadataV2, metadata, is_ascii)?,
            is_utf8: thrift_field!(ContentMetadataV2, metadata, is_utf8)?,
            ends_in_newline: thrift_field!(ContentMetadataV2, metadata, ends_in_newline)?,
            first_line: metadata.first_line,
        };

        Ok(res)
    }

    fn into_thrift(self) -> thrift::ContentMetadataV2 {
        thrift::ContentMetadataV2 {
            content_id: Some(self.content_id.into_thrift()),
            newline_count: Some(self.newline_count as i64),
            total_size: Some(self.total_size as i64),
            is_binary: Some(self.is_binary),
            is_ascii: Some(self.is_ascii),
            is_utf8: Some(self.is_utf8),
            ends_in_newline: Some(self.ends_in_newline),
            sha1: Some(self.sha1.into_thrift()),
            git_sha1: Some(self.git_sha1.into_thrift()),
            sha256: Some(self.sha256.into_thrift()),
            first_line: self.first_line,
        }
    }
}

impl Arbitrary for ContentMetadataV2 {
    fn arbitrary(g: &mut Gen) -> Self {
        // Large u64 values can't be represented in thrift
        let total_size = u64::arbitrary(g) / 2;
        Self {
            total_size,
            newline_count: u64::arbitrary(g) / 2,
            content_id: ContentId::arbitrary(g),
            is_binary: bool::arbitrary(g),
            is_ascii: bool::arbitrary(g),
            is_utf8: bool::arbitrary(g),
            ends_in_newline: bool::arbitrary(g),
            sha1: hash::Sha1::arbitrary(g),
            sha256: hash::Sha256::arbitrary(g),
            git_sha1: hash::RichGitSha1::from_sha1(hash::GitSha1::arbitrary(g), "blob", total_size),
            first_line: Option::arbitrary(g),
        }
    }
}

impl BlobstoreValue for ContentMetadataV2 {
    type Key = ContentMetadataV2Id;

    fn into_blob(self) -> ContentMetadataV2Blob {
        let id = From::from(self.content_id.clone());
        let thrift = self.into_thrift();
        let data = compact_protocol::serialize(&thrift);
        Blob::new(id, data)
    }

    fn from_blob(blob: ContentMetadataV2Blob) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(blob.data().as_ref())
            .with_context(|| ErrorKind::BlobDeserializeError("ContentMetadataV2".into()))?;
        Self::from_thrift(thrift_tc)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use quickcheck::quickcheck;

    quickcheck! {
        fn content_metadata_v2_thrift_roundtrip(metadata: ContentMetadataV2) -> bool {
            let thrift_metadata = metadata.clone().into_thrift();
            let from_thrift_metadata = ContentMetadataV2::from_thrift(thrift_metadata)
                .expect("thrift roundtrips should always be valid");
            println!("metadata: {:?}", metadata);
            println!("metadata_from_thrift: {:?}", from_thrift_metadata);
            metadata == from_thrift_metadata
        }

        fn content_metadata_v2_blob_roundtrip(metadata: ContentMetadataV2) -> bool {
            let blob = metadata.clone().into_blob();
            let metadata_from_blob = ContentMetadataV2::from_blob(blob)
                .expect("blob roundtrips should always be valid");
            metadata == metadata_from_blob
        }
    }
}
