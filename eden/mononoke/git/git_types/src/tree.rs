/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Result;
use anyhow::bail;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::KeyedBlobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use filestore::FetchKey;
use filestore::FilestoreConfig;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::future::ready;
use futures::stream;
use futures::stream::BoxStream;
use futures_watchdog::WatchdogExt;
use gix_hash::ObjectId;
use gix_object::Object;
use gix_object::Tree;
use gix_object::tree;
use manifest::Entry;
use manifest::Manifest;
use manifest::ManifestOps;
use manifest::ManifestParentReplacement;
use manifest::derive_manifest;
use manifest::flatten_subentries;
use mononoke_types::BasicFileChange;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::FileType;
use mononoke_types::GitLfs;
use mononoke_types::MPathElement;
use mononoke_types::NonRootMPath;
use mononoke_types::SortedVectorTrieMap;
use mononoke_types::hash::GitSha1;
use mononoke_types::hash::RichGitSha1;
use sorted_vector_map::SortedVectorMap;

use crate::DeltaObjectKind;
use crate::GitIdentifier;
use crate::MappedGitCommitId;
use crate::errors::MononokeGitError;
use crate::fetch_non_blob_git_object;
use crate::git_lfs::generate_and_store_git_lfs_pointer;
use crate::git_object_bytes_with_hash;
use crate::upload_non_blob_git_object;

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
    async fn oid_from_content_id<B: Blobstore>(
        ctx: &CoreContext,
        blobstore: &B,
        content_id: ContentId,
    ) -> Result<ObjectId> {
        let key = FetchKey::Canonical(content_id);
        let metadata = filestore::get_metadata(blobstore, ctx, &key)
            .await?
            .ok_or(MononokeGitError::ContentMissing(key))?;
        metadata.git_sha1.to_object_id()
    }

    pub async fn new<B: Blobstore + Clone>(
        ctx: &CoreContext,
        blobstore: &B,
        file_change: &BasicFileChange,
    ) -> Result<Self> {
        let file_type = file_change.file_type();

        let oid = if file_type == FileType::GitSubmodule {
            let bytes =
                filestore::fetch_concat_exact(blobstore, ctx, file_change.content_id(), 20).await?;
            gix_hash::oid::try_from_bytes(&bytes)?.to_owned()
        } else {
            Self::oid_from_content_id(ctx, blobstore, file_change.content_id()).await?
        };
        let entry_kind: tree::EntryKind = file_type.into();
        Ok(Self(oid, entry_kind.into()))
    }

    pub fn from_oid_and_file_type(oid: RichGitSha1, file_type: FileType) -> Result<Self> {
        let oid = oid.to_object_id()?;
        let entry_kind: tree::EntryKind = file_type.into();
        Ok(Self(oid, entry_kind.into()))
    }

    pub async fn from_content_id_and_file_type<B: Blobstore>(
        ctx: &CoreContext,
        blobstore: &B,
        content_id: ContentId,
        file_type: FileType,
    ) -> Result<Self> {
        if file_type == FileType::GitSubmodule {
            anyhow::bail!("Non-canonical LFS pointer blob cannot be a submodule")
        }
        let oid = Self::oid_from_content_id(ctx, blobstore, content_id).await?;
        let entry_kind: tree::EntryKind = file_type.into();
        Ok(Self(oid, entry_kind.into()))
    }

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
        if self.is_submodule() {
            anyhow::bail!("Fetching size of GitLeaf item that is a submodule is not supported");
        }
        let key = GitSha1::from_object_id(&self.0)?.into();
        let metadata = filestore::get_metadata(blobstore, ctx, &key)
            .await?
            .ok_or_else(|| anyhow::anyhow!("No metadata for {}", self.0))?;
        Ok(metadata.total_size)
    }
}

#[allow(async_fn_in_trait)]
pub trait GitEntry {
    fn oid(&self) -> ObjectId;
    fn identifier(&self) -> Result<GitIdentifier>;
    fn is_submodule(&self) -> bool;
    fn kind(&self) -> DeltaObjectKind;
    async fn size(&self, ctx: &CoreContext, blobstore: &impl Blobstore) -> Result<u64>;
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

    fn kind(&self) -> DeltaObjectKind {
        match self {
            Entry::Tree(_) => DeltaObjectKind::Tree,
            Entry::Leaf(_) => DeltaObjectKind::Blob,
        }
    }

