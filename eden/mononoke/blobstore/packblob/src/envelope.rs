/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::pack;

use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use blobstore::SizeMetadata;
use bufsize::SizeCounter;
use bytes::Buf;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;
use fbthrift::compact_protocol;
use fbthrift::compact_protocol::CompactProtocolSerializer;
use fbthrift::serialize::Serialize as ThriftSerialize;
use mononoke_types::BlobstoreBytes;
use packblob_thrift::StorageEnvelope;
use packblob_thrift::StorageFormat;
use std::mem::size_of;

enum HeaderType {
    PackBlobCompactFormat,
}

impl TryFrom<u32> for HeaderType {
    type Error = Error;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            // 0 is thrift compact_protocol.  We can use other values for other encodings in future
            0 => Ok(HeaderType::PackBlobCompactFormat),
            _ => Err(format_err!("Unknown header value for packblob {}", value))?,
        }
    }
}

impl From<HeaderType> for u32 {
    fn from(value: HeaderType) -> u32 {
        match value {
            HeaderType::PackBlobCompactFormat => 0,
        }
    }
}

// Serialize a Thrift value using the compact protocol with a header prefixed
fn compact_serialize_with_header<T>(header: u32, v: T) -> Bytes
where
    T: ThriftSerialize<CompactProtocolSerializer<SizeCounter>>
        + ThriftSerialize<CompactProtocolSerializer<BytesMut>>,
{
    // Get the size for the compact serialization
    let sz = compact_protocol::serialize_size(&v) + size_of::<u32>();

    // Use BytesMut split to get two views on the buffer
    let mut overall_buf = BytesMut::with_capacity(sz);
    overall_buf.put_u32(header);
    let body_buf = overall_buf.split();

    // Thrift serialize
    let body = compact_protocol::serialize_to_buffer(&v, body_buf).into_inner();

    // Recombine header and body. This is O(1) as buffers are contiguous
    overall_buf.unsplit(body);

    overall_buf.freeze()
}

// new type so can implement conversions
pub(crate) struct PackEnvelope(pub packblob_thrift::StorageEnvelope);

impl PackEnvelope {
    pub fn decode(self, key: &str) -> Result<(BlobstoreBytes, SizeMetadata), Error> {
        Ok(match self.0.storage {
            StorageFormat::Single(single) => {
                let (decoded, unique_compressed_size) = pack::decode_independent(single)
                    .with_context(|| format!("While decoding independent {:?}", key))?;
                let sizing = SizeMetadata {
                    unique_compressed_size,
                    pack_meta: None,
                };
                (decoded, sizing)
            }
            StorageFormat::Packed(packed) => pack::decode_pack(packed, key)
                .with_context(|| format!("While decoding pack for {:?}", key))?,
            StorageFormat::UnknownField(e) => {
                return Err(format_err!("StorageFormat::UnknownField {:?}", e));
            }
        })
    }
}

impl TryFrom<BlobstoreBytes> for PackEnvelope {
    type Error = Error;

    fn try_from(bytes: BlobstoreBytes) -> Result<Self, Error> {
        let mut bytes = bytes.into_bytes();
        let header: HeaderType = HeaderType::try_from(bytes.get_u32())?;
        let t: StorageEnvelope = match header {
            HeaderType::PackBlobCompactFormat => compact_protocol::deserialize(&bytes)?,
        };
        Ok(PackEnvelope(t))
    }
}

impl From<PackEnvelope> for BlobstoreBytes {
    fn from(p: PackEnvelope) -> Self {
        let data = compact_serialize_with_header(HeaderType::PackBlobCompactFormat.into(), &p.0);
        BlobstoreBytes::from_bytes(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_roundtrip() -> Result<(), Error> {
        let value = "hello world!".to_string();
        let magic = 42;
        let mut bytes = compact_serialize_with_header(magic, value.clone());
        let header = bytes.get_u32();
        assert_eq!(magic, header);
        let roundtripped: String = compact_protocol::deserialize(bytes)?;
        assert_eq!(value, roundtripped);
        Ok(())
    }
}
