/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Target types for the mincode fuzz targets.
//!
//! mincode is a serde format with no framing of its own, so there is no
//! decoder to point a fuzzer at directly: the target type decides which
//! `Deserializer` methods run and in what order. The two shapes below are the
//! ones read off disk today - metalog's `Root` and revisionstore's LFS
//! pointer. Both are `pub(crate)` in their own crates, so these are structural
//! mirrors: same field order and same serde calls, therefore the same
//! encoding.

use std::collections::BTreeMap;
use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

/// Mirror of `metalog::metalog::Root`, decoded by `load_root` on every
/// `MetaLog::open`.
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct Root {
    pub map: BTreeMap<String, SerId20>,
    pub timestamp: u64,
    pub message: String,
}

/// Mirror of `metalog::metalog::SerId20`. `types::serde_with::hgid::tuple`
/// encodes an `Id20` as a 20 element tuple, which is what `[u8; 20]` does.
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct SerId20(pub [u8; 20]);

/// Mirror of `revisionstore::lfs::LfsPointersEntry`, decoded by
/// `LfsPointersStore::get_from_slice` on every LFS pointer read.
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct LfsPointer {
    pub hgid: [u8; 20],
    pub size: u64,
    pub is_binary: bool,
    pub copy_from: Option<TupleKey>,
    pub content_hashes: HashMap<ContentHashType, ContentHash>,
}

/// Mirror of `types::serde_with::key::TupleKey`.
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct TupleKey {
    pub path: RepoPathBuf,
    pub hgid: [u8; 20],
}

/// Mirror of `types::RepoPathBuf`.
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct RepoPathBuf(pub String);

/// Mirror of `revisionstore::lfs::ContentHashType`.
#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
pub enum ContentHashType {
    Sha256,
}

/// Mirror of `revisionstore::types::ContentHash`.
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub enum ContentHash {
    Sha256([u8; 32]),
}