    async fn size(&self, ctx: &CoreContext, blobstore: &impl Blobstore) -> Result<u64> {
        match self {
            Entry::Tree(tree_id) => tree_id.size(ctx, blobstore).await,
            Entry::Leaf(leaf) => leaf.size(ctx, blobstore).await,
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

impl GitTree {
    pub fn new<I>(entries: I) -> Self
    where
        I: IntoIterator<Item = (MPathElement, Entry<GitTreeId, GitLeaf>)>,
    {
        Self {
            entries: entries.into_iter().collect(),
        }
    }
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

    async fn load<'a, B: KeyedBlobstore>(
        &'a self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError> {
        fetch_non_blob_git_object(ctx, blobstore, &self.0)
            .watched(ctx.logger())
            .with_max_poll(blobstore::BLOBSTORE_MAX_POLL_TIME_MS)
            .await
            .map_err(anyhow::Error::from)
            .map_err(LoadableError::Error)?
            // TreeRef <-> Tree roundtrips, so it's OK to switch to owned here
            .with_parsed_as_tree(|tree_ref| tree_ref.to_owned())
            .ok_or_else(|| LoadableError::Error(anyhow::anyhow!("Not a tree object: {}", self.0)))?
            .try_into()
            .map_err(LoadableError::Error)
    }
}

pub(crate) async fn get_git_file_changes<B: Blobstore + Clone + 'static>(
    blobstore: &B,
    filestore_config: FilestoreConfig,
    ctx: &CoreContext,
    bcs: &BonsaiChangeset,
) -> Result<Vec<(NonRootMPath, Option<GitLeaf>)>> {
    let file_changes_stream = stream::iter(bcs.file_changes()).map(Ok);
    file_changes_stream
        .map_ok(|(mpath, file_change)| async move {
            match file_change.simplify() {
                Some(basic_file_change) => match basic_file_change.git_lfs() {
                    // File is not LFS file
                    GitLfs::FullContent => Ok((
                        mpath.clone(),
                        Some(GitLeaf::new(ctx, blobstore, basic_file_change).await?),
                    )),
                    // No pointer content provided: we're generatic spec conformant canonical
                    // pointer
                    GitLfs::GitLfsPointer {
                        non_canonical_pointer: None,
                    } => {
                        let oid = generate_and_store_git_lfs_pointer(
                            blobstore,
                            filestore_config,
                            ctx,
                            basic_file_change,
                        )
                        .await?;
                        let leaf =
                            GitLeaf::from_oid_and_file_type(oid, basic_file_change.file_type())?;
                        Ok((mpath.clone(), Some(leaf)))
                    }
                    // Non canonical LFS pointer provided - let's use it as is
                    GitLfs::GitLfsPointer {
                        non_canonical_pointer: Some(non_canonical_pointer),
                    } => Ok((
                        mpath.clone(),
                        Some(
                            GitLeaf::from_content_id_and_file_type(
                                ctx,
                                blobstore,
                                non_canonical_pointer,
                                basic_file_change.file_type(),
                            )
                            .await?,
                        ),
                    )),
                },
                None => Ok((mpath.clone(), None)),
            }
        })
        .try_buffer_unordered(100)
        .try_collect()
        .boxed()
        .await
}

pub(crate) async fn get_git_subtree_changes(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    known: Option<&HashMap<ChangesetId, MappedGitCommitId>>,
    bcs: &BonsaiChangeset,
) -> Result<Vec<ManifestParentReplacement<GitTreeId, GitLeaf>>> {
    let copy_sources = bcs.subtree_changes().iter().filter_map(|(path, change)| {
        let (from_cs_id, from_path) = change.copy_source()?;
        Some((path, from_cs_id, from_path))
    });
    stream::iter(copy_sources)
        .map(|(path, from_cs_id, from_path)| {
            cloned!(ctx);
            let blobstore = derivation_ctx.blobstore().clone();
            async move {
                let root_tree_id = derivation_ctx
                    .fetch_unknown_dependency::<MappedGitCommitId>(&ctx, known, from_cs_id)
                    .await?
                    .fetch_root_tree(&ctx, &blobstore)
                    .await?;
                let entry = root_tree_id
                    .find_entry(ctx, blobstore, from_path.clone())
                    .await?
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Subtree copy source {} does not exist in {}",
                            from_path,
                            from_cs_id
                        )
                    })?;
                Ok(ManifestParentReplacement {
                    path: path.clone(),
                    replacements: vec![entry],
                })
            }
        })
        .buffered(100)
        .try_collect()
        .boxed()
        .await
}

pub(crate) async fn derive_git_tree<B: Blobstore + Clone + 'static>(
    ctx: &CoreContext,
    blobstore: B,
    parents: Vec<GitTreeId>,
    changes: Vec<(NonRootMPath, Option<GitLeaf>)>,
    subtree_changes: Vec<ManifestParentReplacement<GitTreeId, GitLeaf>>,
) -> Result<GitTreeId> {
    let tree_id = derive_manifest(
        ctx.clone(),
        blobstore.clone(),
        parents,
        changes,
        subtree_changes,
        {
            cloned!(ctx, blobstore);
            move |tree_info| {
                cloned!(ctx, blobstore);
                async move {
                    let members = flatten_subentries(&ctx, &(), tree_info.subentries)
                        .await?
                        .map(|(p, (_, entry))| (p, entry));
                    let tree: Tree = GitTree::new(members).into();
                    let (raw_tree_bytes, git_hash) =
                        git_object_bytes_with_hash(&Object::Tree(tree))?;
                    upload_non_blob_git_object(&ctx, &blobstore, git_hash.as_ref(), raw_tree_bytes)
                        .await?;
                    Ok(((), GitTreeId(git_hash)))
                }
            }
        },
        {
            // NOTE: A None leaf will happen in derive_manifest if the parents have conflicting
            // leaves. However, since we're deriving from a Bonsai changeset and using our Git Tree
            // manifest which has leaves that are equivalent derived to their Bonsai
            // representation, that won't happen.
            |leaf_info| {
                let leaf = leaf_info
                    .change
                    .ok_or_else(|| MononokeGitError::TreeDerivationFailed.into())
                    .map(|l| ((), l));
                ready(leaf)
            }
        },
    )
    .await?;

    match tree_id {
        Some(tree_id) => Ok(tree_id),
        None => {
            // Essentially an empty tree. No need to store it, just create and return
            Ok(GitTreeId(ObjectId::empty_tree(gix_hash::Kind::Sha1)))
        }
    }
}
