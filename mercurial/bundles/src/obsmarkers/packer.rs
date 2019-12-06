/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Packing obsmarkers to be sent after e.g. a pushrebase
//! Format documentation: https://www.mercurial-scm.org/repo/hg/file/tip/mercurial/obsolete.py

use super::MetadataEntry;
use crate::chunk::Chunk;
use crate::errors::*;
use byteorder::ByteOrder;
use bytes::{BigEndian, BufMut};
use futures::stream::iter_result;
use futures::Stream;
use mercurial_types::HgChangesetId;
use mononoke_types::DateTime;
use std::convert::TryFrom;

const VERSION: u8 = 1;

pub fn obsmarkers_packer_stream(
    pairs_stream: impl Stream<Item = (HgChangesetId, Vec<HgChangesetId>), Error = Error>,
    time: DateTime,
    metadata: Vec<MetadataEntry>,
) -> impl Stream<Item = Chunk, Error = Error> {
    let version_chunk = Chunk::new(vec![VERSION]);

    let chunks_stream = pairs_stream.and_then(move |(predecessor, successors)| {
        prepare_obsmarker_chunk(&predecessor, &successors, &time, &metadata)
    });

    iter_result(vec![version_chunk]).chain(chunks_stream)
}

fn prepare_obsmarker_chunk(
    predecessor: &HgChangesetId,
    successors: &Vec<HgChangesetId>,
    time: &DateTime,
    metadata: &Vec<MetadataEntry>,
) -> Result<Chunk> {
    // Reserve space, fill it with zeros before writing out our data.
    let mut v: Vec<u8> = vec![0; 19];

    // 0: length: uint32. We'll write it out later.

    // 4: seconds since epoch: f64
    BigEndian::write_f64(&mut v[4..], time.timestamp_secs() as f64);

    // 12: timezone offset in minutes: int16
    BigEndian::write_i16(&mut v[12..], time.tz_offset_minutes());

    // 14: flags: uint16. We don't have any.

    // 16: number of successors: uint8.
    let new_len = u8::try_from(successors.len())?;
    v[16] = new_len;

    // 17: number of parents: uint8. We set 3, for "no parent data stored".
    v[17] = 3;

    // 18: Number of metadata entries: uint8.
    let metadata_len = u8::try_from(metadata.len())?;
    v[18] = metadata_len;

    // predecessor changeset identifier (20 bytes)
    v.put_slice(predecessor.as_ref());

    // successors changeset identifiers (20 bytes, each)
    for succ in successors.iter() {
        v.put_slice(succ.as_ref());
    }

    let metadata_bytes: Vec<&[u8]> = metadata
        .iter()
        .map(|entry| vec![entry.key.as_bytes(), entry.value.as_bytes()])
        .flatten()
        .collect();

    // Metadata sizes, uint8 each
    for bytes in metadata_bytes.iter() {
        v.put_u8(u8::try_from(bytes.len())?);
    }

    // Metadata keys and values, variable size
    for bytes in metadata_bytes.iter() {
        v.put_slice(bytes);
    }

    // And now, we write out the final length into the first 4 bytes we reserved.
    let len = v.len();
    BigEndian::write_u32(&mut v[0..], u32::try_from(len)?);

    Chunk::new(v)
}

#[cfg(test)]
mod test {
    use super::*;
    use failure_ext::Error;
    use futures::{stream, Async, Poll};
    use futures_ext::StreamExt;
    use mercurial_types_mocks::nodehash;
    use quickcheck::quickcheck;

    fn long_string() -> String {
        String::from_utf8(vec![b'T'; u16::max_value() as usize]).unwrap()
    }

    fn size_matches(data: &[u8]) -> bool {
        (BigEndian::read_u32(data) as usize) == data.len()
    }

    fn metadata_matches(data: &[u8], metadata: &Vec<MetadataEntry>) -> bool {
        let mut off = 0;

        let sizes_ok = metadata.iter().all(|e| {
            let key_size = data[off] as usize;
            off += 1;

            let value_size = data[off] as usize;
            off += 1;

            key_size == e.key.as_bytes().len() && value_size == e.value.as_bytes().len()
        });

        let strings_ok = metadata.iter().all(|e| {
            let key_size = e.key.as_bytes().len();
            let value_size = e.value.as_bytes().len();

            let key = String::from_utf8(data[off..off + key_size].to_vec());
            off += key_size;

            let value = String::from_utf8(data[off..off + value_size].to_vec());
            off += value_size;

            key.is_ok() && key.unwrap() == e.key && value.is_ok() && value.unwrap() == e.value
        });

        sizes_ok && strings_ok
    }

    fn successors_match(data: &[u8], successors: &Vec<HgChangesetId>) -> bool {
        let mut off = 0;

        successors.iter().all(|succ| {
            let end = off + 20;
            let range = off..end;
            off = end;

            HgChangesetId::from_bytes(&data[range]).expect("not a changeset") == *succ
        })
    }

    fn content_matches(
        data: &[u8],
        predecessor: &HgChangesetId,
        successors: &Vec<HgChangesetId>,
        time: &DateTime,
        metadata: &Vec<MetadataEntry>,
    ) -> bool {
        BigEndian::read_f64(&data[4..]) == time.timestamp_secs() as f64
            && BigEndian::read_i16(&data[12..]) == time.tz_offset_minutes()
            && BigEndian::read_i16(&data[14..]) == 0
            && data[16] == (successors.len() as u8)
            && data[17] == 3
            && data[18] == (metadata.len() as u8)
            && HgChangesetId::from_bytes(&data[19..39]).expect("not a changeset") == *predecessor
            && successors_match(&data[39..], &successors)
            && metadata_matches(&data[(39 + 20 * successors.len())..], &metadata)
    }

