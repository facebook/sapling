/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use blobstore::BlobstoreGetData;
use bufsize::SizeCounter;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use fbthrift::{
    compact_protocol::{self, CompactProtocolSerializer},
    serialize::Serialize as ThriftSerialize,
};
use mononoke_types::BlobstoreBytes;
use packblob_thrift::StorageEnvelope;
use std::{
    convert::{TryFrom, TryInto},
    mem::size_of,
};

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

impl TryFrom<BlobstoreBytes> for PackEnvelope {
    type Error = Error;

    fn try_from(bytes: BlobstoreBytes) -> Result<Self, Error> {
        let mut bytes = bytes.into_bytes();
        let header: HeaderType = HeaderType::try_from(bytes.get_u32())?;
        let t: StorageEnvelope = match header {
            HeaderType::PackBlobCompactFormat => compact_protocol::deserialize(bytes)?,
        };
        Ok(PackEnvelope(t))
    }
}

impl Into<BlobstoreBytes> for PackEnvelope {
    fn into(self) -> BlobstoreBytes {
        let data = compact_serialize_with_header(
            HeaderType::PackBlobCompactFormat.into(),
            &self.0.clone(),
        );
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
        let roundtripped: String = compact_protocol::deserialize(&bytes)?;
        assert_eq!(value, roundtripped);
        Ok(())
    }
}
