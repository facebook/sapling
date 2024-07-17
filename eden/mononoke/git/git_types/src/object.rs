/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
use digest::Digest;
use gix_object::Object;
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
        sha1.update([0]);
        sha1.update(<[u8] as AsRef<[u8]>>::as_ref(object_buff));

        let hash: [u8; 20] = sha1.finalize().into();

        RichGitSha1::from_byte_array(hash, self.as_str(), size)
    }
}

#[derive(Clone, Debug)]
pub struct ObjectContent {
    pub parsed: Object,
    pub raw: Bytes,
}

impl ObjectContent {
    pub fn is_tree(&self) -> bool {
        self.parsed.as_tree().is_some()
    }

    pub fn is_blob(&self) -> bool {
        self.parsed.as_blob().is_some()
    }
}
