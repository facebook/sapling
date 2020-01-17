/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crypto::{digest::Digest, sha1::Sha1};
use mononoke_types::hash::GitSha1;
use std::convert::TryInto;

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub enum ObjectKind {
    Blob,
    Tree,
    Commit,
}

impl ObjectKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Blob => "blob",
            Self::Tree => "tree",
            Self::Commit => "commit",
        }
    }

    pub fn is_tree(&self) -> bool {
        match self {
            Self::Blob => false,
            Self::Tree => true,
            Self::Commit => false,
        }
    }

    pub fn create_oid(&self, object_buff: impl AsRef<[u8]>) -> GitSha1 {
        let object_buff = object_buff.as_ref();
        let size = object_buff
            .len()
            .try_into()
            .expect("Object size must fit in a u64");

        let mut sha1 = Sha1::new();
        sha1.input_str(&format!("{} {}", self.as_str(), size));
        sha1.input(&[0]);
        sha1.input(object_buff.as_ref());

        let mut hash = [0u8; 20];
        sha1.result(&mut hash);

        GitSha1::from_byte_array(hash, self.as_str(), size)
    }
}
