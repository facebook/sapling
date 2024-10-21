/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use context::CoreContext;
use futures::stream;
use futures::stream::BoxStream;
use futures::StreamExt;
use gix_hash::ObjectId;
use gix_object::tree;
use gix_object::Tree;
use gix_object::WriteTo;
use manifest::Entry;
use manifest::Manifest;
use mononoke_types::hash::GitSha1;
use mononoke_types::FileType;
use mononoke_types::MPathElement;
use mononoke_types::SortedVectorTrieMap;
use sorted_vector_map::SortedVectorMap;

use crate::fetch_non_blob_git_object;
use crate::GitIdentifier;

/// An id of a Git tree object.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct GitTreeId(pub ObjectId);

impl GitTreeId {
    pub(crate) async fn size(&self, ctx: &CoreContext, blobstore: &impl Blobstore) -> Result<u64> {
        Ok(fetch_non_blob_git_object(ctx, blobstore, &self.0)
            .await?
            .size())
    }
}

/// A leaf within a git tree.  This is the object id of the leaf and its mode as stored in the parent tree object.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct GitLeaf(pub ObjectId, pub tree::EntryMode);

impl GitLeaf {
    pub fn oid(&self) -> ObjectId {
        self.0
    }

    pub fn file_type(&self) -> Result<FileType> {
        match self.1.into() {
            tree::EntryKind::Tree => bail!("Invalid leaf entry kind: {} is a tree", self.0),
            tree::EntryKind::Blob => Ok(FileType::Regular),
            tree::EntryKind::BlobExecutable => Ok(FileType::Executable),
            tree::EntryKind::Link => Ok(FileType::Symlink),
            tree::EntryKind::Commit => Ok(FileType::GitSubmodule),
        }
    }

    pub fn is_submodule(&self) -> bool {
        tree::EntryKind::from(self.1) == tree::EntryKind::Commit
    }

    pub(crate) async fn size(&self, ctx: &CoreContext, blobstore: &impl Blobstore) -> Result<u64> {
        let key = GitSha1::from_object_id(&self.0)?.into();
        let metadata = filestore::get_metadata(blobstore, ctx, &key)
            .await?
            .ok_or_else(|| anyhow::anyhow!("No metadata for {}", self.0))?;
        Ok(metadata.total_size)
    }
}

pub trait GitEntry {
    fn oid(&self) -> ObjectId;
    fn identifier(&self) -> Result<GitIdentifier>;
    fn is_submodule(&self) -> bool;
}

impl GitEntry for Entry<GitTreeId, GitLeaf> {
    fn oid(&self) -> ObjectId {
        match self {
            Entry::Tree(tree_id) => tree_id.0,
            Entry::Leaf(leaf) => leaf.0,
        }
    }

    fn identifier(&self) -> Result<GitIdentifier> {
        match self {
            Entry::Tree(tree_id) => {
                Ok(GitIdentifier::NonBlob(GitSha1::from_object_id(&tree_id.0)?))
            }
            Entry::Leaf(leaf) => Ok(GitIdentifier::Basic(GitSha1::from_object_id(&leaf.0)?)),
        }
    }

    fn is_submodule(&self) -> bool {
        match self {
            Entry::Tree(_) => false,
            Entry::Leaf(leaf) => leaf.is_submodule(),
        }
    }
}

/// A loaded Git tree object, suitable for use with Mononoke's manifest types.
pub struct GitTree {
    /// The entries in the tree, sorted by path element name.
    ///
    /// Note that the order of entries in a Git tree is different than the
    /// order of entries in a Mononoke tree.  This is because Git trees are
    /// sorted by their own ordering which treats directories differently.
    entries: SortedVectorMap<MPathElement, Entry<GitTreeId, GitLeaf>>,
}

#[async_trait]
impl<Store: Send + Sync> Manifest<Store> for GitTree {
    type TreeId = GitTreeId;
    type Leaf = GitLeaf;
    type TrieMapType = SortedVectorTrieMap<Entry<GitTreeId, GitLeaf>>;

    async fn lookup(
        &self,
        _ctx: &CoreContext,
        _blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<Self::TreeId, Self::Leaf>>> {
        Ok(self.entries.get(name).cloned())
    }

    async fn list(
        &self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        Ok(stream::iter(self.entries.iter().map(|(k, v)| Ok((k.clone(), v.clone())))).boxed())
    }

    async fn into_trie_map(
        self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<Self::TrieMapType> {
        let entries = self
            .entries
            .into_iter()
            .map(|(k, v)| (k.to_smallvec(), v))
            .collect();
        Ok(SortedVectorTrieMap::new(entries))
    }
}

impl TryFrom<Tree> for GitTree {
    type Error = anyhow::Error;

    fn try_from(value: Tree) -> Result<Self> {
        let entries = value
            .entries
            .into_iter()
            .map(|entry| {
                let elem = MPathElement::new(entry.filename.into())?;
                let entry = match entry.mode.into() {
                    tree::EntryKind::Tree => Entry::Tree(GitTreeId(entry.oid)),
                    _ => Entry::Leaf(GitLeaf(entry.oid, entry.mode)),
                };
                Ok((elem, entry))
            })
            .collect::<Result<SortedVectorMap<_, _>>>()?;
        Ok(Self { entries })
    }
}

impl From<GitTree> for Tree {
    fn from(value: GitTree) -> Self {
        let mut entries = value
            .entries
            .into_iter()
            .map(|(elem, entry)| {
                let filename = elem.to_smallvec().into_vec().into();
                let (oid, mode) = match entry {
                    Entry::Tree(GitTreeId(oid)) => (oid, tree::EntryKind::Tree.into()),
                    Entry::Leaf(GitLeaf(oid, mode)) => (oid, mode),
                };
                tree::Entry {
                    oid,
                    mode,
                    filename,
                }
            })
            .collect::<Vec<_>>();
        // Git tree entries are sorted by their own ordering, which does not
        // match the order in Mononoke.  Git Oxide has a custom implementation
        // of `Ord` on `tree::Entry` that provides the correct ordering, which
        // means that we can sort the entries to obtain the correct order.
        //
        // Because the entries come from a collection sorted in Mononoke order,
        // most of the entries are in roughly the right order, so we use `sort`
        // as it is faster on mostly-sorted data.
        entries.sort();
        Self { entries }
    }
}

#[async_trait]
impl Loadable for GitTreeId {
    type Value = GitTree;

    async fn load<'a, B: Blobstore>(
        &'a self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError> {
        fetch_non_blob_git_object(ctx, blobstore, &self.0)
            .await
            .map_err(anyhow::Error::from)
            .map_err(LoadableError::Error)?
            .try_into_tree()
            .map_err(|_| LoadableError::Error(anyhow::anyhow!("Not a tree object: {}", self.0)))?
            .try_into()
            .map_err(LoadableError::Error)
    }
}
