// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use mononoke_types::hash;

pub(crate) fn blake2_path_hash(data: &Vec<u8>) -> Vec<u8> {
    let mut hash_content = hash::Context::new("path".as_bytes());
    hash_content.update(data);
    Vec::from(hash_content.finish().as_ref())
}