    quickcheck! {
        fn test_prepare_no_metadata(predecessor: HgChangesetId, successors: Vec<HgChangesetId>, time: DateTime) -> bool {
            let chunk = prepare_obsmarker_chunk(&predecessor, &successors, &time, &vec![]);
            chunk.is_ok() && size_matches(&chunk.unwrap().into_bytes().unwrap())
        }

        fn test_prepare_metadata(predecessor: HgChangesetId, successors: Vec<HgChangesetId>, time: DateTime, metadata: Vec<MetadataEntry>) -> bool {
            let chunk = prepare_obsmarker_chunk(&predecessor, &successors, &time, &metadata);
            let max_size = u8::max_value() as usize;

            if metadata.len() > max_size || successors.len() > max_size {
                // NOTE: With the default quickcheck configuration, we won't exercise this. We
                // actually test this below, but this branch ensures the tests won't fail if
                // we use a larger quickcheck vector size (through QUICKCHECK_GENERATOR_SIZE).
                chunk.is_err()
            } else {
                chunk.is_ok() && size_matches(&chunk.unwrap().into_bytes().unwrap())
            }
        }

        fn test_roundtrip(predecessor: HgChangesetId, successors: Vec<HgChangesetId>, time: DateTime, metadata: Vec<MetadataEntry>) -> bool {
            let metadata = metadata.into_iter().take(4).collect(); // See above
            let chunk = prepare_obsmarker_chunk(&predecessor, &successors, &time, &metadata);
            chunk.is_ok() && content_matches(&chunk.unwrap().into_bytes().unwrap(), &predecessor, &successors, &time, &metadata)
        }
    }

    #[test]
    fn test_no_successors() {
        let time = DateTime::now();
        let successors = vec![];
        let metadata = vec![];
        let chunk = prepare_obsmarker_chunk(&nodehash::ONES_CSID, &successors, &time, &metadata);
        assert!(chunk.is_ok());
    }

    #[test]
    fn test_successor_count_overflow() {
        let time = DateTime::now();
        let successors = vec![nodehash::TWOS_CSID; u16::max_value() as usize];
        let metadata = vec![];
        let chunk = prepare_obsmarker_chunk(&nodehash::ONES_CSID, &successors, &time, &metadata);
        assert!(chunk.is_err());
    }

    #[test]
    fn test_metadata_count_overflow() {
        let entry = MetadataEntry::new("key", "value");
        let time = DateTime::now();
        let metadata = vec![entry; u16::max_value() as usize];
        let chunk = prepare_obsmarker_chunk(
            &nodehash::ONES_CSID,
            &vec![nodehash::TWOS_CSID],
            &time,
            &metadata,
        );
        assert!(chunk.is_err());
    }

    #[test]
    fn test_metadata_key_overflow() {
        let entry = MetadataEntry::new(long_string(), "value");
        let time = DateTime::now();
        let metadata = vec![entry];
        let chunk = prepare_obsmarker_chunk(
            &nodehash::ONES_CSID,
            &vec![nodehash::TWOS_CSID],
            &time,
            &metadata,
        );
        assert!(chunk.is_err());
    }

    #[test]
    fn test_metadata_value_overflow() {
        let entry = MetadataEntry::new("key", long_string());
        let time = DateTime::now();
        let metadata = vec![entry];
        let chunk = prepare_obsmarker_chunk(
            &nodehash::ONES_CSID,
            &vec![nodehash::TWOS_CSID],
            &time,
            &metadata,
        );
        assert!(chunk.is_err());
    }

    fn stream_for_pairs(
        pairs: Vec<(HgChangesetId, Vec<HgChangesetId>)>,
    ) -> impl Stream<Item = (HgChangesetId, Vec<HgChangesetId>), Error = Error> {
        stream::iter_ok(pairs).boxify()
    }

    fn extract_chunk(wrapped: Poll<Option<Chunk>, Error>) -> Result<Chunk> {
        if let Ok(Async::Ready(Some(chunk))) = wrapped {
            return Ok(chunk);
        }
        Err(Error::msg("no chunk"))
    }

    fn extract_vec(wrapped: Poll<Option<Chunk>, Error>) -> Result<Vec<u8>> {
        Ok(extract_chunk(wrapped)?.into_bytes()?.to_vec())
    }

    #[test]
    fn test_stream_emits_version() {
        let pairs_stream = stream_for_pairs(vec![
            (nodehash::ONES_CSID, vec![nodehash::TWOS_CSID]),
            (nodehash::THREES_CSID, vec![nodehash::FOURS_CSID]),
        ]);
        let time = DateTime::now();
        let meta = vec![];
        let mut packer = obsmarkers_packer_stream(pairs_stream, time, meta.clone());

        let r1 = extract_vec(packer.poll()).unwrap();
        assert_eq!(r1, vec![VERSION]);

        let r2 = extract_vec(packer.poll()).unwrap();
        assert!(size_matches(&r2));
        assert!(content_matches(
            &r2,
            &nodehash::ONES_CSID,
            &vec![nodehash::TWOS_CSID],
            &time,
            &meta
        ));

        let r3 = extract_vec(packer.poll()).unwrap();
        assert!(size_matches(&r3));
        assert!(content_matches(
            &r3,
            &nodehash::THREES_CSID,
            &vec![nodehash::FOURS_CSID],
            &time,
            &meta
        ));

        assert!(extract_vec(packer.poll()).is_err());
    }
}
