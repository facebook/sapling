/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use bytes::Bytes;
use fbthrift::compact_protocol;
use futures::Stream;
use futures::StreamExt;
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

const MAX_BYTES_FOR_FIRST_LINE: usize = 64;
const UTF8_BYTES_COUNT: usize = 8;

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

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PartialMetadata {
    pub is_binary: bool,
    pub is_ascii: bool,
    pub is_utf8: bool,
    pub ends_in_newline: bool,
    pub newline_count: u64,
    pub first_line: Option<String>,
}

enum FoldState<T> {
    InProgress(T),
    Done(bool),
}

/// Computes if the entire stream of bytes is valid UTF-8 encoded.
pub async fn is_utf8(bytes_stream: impl Stream<Item = Bytes>) -> bool {
    let output = bytes_stream
        .fold(
            FoldState::InProgress(Bytes::new()),
            |acc, bytes| async move {
                match acc {
                    FoldState::Done(_) => acc,
                    FoldState::InProgress(ref rem_bytes) => {
                        let bytes = [rem_bytes, bytes.as_ref()].concat();
                        match std::str::from_utf8(bytes.as_ref()) {
                            // The entire chunk was valid UTF8, carry on to the next chunk.
                            Ok(_) => FoldState::InProgress(Bytes::new()),
                            Err(error) => {
                                let (_, invalid) = bytes.split_at(error.valid_up_to());
                                // If the length of invalid slice is more than a UTF8 codepoint
                                // then the file isn't UTF-8 encoded.
                                if invalid.len() > UTF8_BYTES_COUNT {
                                    FoldState::Done(false)
                                } else {
                                    // The remaining invalid bytes need to be carried over to the next
                                    // chunk to be concatenated with it.
                                    FoldState::InProgress(Bytes::copy_from_slice(invalid))
                                }
                            }
                        }
                    }
                }
            },
        )
        .await;
    match output {
        // Check continued till the last chunk. If the last chunk was valid UTF-8,
        // then 'bytes' would be empty and the entire file would be valid UTF-8
        FoldState::InProgress(ref bytes) => bytes.is_empty(),
        // The UTF8 check was completed before the last chunk with `status` value
        FoldState::Done(status) => status,
    }
}

/// Computes if the entire stream of bytes is valid ASCII.
pub async fn is_ascii(bytes_stream: impl Stream<Item = Bytes>) -> bool {
    // NOTE: This can be achieved in much shorter form by using short-circuiting
    // variants like 'all' or 'any'. However, that leads to Multiplexer error due
    // to the stream getting dropped prematurely.
    let output = bytes_stream
        .fold(FoldState::InProgress(true), |acc, bytes| async move {
            match acc {
                FoldState::Done(_) => acc,
                FoldState::InProgress(val) => {
                    let is_ascii = val && bytes.as_ref().iter().all(u8::is_ascii);
                    if !is_ascii {
                        FoldState::Done(false)
                    } else {
                        FoldState::InProgress(true)
                    }
                }
            }
        })
        .await;
    match output {
        FoldState::InProgress(val) => val,
        FoldState::Done(val) => val,
    }
}

/// Computes if the stream of bytes represents binary content
pub async fn is_binary(bytes_stream: impl Stream<Item = Bytes>) -> bool {
    let output = bytes_stream
        .fold(FoldState::InProgress(false), |acc, bytes| async move {
            match acc {
                FoldState::Done(_) => acc,
                FoldState::InProgress(val) => {
                    let is_binary = val || bytes.as_ref().contains(&b'\0');
                    FoldState::InProgress(is_binary)
                }
            }
        })
        .await;
    match output {
        FoldState::InProgress(val) => val,
        FoldState::Done(val) => val,
    }
}

/// Computes if the entire stream of bytes ends in a newline.
pub async fn ends_in_newline(bytes_stream: impl Stream<Item = Bytes>) -> bool {
    bytes_stream
        .fold(false, |acc, bytes| async move {
            match bytes.as_ref().last() {
                Some(&byte) => byte == b'\n',
                None => acc,
            }
        })
        .await
}

/// Computes the count of newline characters in the entire stream of bytes.
pub async fn newline_count(bytes_stream: impl Stream<Item = Bytes>) -> u64 {
    bytes_stream
        .fold(0, |acc, bytes| async move {
            acc + bytes
                .as_ref()
                .iter()
                .fold(0, |acc, &byte| if byte == b'\n' { acc + 1 } else { acc })
        })
        .await
}

/// Gets the first UTF-8 encoded line OR the first 64 bytes of data from the input
/// data stream, whichever is shortest.
pub async fn first_line(bytes_stream: impl Stream<Item = Bytes>) -> Option<String> {
    let line = bytes_stream
        .fold(
            (String::with_capacity(MAX_BYTES_FOR_FIRST_LINE), false),
            |(mut acc, done), bytes| async move {
                // We already have the first line that we are looking for,
                // no need to look at further data.
                if done || acc.len() >= MAX_BYTES_FOR_FIRST_LINE {
                    (acc, true)
                } else {
                    let valid_line = match std::str::from_utf8(bytes.as_ref()) {
                        Ok(line) => line.lines().next(),
                        Err(error) => {
                            let (valid, invalid) = bytes.split_at(error.valid_up_to());
                            // If the length of invalid slice is more than a UTF8 codepoint
                            // then the file isn't UTF-8 encoded. Return whatever we have
                            // in the accumulator and exit.
                            if invalid.len() > UTF8_BYTES_COUNT {
                                return (acc, true);
                            }
                            // We know that the slice is valid UTF-8 by this point, so safe to do the below.
                            let valid = unsafe { std::str::from_utf8_unchecked(valid) };
                            valid.lines().next()
                        }
                    };
                    let valid_line = match valid_line {
                        Some(line) => line,
                        None => return (acc, true),
                    };
                    let len_to_push =
                        std::cmp::min(MAX_BYTES_FOR_FIRST_LINE - acc.len(), valid_line.len());
                    // Push only till the end of the line or till the end of buffer, whichever is the shortest.
                    acc.push_str(valid_line[..len_to_push].as_ref());
                    (acc, done)
                }
            },
        )
        .await
        .0;
    if line.is_empty() { None } else { Some(line) }
}

#[cfg(test)]
mod test {
    use quickcheck::quickcheck;

    use super::*;

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
