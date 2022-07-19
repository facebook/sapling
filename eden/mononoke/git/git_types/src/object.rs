/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use digest::Digest;
use mononoke_types::hash::RichGitSha1;
use sha1::Sha1;

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

    pub fn create_oid(&self, object_buff: impl AsRef<[u8]>) -> RichGitSha1 {
        let object_buff = object_buff.as_ref();
        let size = object_buff
            .len()
            .try_into()
            .expect("Object size must fit in a u64");

        let mut sha1 = Sha1::new();
        sha1.update(&format!("{} {}", self.as_str(), size));
        sha1.update(&[0]);
        sha1.update(object_buff.as_ref());

        let hash: [u8; 20] = sha1.finalize().into();

        RichGitSha1::from_byte_array(hash, self.as_str(), size)
    }
}
