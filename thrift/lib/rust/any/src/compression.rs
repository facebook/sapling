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

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use standard::StandardProtocol;
use type_rep::ProtocolUnion;

use crate::Any;

const COMPRESSED_COMPACT_PROTOCOL: &str = "cpp_compact_compressed";

pub fn is_compressed(any: &Any) -> bool {
    if let ProtocolUnion::custom(custom) = &any.protocol {
        return custom == COMPRESSED_COMPACT_PROTOCOL;
    }
    false
}

pub fn compress_any(any: &Any) -> Result<Any> {
    if is_compressed(any) {
        return Ok(any.clone());
    }
    if !matches!(
        any.protocol,
        ProtocolUnion::standard(StandardProtocol::Compact)
    ) {
        bail!("legacy Any compression only supports the Compact protocol");
    }
    let mut compressed = any.clone();
    compressed.protocol = ProtocolUnion::custom(COMPRESSED_COMPACT_PROTOCOL.to_string());
    compressed.data =
        zstd::stream::encode_all(any.data.as_slice(), 0 /* use zstd’s default level */)
            .context("failed to compress")?;
    Ok(compressed)
}

pub fn decompress_any(any: &Any) -> Result<Any> {
    if !is_compressed(any) {
        if matches!(
            any.protocol,
            ProtocolUnion::standard(StandardProtocol::Compact)
        ) {
            return Ok(any.clone());
        }
        bail!("legacy Any decompression only supports the Compact protocol");
    }
    let mut decompressed = any.clone();
    decompressed.protocol = ProtocolUnion::standard(StandardProtocol::Compact);
    decompressed.data =
        zstd::stream::decode_all(any.data.as_slice()).context("failed to decompress")?;
    Ok(decompressed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_trip_compression() -> Result<()> {
        let any = Any {
            protocol: ProtocolUnion::standard(StandardProtocol::Compact),
            data: b"hello world".to_vec(),
            ..Default::default()
        };
        assert!(!is_compressed(&any));
        let compressed = compress_any(&any)?;
        assert!(is_compressed(&compressed));
        let decompressed = decompress_any(&compressed)?;
        assert!(!is_compressed(&decompressed));
        assert_eq!(any, decompressed);
        Ok(())
    }

    #[test]
    fn test_rejects_non_compact_protocol() {
        let any = Any {
            protocol: ProtocolUnion::standard(StandardProtocol::Json5),
            data: b"{}".to_vec(),
            ..Default::default()
        };

        assert!(compress_any(&any).is_err());
    }

    #[test]
    fn test_preserves_uncompressed_compact_payload() -> Result<()> {
        let any = Any {
            protocol: ProtocolUnion::standard(StandardProtocol::Compact),
            data: b"hello world".to_vec(),
            ..Default::default()
        };

        assert_eq!(decompress_any(&any)?, any);
        Ok(())
    }

    #[test]
    fn test_rejects_uncompressed_non_compact_payload() {
        let any = Any {
            protocol: ProtocolUnion::standard(StandardProtocol::Json5),
            data: b"{}".to_vec(),
            ..Default::default()
        };

        assert!(decompress_any(&any).is_err());
    }
}
