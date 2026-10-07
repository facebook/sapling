/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use fbthrift::binary_protocol;
use fbthrift::compact_protocol;
use fbthrift::json5_protocol;
use fbthrift::simplejson_protocol;
use standard::StandardProtocol;
use type_rep::ProtocolUnion;

use crate::GetThriftAnyType;

// For convenience
pub trait SerializableToAny:
    GetThriftAnyType
    + fbthrift::GetUri
    + fbthrift::GetTypeNameType
    + fbthrift::GetTType
    + compact_protocol::SerializeRef
    + json5_protocol::SerializeRef
    + simplejson_protocol::SerializeRef
{
}
impl<T> SerializableToAny for T where
    T: GetThriftAnyType
        + fbthrift::GetUri
        + fbthrift::GetTypeNameType
        + fbthrift::GetTType
        + compact_protocol::SerializeRef
        + json5_protocol::SerializeRef
        + simplejson_protocol::SerializeRef
{
}

pub fn serialize<T: SerializableToAny>(object: &T) -> any::Any {
    any::Any {
        r#type: <T as GetThriftAnyType>::get_thrift_any_type(),
        protocol: ProtocolUnion::standard(StandardProtocol::Compact),
        data: compact_protocol::serialize(object).to_vec(),
        ..Default::default()
    }
}

/// Serializes a typed value into a Thrift Any using the SimpleJSON protocol.
pub fn serialize_json<T: SerializableToAny>(object: &T) -> any::Any {
    any::Any {
        r#type: <T as GetThriftAnyType>::get_thrift_any_type(),
        protocol: ProtocolUnion::standard(StandardProtocol::SimpleJson),
        data: simplejson_protocol::serialize(object).to_vec(),
        ..Default::default()
    }
}

/// Serializes a typed value into a Thrift Any using the JSON5 protocol.
pub fn serialize_json5<T: SerializableToAny>(object: &T) -> any::Any {
    any::Any {
        r#type: <T as GetThriftAnyType>::get_thrift_any_type(),
        protocol: ProtocolUnion::standard(StandardProtocol::Json5),
        data: json5_protocol::serialize_ref(object).to_vec(),
        ..Default::default()
    }
}

// 'any_wrapper'
pub trait SerializableThriftObject:
    'static
    + GetThriftAnyType
    + fbthrift::GetTType
    + fbthrift::GetUri
    + fbthrift::GetTypeNameType
    + binary_protocol::DeserializeSlice
    + compact_protocol::DeserializeSlice
    + json5_protocol::DeserializeSlice
    + simplejson_protocol::DeserializeSlice
    + binary_protocol::SerializeRef
    + compact_protocol::SerializeRef
    + json5_protocol::SerializeRef
    + simplejson_protocol::SerializeRef
{
}

impl<T> SerializableThriftObject for T where
    T: 'static
        + GetThriftAnyType
        + fbthrift::GetTType
        + fbthrift::GetUri
        + fbthrift::GetTypeNameType
        + binary_protocol::DeserializeSlice
        + compact_protocol::DeserializeSlice
        + json5_protocol::DeserializeSlice
        + simplejson_protocol::DeserializeSlice
        + binary_protocol::SerializeRef
        + compact_protocol::SerializeRef
        + json5_protocol::SerializeRef
        + simplejson_protocol::SerializeRef
{
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json5_any_uses_the_json5_protocol_id_and_encoding() {
        let value = test_structs::SimpleEnum::VARIANT2;

        let packed = serialize_json5(&value);

        assert!(matches!(
            packed.protocol,
            ProtocolUnion::standard(StandardProtocol::Json5)
        ));
        assert_eq!(packed.data, br#""VARIANT2 (2)""#);
        let decoded: test_structs::SimpleEnum =
            crate::deserialize(&packed).expect("JSON5 Any must round-trip");
        assert_eq!(decoded, value);
    }
}
