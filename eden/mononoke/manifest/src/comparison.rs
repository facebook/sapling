/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::iter::Peekable;

use anyhow::Result;
use blobstore::StoreLoadable;
use borrowed::borrowed;
use cloned::cloned;
use context::CoreContext;
use futures::future;
use futures::future::FutureExt;
use futures::stream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_types::MPath;
use mononoke_types::MPathElement;
use mononoke_types::MPathElementPrefix;
use mononoke_types::NonRootMPath;

use crate::AsyncManifest;
use crate::Entry;
use crate::TrieMapOps;

/// Result of a multi-way comparison between a manifest tree and the merge of
/// a number of base manifest treees.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Comparison<TrieMapType, V> {
    /// The entry at this path is new.
    New(NonRootMPath, V),
    /// The entry at this path has changed compared all to the bases.
    Changed(NonRootMPath, V, Vec<Option<V>>),
    /// The entry at this path is the same as at least one of the bases.
    Same(NonRootMPath, V, Vec<Option<V>>, usize),
    /// The entry at this path has been removed.
    Removed(NonRootMPath, Vec<Option<V>>),
    /// All entries with this path and prefix are new.
    ManyNew(MPath, MPathElementPrefix, TrieMapType),
    /// All entries with this path and prefix are the same as at least one of the bases.
    ManySame(
        MPath,
        MPathElementPrefix,
        TrieMapType,
        Vec<Option<TrieMapType>>,
        /// The index of the first base manifest that this entry is the same as.
        usize,
    ),
    /// All entries with this path and prefix have been removed.
    ManyRemoved(MPath, MPathElementPrefix, Vec<Option<TrieMapType>>),
}

/// Result of a multi-way comparison between a single manifest and the merge
/// of a number of base manifests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestComparison<TrieMapType, V> {
    /// The entry at this path is new.
    New(MPathElement, V),
    /// The entry at this path has changed compared to all of the bases.
    Changed(MPathElement, V, Vec<Option<V>>),
    /// The entry at this path is the same as at least one of the bases.
    Same(MPathElement, V, Vec<Option<V>>, usize),
    /// The entry at this path has been removed.
    Removed(MPathElement, Vec<Option<V>>),
    /// All entries with this prefix are new.
    ManyNew(MPathElementPrefix, TrieMapType),
    /// All entries with this prefix are the same as at least one of the bases.
    ManySame(
        MPathElementPrefix,
        TrieMapType,
        Vec<Option<TrieMapType>>,
        /// The index of the first base manifest that this entry is the same as.
        usize,
    ),
    /// All entries with this prefix have been removed.
    ManyRemoved(MPathElementPrefix, Vec<Option<TrieMapType>>),
}

pub async fn compare_manifest<'a, M, Store>(
    ctx: &'a CoreContext,
    blobstore: &'a Store,
    mf: M,
    base_mfs: Vec<Option<M>>,
) -> Result<
    impl Stream<Item = Result<ManifestComparison<M::TrieMapType, Entry<M::TreeId, M::LeafId>>>> + 'a,
