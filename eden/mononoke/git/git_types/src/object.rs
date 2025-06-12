/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::hash::Hash;
use std::hash::Hasher;
use std::sync::Arc;

use bytes::Bytes;
use digest::Digest;
use gix_object::BlobRef;
use gix_object::CommitRef;
use gix_object::Object;
use gix_object::ObjectRef;
use gix_object::TagRef;
use gix_object::TreeRef;
use mononoke_types::hash::RichGitSha1;
use ouroboros::self_referencing;
use sha1::Sha1;

use crate::errors::GitError;

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
        sha1.update(format!("{} {}", self.as_str(), size));
        sha1.update([0]);
        sha1.update(<[u8] as AsRef<[u8]>>::as_ref(object_buff));

        let hash: [u8; 20] = sha1.finalize().into();

        RichGitSha1::from_byte_array(hash, self.as_str(), size)
    }
}

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct OwnedObjectContent {
    pub parsed: Object,
    pub raw: Bytes,
}

impl OwnedObjectContent {
    pub fn new(parsed: Object, raw: Bytes) -> Self {
        Self { parsed, raw }
    }

    pub fn is_tree(&self) -> bool {
        self.parsed.as_tree().is_some()
    }

    pub fn is_blob(&self) -> bool {
        self.parsed.as_blob().is_some()
    }
}

#[self_referencing]
#[derive(Debug)]
pub struct ObjectContentInner {
    raw: Bytes,
    #[borrows(raw)]
    #[not_covariant]
    parsed: ObjectRef<'this>,
}

#[derive(Debug, Clone)]
pub struct ObjectContent(Arc<ObjectContentInner>);

impl ObjectContent {
    pub fn try_from_loose(raw: Bytes) -> Result<Self, GitError> {
        Ok(Self(Arc::new(
            ObjectContentInnerTryBuilder {
                raw,
                parsed_builder: |raw| {
                    ObjectRef::from_loose(raw).map_err(|e| {
                        let mut hasher = Sha1::new();
                        hasher.update(raw);
                        let hash = hasher.finalize();
                        let num_bytes_to_show = raw.len().min(100);
                        let error_context = format!(
                            "{hash:x}\n{}",
                            String::from_utf8_lossy_owned(raw.slice(..num_bytes_to_show).into())
                        );
                        GitError::InvalidContent(
                            error_context,
                            anyhow::anyhow!(e.to_string()).into(),
                        )
                    })
                },
            }
            .try_build()?,
        )))
    }

    fn inner(&self) -> &'_ ObjectContentInner {
        &self.0
    }
    pub fn raw(&self) -> &'_ Bytes {
        self.inner().borrow_raw()
    }

    pub fn is_tree(&self) -> bool {
        self.inner()
            .with_parsed(move |parsed| parsed.as_tree().is_some())
    }
    pub fn is_blob(&self) -> bool {
        self.inner()
            .with_parsed(move |parsed| parsed.as_blob().is_some())
    }
    pub fn is_tag(&self) -> bool {
        self.inner()
            .with_parsed(move |parsed| parsed.as_tag().is_some())
    }
    pub fn is_commit(&self) -> bool {
        self.inner()
            .with_parsed(move |parsed| parsed.as_commit().is_some())
    }

    pub fn with_parsed<Out>(&self, f: impl FnOnce(&ObjectRef<'_>) -> Out) -> Out {
        self.inner().with_parsed(f)
    }
    pub fn with_parsed_as_tree<Out>(&self, f: impl FnOnce(&TreeRef<'_>) -> Out) -> Option<Out> {
        self.inner().with_parsed(|parsed| {
            let tree = parsed.as_tree()?;
            Some(f(tree))
        })
    }
    pub fn with_parsed_as_blob<Out>(&self, f: impl FnOnce(&BlobRef<'_>) -> Out) -> Option<Out> {
        self.inner().with_parsed(|parsed| {
            let blob = parsed.as_blob()?;
            Some(f(blob))
        })
    }
    pub fn with_parsed_as_tag<Out>(&self, f: impl FnOnce(&TagRef<'_>) -> Out) -> Option<Out> {
        self.inner().with_parsed(|parsed| {
            let tag = parsed.as_tag()?;
            Some(f(tag))
        })
    }
    pub fn with_parsed_as_commit<Out>(&self, f: impl FnOnce(&CommitRef<'_>) -> Out) -> Option<Out> {
        self.inner().with_parsed(|parsed| {
            let commit = parsed.as_commit()?;
            Some(f(commit))
        })
    }
}
impl Hash for ObjectContent {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw().hash(state);
    }
}

impl PartialEq for ObjectContent {
    fn eq(&self, other: &Self) -> bool {
        self.raw() == other.raw()
    }
}

impl Eq for ObjectContent {}
