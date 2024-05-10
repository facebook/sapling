/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use bufsize::SizeCounter;
use bytes::Bytes;
use bytes::BytesMut;
use fbthrift::compact_protocol;
use fbthrift::compact_protocol::CompactProtocolDeserializer;
use fbthrift::compact_protocol::CompactProtocolSerializer;
use fbthrift::Deserialize;
use fbthrift::GetTType;
use fbthrift::Serialize;
pub use thrift_convert_proc_macros::ThriftConvert;

mod impls;

/// A trait for types that can be converted to and from Thrift.
///
/// Deriving this trait is supported for:
/// - Structs that have only ThriftConvert-able fields. A helper attribute is used to specify
/// the path to the corresponding thrift type of the struct (#[thrift(path::to::thrift_type)]).
/// Each field's name must match the corresponding thrift struct field name.
/// - Enums where the variants either have no fields or have a single unnamed field. The helper
/// attribute must be used to specify the thrift type for the enum, as well as the thrift type
/// for variants that have no fields. The enum is converted to a thrift union, and the each
/// enum variant is converted to the thrift union variant that have the same name after being
/// converted to snake case.
///
/// See thrift_convert/tests for examples of supported cases.
pub trait ThriftConvert: Sized {
    const NAME: &'static str;
    type Thrift: Serialize<CompactProtocolSerializer<SizeCounter>>
        + Serialize<CompactProtocolSerializer<BytesMut>>
        + Deserialize<CompactProtocolDeserializer<std::io::Cursor<Bytes>>>
        + GetTType;
    fn from_thrift(t: Self::Thrift) -> Result<Self>;
    fn into_thrift(self) -> Self::Thrift;
    fn from_bytes(bytes: &Bytes) -> Result<Self> {
        let thrift = compact_protocol::deserialize(bytes)
            .with_context(|| anyhow!("error while deserializing blob for '{}'", Self::NAME))?;
        Self::from_thrift(thrift)
    }
    fn into_bytes(self) -> Bytes {
        compact_protocol::serialize(&self.into_thrift())
    }
}