>
where
    M: AsyncManifest<Store>,
    M::TreeId: Send + Sync + Eq + 'static,
    M::LeafId: Send + Sync + Eq + 'static,
    M::TrieMapType: TrieMapOps<Store, Entry<M::TreeId, M::LeafId>> + Eq,
    Store: Send + Sync + 'static,
{
    let (mf_trie_map, base_mf_trie_maps) = future::try_join(
        mf.into_trie_map(ctx, blobstore),
        future::try_join_all(base_mfs.into_iter().map(|p| async move {
            match p {
                Some(p) => Ok(Some(p.into_trie_map(ctx, blobstore).await?)),
                None => Ok(None),
            }
        })),
    )
    .await?;
    Ok(bounded_traversal::bounded_traversal_stream(
        256,
        Some((MPathElementPrefix::new(), mf_trie_map, base_mf_trie_maps)),
        {
            cloned!(ctx, blobstore);
            move |(prefix, mf_trie_map, base_mf_trie_maps)| {
                cloned!(ctx, blobstore);
                async move {
                    if let Some(index) = base_mf_trie_maps
                        .iter()
                        .position(|parent| parent.as_ref() == Some(&mf_trie_map))
                    {
                        return anyhow::Ok((
                            stream::iter(vec![Ok(ManifestComparison::ManySame(
                                prefix,
                                mf_trie_map,
                                base_mf_trie_maps,
                                index,
                            ))]),
                            vec![],
                        ));
                    }

                    if base_mf_trie_maps.is_empty()
                        || base_mf_trie_maps
                            .iter()
                            .all(|parent| parent.as_ref().map_or(true, TrieMapOps::is_empty))
                    {
                        return Ok((
                            stream::iter(vec![Ok(ManifestComparison::ManyNew(
                                prefix,
                                mf_trie_map,
                            ))]),
                            vec![],
                        ));
                    }

                    borrowed!(ctx);
                    let ((mf_value, mf_children), expanded_base_mfs) = future::try_join(
                        mf_trie_map.expand(ctx, blobstore),
                        future::try_join_all(base_mf_trie_maps.into_iter().map({
                            |parent| async move {
                                match parent {
                                    Some(parent) => parent.expand(ctx, blobstore).await,
                                    None => Ok((None, Vec::new())),
                                }
                            }
                        })),
                    )
                    .await?;
                    let (parent_values, parent_children): (Vec<_>, Vec<_>) =
                        expanded_base_mfs.into_iter().unzip();

                    let mut out = Vec::new();
                    let mut recurse = Vec::new();

                    if let Some(value) = mf_value {
                        if let Some(index) = parent_values
                            .iter()
                            .position(|parent_value| parent_value.as_ref() == Some(&value))
                        {
                            out.push(Ok(ManifestComparison::Same(
                                prefix.to_element()?,
                                value,
                                parent_values,
                                index,
                            )));
                        } else if parent_values.is_empty()
                            || parent_values.iter().all(Option::is_none)
                        {
                            out.push(Ok(ManifestComparison::New(prefix.to_element()?, value)));
                        } else {
                            out.push(Ok(ManifestComparison::Changed(
                                prefix.to_element()?,
                                value,
                                parent_values,
                            )));
                        }
                    } else if !parent_values.is_empty()
                        && !parent_values.iter().all(Option::is_none)
                    {
                        out.push(Ok(ManifestComparison::Removed(
                            prefix.to_element()?,
                            parent_values,
                        )));
                    }

                    let mut diff_iter = DiffIter::new(mf_children, parent_children);

                    while let Some((ch, child_value, child_base_mfs)) = diff_iter.next() {
                        let mut prefix = prefix.clone();
                        prefix.push(ch)?;
                        if let Some(value) = child_value {
                            if let Some(index) = child_base_mfs
                                .iter()
                                .position(|parent| parent.as_ref() == Some(&value))
                            {
                                out.push(Ok(ManifestComparison::ManySame(
                                    prefix,
                                    value,
                                    child_base_mfs,
                                    index,
                                )));
                            } else if child_base_mfs.is_empty()
                                || child_base_mfs.iter().all(|mf| mf.is_none())
                            {
                                out.push(Ok(ManifestComparison::ManyNew(prefix, value)));
                            } else {
                                recurse.push((prefix, value, child_base_mfs));
                            }
                        } else if !child_base_mfs.is_empty()
                            && !child_base_mfs
                                .iter()
                                .all(|parent| parent.as_ref().map_or(true, TrieMapOps::is_empty))
                        {
                            out.push(Ok(ManifestComparison::ManyRemoved(prefix, child_base_mfs)));
                        }
                    }

                    Ok((stream::iter(out), recurse))
                }
                .boxed()
            }
        },
    )
    .try_flatten())
}

struct DiffIter<TrieMapType> {
    mf: Peekable<<Vec<(u8, TrieMapType)> as std::iter::IntoIterator>::IntoIter>,
    base_mfs: Vec<Peekable<<Vec<(u8, TrieMapType)> as std::iter::IntoIterator>::IntoIter>>,
}

impl<TrieMapType> DiffIter<TrieMapType> {
    fn new(mf: Vec<(u8, TrieMapType)>, base_mfs: Vec<Vec<(u8, TrieMapType)>>) -> Self {
        Self {
            mf: mf.into_iter().peekable(),
            base_mfs: base_mfs
                .into_iter()
                .map(|p| p.into_iter().peekable())
                .collect(),
        }
    }

