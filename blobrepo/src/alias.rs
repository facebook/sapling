// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bytes::Bytes;

use crypto::digest::Digest;
use crypto::sha2::Sha256;

use mononoke_types::{hash, ContentId, MononokeId};

/// Format: alias.sha256.SHA256HASH
/// Used to make a mapping {alias.sha256.SHA256HASH: content.blake2.BLAKE2HASH}
pub fn get_sha256_alias_key(key: String) -> String {
    format!("alias.sha256.{}", key)
}

pub fn get_sha256_alias(contents: &Bytes) -> String {
    let output = get_sha256(contents);
    get_sha256_alias_key(output.to_hex().to_string())
}

pub fn get_sha256(contents: &Bytes) -> hash::Sha256 {
    let mut hasher = Sha256::new();
    hasher.input(contents);
    let mut hash_buffer: [u8; 32] = [0; 32];
    hasher.result(&mut hash_buffer);
    hash::Sha256::from_byte_array(hash_buffer)
}

/// Format: alias.content.blake2.BLAKE2HASH
/// Used to make a mapping {alias.content.blake2.BLAKE2HASH: alias.sha256.SHA256HASH}
pub fn get_content_id_alias_key(key: ContentId) -> String {
    format!("alias.{}", key.blobstore_key())
}
