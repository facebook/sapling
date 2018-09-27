// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bytes::Bytes;

use crypto::digest::Digest;
use crypto::sha2::Sha256;

pub fn get_sha256_alias_key(key: String) -> String {
    format!("alias.sha256.{}", key)
}

pub fn get_sha256_alias(contents: &Bytes) -> String {
    let mut hasher = Sha256::new();
    hasher.input(contents);
    let output = hasher.result_str();
    get_sha256_alias_key(output)
}
