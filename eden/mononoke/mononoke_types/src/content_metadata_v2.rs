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
    pub is_generated: bool,
    pub is_partially_generated: bool,
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
            is_generated: thrift_field!(ContentMetadataV2, metadata, is_generated)?,
            is_partially_generated: thrift_field!(
                ContentMetadataV2,
                metadata,
                is_partially_generated
            )?,
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
            is_generated: Some(self.is_generated),
            is_partially_generated: Some(self.is_partially_generated),
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
            is_generated: bool::arbitrary(g),
            is_partially_generated: bool::arbitrary(g),
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
    pub is_generated: bool,
    pub is_partially_generated: bool,
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

async fn contains_marker(bytes_stream: impl Stream<Item = Bytes>, marker: &str) -> bool {
    let output = bytes_stream
        .fold(
            FoldState::InProgress(Bytes::new()),
            |acc, bytes| async move {
                match acc {
                    FoldState::Done(_) => acc,
                    FoldState::InProgress(ref rem_bytes) => {
                        // Before processing the current chunk, prepend the carry-on bytes from the last
                        // chunk to handle cases of the marker splitting across chunks.
                        let bytes = [rem_bytes, bytes.as_ref()].concat();
                        match std::str::from_utf8(bytes.as_ref()) {
                            Ok(content) => match content.contains(marker) {
                                // The marker is present in the file content. Consider this file
                                // as generated.
                                true => FoldState::Done(true),
                                // The marker was not present. Before processing the next chunk, we need to
                                // consider scenarios where the marker was split across chunks, for example
                                // [@gener]..[ated]. There are multiple variants of this split and to accommodate
                                // all cases, we will pick the last N chars where N = len of marker (i.e. skip
                                // first M - N chars where M is the number of chars in content) and carry it
                                // to the next chunk where it will be prepended before processing it.
                                false => {
                                    let chars = content.chars();
                                    let skip_count: i32 = std::cmp::max(
                                        chars.count() as i32 - marker.len() as i32,
                                        0,
                                    );
                                    let rem_str: String =
                                        content.chars().skip(skip_count as usize).collect();
                                    FoldState::InProgress(Bytes::copy_from_slice(
                                        rem_str.as_bytes(),
                                    ))
                                }
                            },
                            Err(error) => {
                                let (valid, invalid) = bytes.split_at(error.valid_up_to());
                                // We know that the slice is valid UTF-8 by this point, so safe to do the below.
                                let valid = unsafe { std::str::from_utf8_unchecked(valid) };
                                // If the length of invalid slice is more than a UTF8 codepoint
                                // then the file isn't UTF-8 encoded. A non-UTF-8 file cannot
                                // be parsed for checking the presence of marker.
                                if invalid.len() > UTF8_BYTES_COUNT {
                                    FoldState::Done(false)
                                // The UTF-8 valid part of the content contains the required
                                // marker. The check is completed successfully.
                                } else if valid.contains(marker) {
                                    FoldState::Done(true)
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
        // Check continued till the last chunk and the generated marker was not found
        // till the last chunk. Return false.
        FoldState::InProgress(_) => false,
        // Check completed with either true or false status. Return the status.
        FoldState::Done(status) => status,
    }
}

pub async fn is_generated(bytes_stream: impl Stream<Item = Bytes>) -> bool {
    contains_marker(bytes_stream, concat!("@", "generated")).await
}

pub async fn is_partially_generated(bytes_stream: impl Stream<Item = Bytes>) -> bool {
    contains_marker(bytes_stream, concat!("@", "partially-generated")).await
}

#[cfg(test)]
mod test {
    use futures::future;
    use futures::stream;
    use quickcheck::quickcheck;
    use rand::distributions::Alphanumeric;
    use rand::distributions::Distribution;
    use rand::distributions::Standard;
    use rand::thread_rng;
    use rand::Rng;

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

    // is_ascii tests
    #[tokio::test]
    async fn basic_is_ascii_test() {
        let input = "This is a sample ASCII_string@#$()&^/';[]`~*";
        let bytes_stream = stream::once(future::ready(Bytes::from(input)));
        assert!(
            is_ascii(bytes_stream).await,
            "The input '{}' wasn't ASCII",
            input
        )
    }

    #[tokio::test]
    async fn negative_is_ascii_test() {
        let input = "यह एक नमूना गैर-ASCII स्ट्रिंग है";
        let bytes_stream = stream::once(future::ready(Bytes::from(input)));
        assert!(
            !is_ascii(bytes_stream).await,
            "The input '{}' was ASCII",
            input
        )
    }

    #[tokio::test]
    async fn single_non_ascii_codepoint_test() {
        let input = "This is almost an ASCII ह string";
        let bytes_stream = stream::once(future::ready(Bytes::from(input)));
        assert!(
            !is_ascii(bytes_stream).await,
            "The input '{}' was ASCII",
            input
        )
    }

    #[tokio::test]
    async fn arbitrary_is_ascii_test() {
        let bytes = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(1024)
            .collect::<Bytes>();
        let bytes_stream = stream::once(future::ready(bytes));
        assert!(is_ascii(bytes_stream).await);
    }

    #[tokio::test]
    async fn arbitrary_stream_is_ascii_test() {
        let mut rng = thread_rng();
        let bytes_stream = stream::iter((0..50).map(|_| {
            let chunk_size: usize = rng.gen_range(20..50);
            Alphanumeric
                .sample_iter(&mut rng)
                .take(chunk_size)
                .collect::<Bytes>()
        }));
        assert!(is_ascii(bytes_stream).await);
    }

    #[tokio::test]
    async fn single_string_single_stream_is_ascii_test() {
        let mut rng = thread_rng();
        let bytes = Alphanumeric
            .sample_iter(&mut rng)
            .take(4096)
            .collect::<Bytes>();
        let bytes_stream = stream::iter(bytes.chunks(37).into_iter().map(Bytes::copy_from_slice));
        assert!(is_ascii(bytes_stream).await);
    }

    #[tokio::test]
    async fn single_string_multiple_stream_is_ascii_test() {
        let mut rng = thread_rng();
        let bytes = Alphanumeric
            .sample_iter(&mut rng)
            .take(4096)
            .collect::<Bytes>();
        for chunk in [230, 10, 35, 89, 1000] {
            let bytes_stream =
                stream::iter(bytes.chunks(chunk).into_iter().map(Bytes::copy_from_slice));
            assert!(is_ascii(bytes_stream).await);
        }
    }

    // is_utf8 tests
    #[tokio::test]
    async fn basic_is_utf8_test() {
        let input =
            "This is a sample UTF8 encoded _string_ @#$()&^/';[]`~*. यह एक नमूना UTF8 स्ट्रिंग है 😋";
        let bytes_stream = stream::once(future::ready(Bytes::from(input)));
        assert!(
            is_utf8(bytes_stream).await,
            "The input '{}' wasn't UTF8",
            input
        )
    }

    #[tokio::test]
    async fn negative_is_utf8_test() {
        let bytes = b"C\xF4te d'Ivoire";
        let bytes_stream = stream::once(future::ready(Bytes::from_static(bytes)));
        assert!(!is_utf8(bytes_stream).await)
    }

    #[tokio::test]
    async fn arbitrary_is_utf8_test() {
        let bytes = Bytes::from(
            thread_rng()
                .sample_iter::<char, _>(&Standard)
                .take(1024)
                .collect::<String>(),
        );
        let bytes_stream = stream::once(future::ready(bytes));
        assert!(is_utf8(bytes_stream).await);
    }

    #[tokio::test]
    async fn arbitrary_negative_is_utf8_test() {
        let bytes = thread_rng()
            .sample_iter(&Standard)
            .take(1024)
            .collect::<Bytes>();
        let bytes_stream = stream::once(future::ready(bytes));
        let bytes_stream = bytes_stream.chain(stream::once(future::ready(Bytes::from_static(
            b"C\xF4te d'Ivoire",
        ))));
        assert!(!is_utf8(bytes_stream).await);
    }

    #[tokio::test]
    async fn arbitrary_stream_is_utf8_test() {
        let bytes_stream = stream::iter((0..50).map(|_| {
            let chunk_size: usize = thread_rng().gen_range(20..50);
            Bytes::from(
                thread_rng()
                    .sample_iter::<char, _>(&Standard)
                    .take(chunk_size)
                    .collect::<String>(),
            )
        }));
        assert!(is_utf8(bytes_stream).await);
    }

    #[tokio::test]
    async fn single_string_single_stream_is_utf8_test() {
        let bytes = Bytes::from(
            thread_rng()
                .sample_iter::<char, _>(&Standard)
                .take(4096)
                .collect::<String>(),
        );
        let bytes_stream = stream::iter(bytes.chunks(37).into_iter().map(Bytes::copy_from_slice));
        assert!(is_utf8(bytes_stream).await);
    }

    #[tokio::test]
    async fn single_string_multiple_stream_is_utf8_test() {
        let bytes = Bytes::from(
            thread_rng()
                .sample_iter::<char, _>(&Standard)
                .take(4096)
                .collect::<String>(),
        );
        for chunk in [230, 10, 35, 89, 1000] {
            let bytes_stream =
                stream::iter(bytes.chunks(chunk).into_iter().map(Bytes::copy_from_slice));
            assert!(is_utf8(bytes_stream).await);
        }
    }

    // ends_in_newline tests
    #[tokio::test]
    async fn basic_ends_in_newline_test() {
        let input = "Random string ending in newline\n";
        let bytes_stream = stream::once(future::ready(Bytes::from(input)));
        assert!(ends_in_newline(bytes_stream).await);
    }

    #[tokio::test]
    async fn negative_ends_in_newline_test() {
        let input = "Just some string";
        let bytes_stream = stream::once(future::ready(Bytes::from(input)));
        assert!(!ends_in_newline(bytes_stream).await);
    }

    #[tokio::test]
    async fn non_ending_newlines_test() {
        let input = "\nThere are \n newlines in \n this string \nbut not at the en\nd";
        let bytes_stream = stream::once(future::ready(Bytes::from(input)));
        assert!(!ends_in_newline(bytes_stream).await);
    }

    #[tokio::test]
    async fn ends_in_newline_with_stream_test() {
        let bytes_stream = stream::iter(
            ["This is a", " broken up", " string that ends in newline\n"]
                .into_iter()
                .map(Bytes::from),
        );
        assert!(ends_in_newline(bytes_stream).await);
    }

    #[tokio::test]
    async fn ends_in_newline_with_invalid_stream_test() {
        let bytes_stream = stream::iter(
            [
                "Each chunk\n",
                " of this string\n",
                " ends in newline\n",
                " except the last",
            ]
            .into_iter()
            .map(Bytes::from),
        );
        assert!(!ends_in_newline(bytes_stream).await);
    }

    #[tokio::test]
    async fn ends_in_newline_with_arbitrary_non_ascii_stream_test() {
        let bytes_stream = stream::iter(
            [
                "इस पाठ का प्रत्येक हिस्सा",
                "अंग्रेजी वाक्य नहीं है",
                "इसलिए इसमें कोई न्यूलाइन नहीं होनी चाहिए।",
            ]
            .into_iter()
            .map(Bytes::from),
        );
        assert!(!ends_in_newline(bytes_stream).await);
    }

    // newline_count tests
    #[tokio::test]
    async fn basic_newline_count_test() {
        let input = "Random\n string with\n newline\n embedded in bet\nween\n";
        let bytes_stream = stream::once(future::ready(Bytes::from(input)));
        assert_eq!(5, newline_count(bytes_stream).await, "Expected 5 newlines");
    }

    #[tokio::test]
    async fn no_newline_count_test() {
        let input = "Random string with no newlines";
        let bytes_stream = stream::once(future::ready(Bytes::from(input)));
        assert_eq!(0, newline_count(bytes_stream).await, "Expected 0 newlines");
    }

    #[tokio::test]
    async fn stream_newline_count_test() {
        let bytes_stream = stream::iter(
            [
                "This chunk has\n newline",
                "This chunk doesn't",
                "Neither does this",
                "This\n one\n has\n four\n",
                "\n",
            ]
            .into_iter()
            .map(Bytes::from),
        );
        assert_eq!(6, newline_count(bytes_stream).await, "Expected 6 newlines");
    }

    #[tokio::test]
    async fn no_newline_count_stream_test() {
        let bytes_stream = stream::iter(
            ["No", "newlines", "in", "this", "stream"]
                .into_iter()
                .map(Bytes::from),
        );
        assert_eq!(0, newline_count(bytes_stream).await, "Expected 0 newlines");
    }

    // is_binary tests
    #[tokio::test]
    async fn basic_is_binary_test() {
        let input = b"Binary input with \0 byte";
        let bytes_stream = stream::once(future::ready(Bytes::from_static(input)));
        assert!(is_binary(bytes_stream).await);
    }

    #[tokio::test]
    async fn negative_is_binary_test() {
        let input = b"Just a regular string";
        let bytes_stream = stream::once(future::ready(Bytes::from_static(input)));
        assert!(!is_binary(bytes_stream).await);
    }

    #[tokio::test]
    async fn stream_is_binary_test() {
        let bytes_stream = stream::iter(
            [
                b"This is just text",
                b"So is this       ",
                b"But not \0 this   ",
                b"not even \0 this  ",
            ]
            .iter()
            .map(|&b| Bytes::from_static(b)),
        );
        assert!(is_binary(bytes_stream).await);
    }

    #[tokio::test]
    async fn stream_negative_is_binary_test() {
        let bytes_stream = stream::iter(
            [
                b"This is just text",
                b"This is just text",
                b"This is just text",
                b"This is just text",
            ]
            .iter()
            .map(|&b| Bytes::from_static(b)),
        );
        assert!(!is_binary(bytes_stream).await);
    }
}
