/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # refencode
//!
//! Encode and decode commit references such as bookmarks, remotenames, and
//! visibleheads.

use std::collections::BTreeMap;
use std::io;
use std::str::FromStr;

pub use types::HgId;

/// Encode remote bookmarks like `[('remote/master', node), ...]` to bytes.
pub fn encode_remotenames(name_nodes: &BTreeMap<String, HgId>) -> Vec<u8> {
    let encoded = name_nodes
        .iter()
        .map(|(name, node)| format!("{} bookmarks {}\n", node.to_hex(), name))
        .collect::<Vec<_>>()
        .concat();
    encoded.into_bytes()
}

/// Decode remote bookmarks encoded by `encode_remotenames`.
pub fn decode_remotenames(bytes: &[u8]) -> io::Result<BTreeMap<String, HgId>> {
    let text = std::str::from_utf8(bytes).map_err(invalid)?;
    let mut decoded = BTreeMap::<String, HgId>::new();
    for line in text.lines() {
        let split: Vec<&str> = line.splitn(3, ' ').collect();
        if let [hex, kind, name] = split[..] {
            // See https://fburl.com/1rft34i8 for why ignore default-push/
            if kind == "bookmarks" && !name.starts_with("default-push/") {
                let node = HgId::from_str(hex).map_err(invalid)?;
                decoded.insert(name.to_string(), node);
            }
        } else {
            return Err(invalid(format!("corrupt entry in remotenames: {}", line)));
        }
    }
    Ok(decoded)
}

/// Encode local bookmarks.
pub fn encode_bookmarks(name_nodes: &BTreeMap<String, HgId>) -> Vec<u8> {
    let encoded = name_nodes
        .iter()
        .map(|(name, node)| format!("{} {}\n", node.to_hex(), name))
        .collect::<Vec<_>>()
        .concat();
    encoded.into_bytes()
}

/// Decode local bookmarks encoded by `encode_bookmarks`.
pub fn decode_bookmarks(bytes: &[u8]) -> io::Result<BTreeMap<String, HgId>> {
    let text = std::str::from_utf8(bytes).map_err(invalid)?;
    let mut decoded = BTreeMap::<String, HgId>::new();
    for line in text.lines() {
        let split: Vec<&str> = line.splitn(2, ' ').collect();
        if let [hex, name] = split[..] {
            let node = HgId::from_str(hex).map_err(invalid)?;
            decoded.insert(name.to_string(), node);
        } else {
            return Err(invalid(format!("corrupt entry in bookmarks: {}", line)));
        }
    }
    Ok(decoded)
}

/// Encode visible heads.
pub fn encode_visibleheads(heads: &[HgId]) -> Vec<u8> {
    let encoded = std::iter::once("v1\n".to_string())
        .chain(heads.iter().map(|h| format!("{}\n", h.to_hex())))
        .collect::<Vec<_>>()
        .concat();
    encoded.into_bytes()
}

/// Decode visible heads encoded by `encode_visibleheads`.
pub fn decode_visibleheads(bytes: &[u8]) -> io::Result<Vec<HgId>> {
    let text = std::str::from_utf8(bytes).map_err(invalid)?;
    let mut decoded = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if i == 0 {
            if line != "v1" {
                return Err(invalid(format!("invalid visibleheads format: {}", line)));
            }
        } else {
            let node = HgId::from_str(line).map_err(invalid)?;
            decoded.push(node);
        }
    }
    Ok(decoded)
}

fn invalid(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_remotenames() {
        let m = map();
        let encoded = encode_remotenames(&m);
        let decoded = decode_remotenames(&encoded).unwrap();
        assert_eq!(decoded, m);
    }

    #[test]
    fn test_encode_decode_bookmarks() {
        let m = map();
        let encoded = encode_bookmarks(&m);
        let decoded = decode_bookmarks(&encoded).unwrap();
        assert_eq!(decoded, m);
    }

    #[test]
    fn test_encode_decode_visibleheads() {
        let heads = map().values().cloned().collect::<Vec<HgId>>();
        let encoded = encode_visibleheads(&heads);
        let decoded = decode_visibleheads(&encoded).unwrap();
        assert_eq!(decoded, heads);
    }

    fn map() -> BTreeMap<String, HgId> {
        let mut m = BTreeMap::new();
        for i in 0..10 {
            let name = format!("foo/a{}", i);
            let node = HgId::from_byte_array([i * 11; HgId::len()]);
            m.insert(name, node);
        }
        m
    }
}