    fn next(&mut self) -> Option<(u8, Option<TrieMapType>, Vec<Option<TrieMapType>>)> {
        let mf_next_ch = self.mf.peek().map(|(k, _)| k).copied();
        let min_base_mfs_next_ch = self
            .base_mfs
            .iter_mut()
            .filter_map(|p| p.peek().map(|(k, _)| *k))
            .min();
        let next_ch = match (mf_next_ch, min_base_mfs_next_ch) {
            (None, None) => return None,
            (None, Some(ch)) => ch,
            (Some(ch), None) => ch,
            (Some(ch), Some(parent_ch)) => std::cmp::min(ch, parent_ch),
        };
        let next_mf = (Some(next_ch) == mf_next_ch)
            .then(|| self.mf.next().map(|(_, v)| v))
            .flatten();
        let next_base_mfs = self
            .base_mfs
            .iter_mut()
            .map(|p| {
                (p.peek().map(|(k, _)| *k) == Some(next_ch))
                    .then(|| p.next().map(|(_, v)| v))
                    .flatten()
            })
            .collect();
        Some((next_ch, next_mf, next_base_mfs))
    }
}

pub fn compare_manifest_tree<'a, M, Store>(
    ctx: &'a CoreContext,
    blobstore: &'a Store,
    manifest_id: M::TreeId,
    base_manifest_ids: Vec<M::TreeId>,
) -> impl Stream<Item = Result<Comparison<M::TrieMapType, Entry<M::TreeId, M::LeafId>>>> + 'a
where
    Store: Send + Sync + 'static,
    M: AsyncManifest<Store> + Send + Sync + 'static,
    M::TreeId: StoreLoadable<Store, Value = M> + Send + Sync + Eq + std::fmt::Debug + 'static,
    M::LeafId: Send + Sync + Eq + std::fmt::Debug + 'static,
    M::TrieMapType: TrieMapOps<Store, Entry<M::TreeId, M::LeafId>> + Eq,
{
    let base_manifest_ids: Vec<_> = base_manifest_ids.into_iter().map(Some).collect();
    bounded_traversal::bounded_traversal_stream(
        256,
        Some((MPath::ROOT, manifest_id, base_manifest_ids)),
        {
            move |(path, manifest_id, base_manifest_ids)| {
                async move {
                    let (manifest, base_manifests) = future::try_join(
                        manifest_id.load(ctx, blobstore),
                        future::try_join_all(base_manifest_ids.iter().map(
                            |base_manifest_id| async move {
                                match base_manifest_id {
                                    Some(base_manifest_id) => {
                                        Ok(Some(base_manifest_id.load(ctx, blobstore).await?))
                                    }
                                    None => Ok(None),
                                }
                            },
                        )),
                    )
                    .await?;
                    let mut outs = Vec::new();
                    let mut recurse = Vec::new();
                    let mut cmps =
                        compare_manifest(ctx, blobstore, manifest, base_manifests).await?;
                    while let Some(cmp) = cmps.try_next().await? {
                        match cmp {
                            ManifestComparison::New(elem, entry) => {
                                outs.push(Comparison::New(
                                    path.join_into_non_root_mpath(&elem),
                                    entry,
                                ));
                            }
                            ManifestComparison::Same(elem, entry, base_entries, index) => {
                                outs.push(Comparison::Same(
                                    path.join_into_non_root_mpath(&elem),
                                    entry,
                                    base_entries,
                                    index,
                                ));
                            }
                            ManifestComparison::Changed(elem, entry, base_entries) => match entry {
                                Entry::Tree(tree_id) => {
                                    let mut base_tree_ids = Vec::new();
                                    let mut base_leaf_entries = Vec::new();
                                    for base_entry in base_entries {
                                        match base_entry {
                                            Some(Entry::Tree(tree_id)) => {
                                                base_tree_ids.push(Some(tree_id))
                                            }
                                            Some(Entry::Leaf(_)) | None => {
                                                base_tree_ids.push(None);
                                                base_leaf_entries.push(base_entry);
                                            }
                                        }
                                    }
                                    recurse.push((path.join(&elem), tree_id, base_tree_ids));
                                    if !base_leaf_entries.is_empty() {
                                        outs.push(Comparison::Removed(
                                            path.join_into_non_root_mpath(&elem),
                                            base_leaf_entries,
                                        ));
                                    }
                                }
                                Entry::Leaf(_) => {
                                    outs.push(Comparison::Changed(
                                        path.join_into_non_root_mpath(&elem),
                                        entry,
                                        base_entries,
                                    ));
                                }
                            },
                            ManifestComparison::Removed(elem, entries) => {
                                outs.push(Comparison::Removed(
                                    path.join_into_non_root_mpath(&elem),
                                    entries,
                                ));
                            }
                            ManifestComparison::ManyNew(elem_prefix, entries) => {
                                outs.push(Comparison::ManyNew(path.clone(), elem_prefix, entries));
                            }
                            ManifestComparison::ManySame(
                                elem_prefix,
                                entries,
                                base_entries,
                                index,
                            ) => {
                                outs.push(Comparison::ManySame(
                                    path.clone(),
                                    elem_prefix,
                                    entries,
                                    base_entries,
                                    index,
                                ));
                            }
                            ManifestComparison::ManyRemoved(elem_prefix, entries) => {
                                outs.push(Comparison::ManyRemoved(
                                    path.clone(),
                                    elem_prefix,
                                    entries,
                                ));
                            }
                        }
                    }
                    anyhow::Ok((stream::iter(outs).map(Ok), recurse))
                }
                .boxed()
            }
        },
    )
    .try_flatten()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use anyhow::anyhow;
    use blobstore::Blobstore;
    use blobstore::PutBehaviour;
    use borrowed::borrowed;
    use fbinit::FacebookInit;
    use futures::stream::TryStreamExt;
    use maplit::btreemap;
    use memblob::Memblob;
    use mononoke_types::path::MPath;
    use mononoke_types::FileType;
    use mononoke_types::TrieMap;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::ops::ManifestOps;
    use crate::tests::test_manifest::derive_test_manifest;
    use crate::tests::test_manifest::TestLeafId;
    use crate::tests::test_manifest::TestManifestId;

    async fn get_trie_map(
        ctx: &CoreContext,
        blobstore: &Arc<dyn Blobstore>,
        mf: TestManifestId,
        path: &str,
        prefix: &str,
    ) -> Result<TrieMap<Entry<TestManifestId, (FileType, TestLeafId)>>> {
        let mf = mf
            .find_entry(ctx.clone(), blobstore.clone(), MPath::new(path)?)
            .await?
            .ok_or_else(|| anyhow!("path {} not found", path))?
            .into_tree()
            .ok_or_else(|| anyhow!("path {} is not a tree", path))?;
        let trie_map = mf
            .load(ctx, blobstore)
            .await?
            .into_trie_map(ctx, blobstore)
            .await?;
        trie_map
            .extract_prefix(prefix.as_bytes())
            .ok_or_else(|| anyhow!("prefix {} not found at {}", prefix, path))
    }

    async fn get_entry(
        ctx: &CoreContext,
        blobstore: &Arc<dyn Blobstore>,
        mf: TestManifestId,
        path: &str,
    ) -> Result<Entry<TestManifestId, (FileType, TestLeafId)>> {
        mf.find_entry(ctx.clone(), blobstore.clone(), MPath::new(path)?)
            .await?
            .ok_or_else(|| anyhow!("path {} not found", path))
    }

    #[fbinit::test]
    async fn test_compare_manifest_single_parent(fb: FacebookInit) -> Result<()> {
        let blobstore: Arc<dyn Blobstore> = Arc::new(Memblob::new(PutBehaviour::Overwrite));
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx, blobstore);

        let mf0 = derive_test_manifest(
            ctx,
            blobstore,
            vec![],
            btreemap! {
                "/dir1/file1" => Some("file1"),
                "/dir1/file2" =>  Some("file2"),
                "/dir2/file3" =>  Some("file3"),
                "/dir2/file4" =>  Some("file4"),
                "/dir2/dir3/file5" => Some("file5"),
                "/dir2/dir3/file6" =>  Some("file6"),
                "/dir4a/file7a" => Some("file7a"),
                "/dir4b/file7b" => Some("file7b"),
                "/file7" => Some("file7"),
                "/file8" => Some("file8"),
            },
        )
        .await?
        .unwrap();

        let mf1 = derive_test_manifest(
            ctx,
            blobstore,
            vec![mf0],
            btreemap! {
                "/dir1/file1" => Some("file1a"),
                "/dir2/file3" => None,
                "/dir2/file9" => Some("file9"),
                "/dir2/dir3/file5" => None,
                "/dir2/dir3/file6" => None,
                "/file7" => None,
                "/file7/file7" => Some("file7"),
            },
        )
        .await?
        .unwrap();

        let diff = compare_manifest(
            ctx,
            blobstore,
            mf1.load(ctx, blobstore).await?,
            vec![Some(mf0.load(ctx, blobstore).await?)],
        )
        .await?
        .try_collect::<Vec<_>>()
        .await?;

        assert_eq!(
            diff,
            vec![
                ManifestComparison::ManySame(
                    MPathElementPrefix::from_slice(b"dir4")?,
                    get_trie_map(ctx, blobstore, mf1, "", "dir4").await?,
                    vec![Some(get_trie_map(ctx, blobstore, mf0, "", "dir4").await?)],
                    0
                ),
                ManifestComparison::ManySame(
                    MPathElementPrefix::from_slice(b"file8")?,
                    get_trie_map(ctx, blobstore, mf1, "", "file8").await?,
                    vec![Some(get_trie_map(ctx, blobstore, mf0, "", "file8").await?)],
                    0
                ),
                ManifestComparison::Changed(
                    MPathElement::new_from_slice(b"dir2")?,
                    get_entry(ctx, blobstore, mf1, "dir2").await?,
                    vec![Some(get_entry(ctx, blobstore, mf0, "dir2").await?)],
                ),
                ManifestComparison::Changed(
                    MPathElement::new_from_slice(b"dir1")?,
                    get_entry(ctx, blobstore, mf1, "dir1").await?,
                    vec![Some(get_entry(ctx, blobstore, mf0, "dir1").await?)],
                ),
                ManifestComparison::Changed(
                    MPathElement::new_from_slice(b"file7")?,
                    get_entry(ctx, blobstore, mf1, "file7").await?,
                    vec![Some(get_entry(ctx, blobstore, mf0, "file7").await?)],
                ),
            ]
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_compare_manifest_tree(fb: FacebookInit) -> Result<()> {
        let blobstore: Arc<dyn Blobstore> = Arc::new(Memblob::new(PutBehaviour::Overwrite));
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx, blobstore);

        let mf0 = derive_test_manifest(
            ctx,
            blobstore,
            vec![],
            btreemap! {
                "/dir1/file1" => Some("file1"),
                "/dir1/file2" =>  Some("file2"),
                "/dir2/file3" =>  Some("file3"),
                "/dir2/file4" =>  Some("file4"),
                "/dir2/dir3/file5" => Some("file5"),
                "/dir2/dir3/file6" =>  Some("file6"),
                "/dir4a/file7a" => Some("file7a"),
                "/dir5/file8" => Some("file8"),
                "/file7" => Some("file7"),
            },
        )
        .await?
        .unwrap();

        let mf1 = derive_test_manifest(
            ctx,
            blobstore,
            vec![mf0],
            btreemap! {
                "/dir1/file1" => Some("file1a"),
                "/dir2/file3" => None,
                "/dir2/file9" => Some("file9"),
                "/dir2/dir3/file5" => None,
                "/dir2/dir3/file6" => None,
                "/file7" => None,
                "/file7/file7" => Some("file7"),
            },
        )
        .await?
        .unwrap();

        let mf2 = derive_test_manifest(
            ctx,
            blobstore,
            vec![mf0],
            btreemap! {
                "/dir1/file1" => Some("file1b"),
                "/dir1/file1c" => Some("file1c"),
            },
        )
        .await?
        .unwrap();

        let mf3 = derive_test_manifest(
            ctx,
            blobstore,
            vec![mf1, mf2],
            btreemap! {
                "/dir1/file1" => Some("file1b"),
                "/dir1/file1c" => Some("file1c"),
                "/dir5/file8" => None,
                "/file7" => Some("file7"),
            },
        )
        .await?
        .unwrap();

        let diff1 = compare_manifest_tree::<crate::tests::test_manifest::TestManifest, _>(
            ctx,
            blobstore,
            mf1,
            vec![mf0],
        )
        .try_collect::<Vec<_>>()
        .await?;

        assert_eq!(
            diff1,
            vec![
                Comparison::ManySame(
                    MPath::ROOT,
                    MPathElementPrefix::from_slice(b"dir4")?,
                    get_trie_map(ctx, blobstore, mf1, "", "dir4").await?,
                    vec![Some(get_trie_map(ctx, blobstore, mf0, "", "dir4").await?)],
                    0
                ),
                Comparison::ManySame(
                    MPath::ROOT,
                    MPathElementPrefix::from_slice(b"dir5")?,
                    get_trie_map(ctx, blobstore, mf1, "", "dir5").await?,
                    vec![Some(get_trie_map(ctx, blobstore, mf0, "", "dir5").await?)],
                    0
                ),
                Comparison::Removed(
                    NonRootMPath::new("file7")?,
                    vec![Some(get_entry(ctx, blobstore, mf0, "file7").await?)],
                ),
                Comparison::ManyNew(
                    MPath::new("file7")?,
                    MPathElementPrefix::from_slice(b"")?,
                    get_trie_map(ctx, blobstore, mf1, "file7", "").await?,
                ),
                Comparison::ManySame(
                    MPath::new("dir1")?,
                    MPathElementPrefix::from_slice(b"file2")?,
                    get_trie_map(ctx, blobstore, mf1, "dir1", "file2").await?,
                    vec![Some(
                        get_trie_map(ctx, blobstore, mf0, "dir1", "file2").await?
                    )],
                    0
                ),
                Comparison::Changed(
                    NonRootMPath::new("dir1/file1")?,
                    get_entry(ctx, blobstore, mf1, "dir1/file1").await?,
                    vec![Some(get_entry(ctx, blobstore, mf0, "dir1/file1").await?,)],
                ),
                Comparison::ManyRemoved(
                    MPath::new("dir2")?,
                    MPathElementPrefix::from_slice(b"d")?,
                    vec![Some(get_trie_map(ctx, blobstore, mf0, "dir2", "d").await?)],
                ),
                Comparison::ManyRemoved(
                    MPath::new("dir2")?,
                    MPathElementPrefix::from_slice(b"file3")?,
                    vec![Some(
                        get_trie_map(ctx, blobstore, mf0, "dir2", "file3").await?
                    )],
                ),
                Comparison::ManySame(
                    MPath::new("dir2")?,
                    MPathElementPrefix::from_slice(b"file4")?,
                    get_trie_map(ctx, blobstore, mf1, "dir2", "file4").await?,
                    vec![Some(
                        get_trie_map(ctx, blobstore, mf0, "dir2", "file4").await?
                    )],
                    0
                ),
                Comparison::ManyNew(
                    MPath::new("dir2")?,
                    MPathElementPrefix::from_slice(b"file9")?,
                    get_trie_map(ctx, blobstore, mf1, "dir2", "file9").await?,
                ),
            ]
        );

        let diff2 = compare_manifest_tree::<crate::tests::test_manifest::TestManifest, _>(
            ctx,
            blobstore,
            mf2,
            vec![mf0],
        )
        .try_collect::<Vec<_>>()
        .await?;

        assert_eq!(
            diff2,
            vec![
                Comparison::ManySame(
                    MPath::ROOT,
                    MPathElementPrefix::from_slice(b"f")?,
                    get_trie_map(ctx, blobstore, mf2, "", "f").await?,
                    vec![Some(get_trie_map(ctx, blobstore, mf0, "", "f").await?)],
                    0
                ),
                Comparison::ManySame(
                    MPath::ROOT,
                    MPathElementPrefix::from_slice(b"dir2")?,
                    get_trie_map(ctx, blobstore, mf2, "", "dir2").await?,
                    vec![Some(get_trie_map(ctx, blobstore, mf0, "", "dir2").await?)],
                    0
                ),
                Comparison::ManySame(
                    MPath::ROOT,
                    MPathElementPrefix::from_slice(b"dir4")?,
                    get_trie_map(ctx, blobstore, mf2, "", "dir4").await?,
                    vec![Some(get_trie_map(ctx, blobstore, mf0, "", "dir4").await?)],
                    0
                ),
                Comparison::ManySame(
                    MPath::ROOT,
                    MPathElementPrefix::from_slice(b"dir5")?,
                    get_trie_map(ctx, blobstore, mf2, "", "dir5").await?,
                    vec![Some(get_trie_map(ctx, blobstore, mf0, "", "dir5").await?)],
                    0
                ),
                Comparison::ManySame(
                    MPath::new("dir1")?,
                    MPathElementPrefix::from_slice(b"file2")?,
                    get_trie_map(ctx, blobstore, mf2, "dir1", "file2").await?,
                    vec![Some(
                        get_trie_map(ctx, blobstore, mf0, "dir1", "file2").await?
                    )],
                    0
                ),
                Comparison::Changed(
                    NonRootMPath::new("dir1/file1")?,
                    get_entry(ctx, blobstore, mf2, "dir1/file1").await?,
                    vec![Some(get_entry(ctx, blobstore, mf0, "dir1/file1").await?)],
                ),
                Comparison::ManyNew(
                    MPath::new("dir1")?,
                    MPathElementPrefix::from_slice(b"file1c")?,
                    get_trie_map(ctx, blobstore, mf2, "dir1", "file1c").await?,
                ),
            ]
        );

        let diff3 = compare_manifest_tree::<crate::tests::test_manifest::TestManifest, _>(
            ctx,
            blobstore,
            mf3,
            vec![mf1, mf2],
        )
        .try_collect::<Vec<_>>()
        .await?;

        assert_eq!(
            diff3,
            vec![
                Comparison::ManySame(
                    MPath::ROOT,
                    MPathElementPrefix::from_slice(b"f")?,
                    get_trie_map(ctx, blobstore, mf3, "", "f").await?,
                    vec![
                        Some(get_trie_map(ctx, blobstore, mf1, "", "f").await?),
                        Some(get_trie_map(ctx, blobstore, mf2, "", "f").await?)
                    ],
                    1
                ),
                Comparison::ManySame(
                    MPath::ROOT,
                    MPathElementPrefix::from_slice(b"dir1")?,
                    get_trie_map(ctx, blobstore, mf3, "", "dir1").await?,
                    vec![
                        Some(get_trie_map(ctx, blobstore, mf1, "", "dir1").await?),
                        Some(get_trie_map(ctx, blobstore, mf2, "", "dir1").await?)
                    ],
                    1
                ),
                Comparison::ManySame(
                    MPath::ROOT,
                    MPathElementPrefix::from_slice(b"dir4")?,
                    get_trie_map(ctx, blobstore, mf3, "", "dir4").await?,
                    vec![
                        Some(get_trie_map(ctx, blobstore, mf1, "", "dir4").await?),
                        Some(get_trie_map(ctx, blobstore, mf2, "", "dir4").await?)
                    ],
                    0
                ),
                Comparison::ManyRemoved(
                    MPath::ROOT,
                    MPathElementPrefix::from_slice(b"dir5")?,
                    vec![
                        Some(get_trie_map(ctx, blobstore, mf1, "", "dir5").await?),
                        Some(get_trie_map(ctx, blobstore, mf2, "", "dir5").await?)
                    ],
                ),
                Comparison::ManySame(
                    MPath::new("dir2")?,
                    MPathElementPrefix::from_slice(b"d")?,
                    get_trie_map(ctx, blobstore, mf3, "dir2", "d").await?,
                    vec![
                        None,
                        Some(get_trie_map(ctx, blobstore, mf2, "dir2", "d").await?)
                    ],
                    1
                ),
                Comparison::ManySame(
                    MPath::new("dir2")?,
                    MPathElementPrefix::from_slice(b"file3")?,
                    get_trie_map(ctx, blobstore, mf3, "dir2", "file3").await?,
                    vec![
                        None,
                        Some(get_trie_map(ctx, blobstore, mf2, "dir2", "file3").await?)
                    ],
                    1
                ),
                Comparison::ManySame(
                    MPath::new("dir2")?,
                    MPathElementPrefix::from_slice(b"file4")?,
                    get_trie_map(ctx, blobstore, mf3, "dir2", "file4").await?,
                    vec![
                        Some(get_trie_map(ctx, blobstore, mf1, "dir2", "file4").await?),
                        Some(get_trie_map(ctx, blobstore, mf2, "dir2", "file4").await?)
                    ],
                    0
                ),
                Comparison::ManySame(
                    MPath::new("dir2")?,
                    MPathElementPrefix::from_slice(b"file9")?,
                    get_trie_map(ctx, blobstore, mf3, "dir2", "file9").await?,
                    vec![
                        Some(get_trie_map(ctx, blobstore, mf1, "dir2", "file9").await?),
                        None
                    ],
                    0
                ),
            ]
        );

        Ok(())
    }
}
