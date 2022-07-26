/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use bufsize::SizeCounter;
use bytes::Bytes;
use bytes::BytesMut;
use fbthrift::compact_protocol;
use fbthrift::compact_protocol::CompactProtocolDeserializer;
use fbthrift::compact_protocol::CompactProtocolSerializer;
use fbthrift::Deserialize;
use fbthrift::Serialize;

use crate::errors::ErrorKind;

pub trait ThriftConvert: Sized {
    const NAME: &'static str;
    type Thrift: Serialize<CompactProtocolSerializer<SizeCounter>>
        + Serialize<CompactProtocolSerializer<BytesMut>>
        + Deserialize<CompactProtocolDeserializer<std::io::Cursor<Bytes>>>;
    fn from_thrift(t: Self::Thrift) -> Result<Self>;
    fn into_thrift(self) -> Self::Thrift;
    fn from_bytes(bytes: &Bytes) -> Result<Self> {
        let thrift = compact_protocol::deserialize(bytes)
            .with_context(|| ErrorKind::BlobDeserializeError(Self::NAME.to_string()))?;
        Self::from_thrift(thrift)
    }
    fn into_bytes(self) -> Bytes {
        compact_protocol::serialize(&self.into_thrift())
    }
}
