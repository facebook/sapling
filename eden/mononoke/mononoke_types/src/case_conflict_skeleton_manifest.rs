/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use cloned::cloned;
use context::CoreContext;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::FutureExt;

use crate::blob::CaseConflictSkeletonManifestBlob;
use crate::sharded_map_v2::Rollup;
use crate::sharded_map_v2::ShardedMapV2Node;
use crate::sharded_map_v2::ShardedMapV2Value;
use crate::thrift;
use crate::typed_hash::CaseConflictSkeletonManifestContext;
use crate::typed_hash::CaseConflictSkeletonManifestId;
use crate::typed_hash::IdContext;
use crate::typed_hash::ShardedMapV2NodeCcsmContext;
pub use crate::typed_hash::ShardedMapV2NodeCcsmId;
use crate::Blob;
use crate::BlobstoreValue;
use crate::MPath;
use crate::MPathElement;
use crate::NonRootMPath;
use crate::PrefixTrie;
use crate::ThriftConvert;

#[derive(ThriftConvert, Debug, Clone, PartialEq, Eq, Hash)]
#[thrift(thrift::ccsm::CcsmEntry)]
pub enum CcsmEntry {
    #[thrift(thrift::ccsm::CcsmFile)]
    File,
    Directory(CaseConflictSkeletonManifest),
}

#[derive(ThriftConvert, Debug, Clone, PartialEq, Eq, Hash)]
#[thrift(thrift::ccsm::CaseConflictSkeletonManifest)]
pub struct CaseConflictSkeletonManifest {
    pub subentries: ShardedMapV2Node<CcsmEntry>,
}

impl CcsmEntry {
    pub fn into_dir(self) -> Option<CaseConflictSkeletonManifest> {
        match self {
            Self::File => None,
            Self::Directory(dir) => Some(dir),
        }
    }

    pub fn dir(&self) -> Option<&CaseConflictSkeletonManifest> {
        match self {
            Self::File => None,
            Self::Directory(dir) => Some(dir),
        }
    }

    pub fn rollup_counts(&self) -> CcsmRollupCounts {
        match self {
            Self::File => CcsmRollupCounts {
                descendants_count: 1,
                odd_depth_conflicts: 0,
                even_depth_conflicts: 0,
            },
            Self::Directory(dir) => dir.rollup_counts(),
        }
    }
}

#[async_trait]
impl Loadable for CaseConflictSkeletonManifest {
    type Value = CaseConflictSkeletonManifest;

    async fn load<'a, B: Blobstore>(
        &'a self,
        _ctx: &'a CoreContext,
        _blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError> {
        Ok(self.clone())
    }
}

impl ShardedMapV2Value for CcsmEntry {
    type NodeId = ShardedMapV2NodeCcsmId;
    type Context = ShardedMapV2NodeCcsmContext;
    type RollupData = CcsmRollupCounts;

    const WEIGHT_LIMIT: usize = 1000;

    // The weight function is overridden because the sharded map is stored
    // inlined in CaseConflictSkeletonManifest. So the weight of the sharded map
    // should be propagated to make sure each sharded map blob stays
    // within the weight limit.
    fn weight(&self) -> usize {
        match self {
            Self::File => 1,
            // This `1 +` is needed to offset the extra space required for
            // the bytes that represent the path element to this directory.
            Self::Directory(dir) => 1 + dir.subentries.weight(),
        }
    }
}

#[derive(ThriftConvert, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[thrift(thrift::ccsm::CcsmRollupCounts)]
pub struct CcsmRollupCounts {
    /// The total number of descendant files and directories for this manifest,
    /// including this manifest itself.
    pub descendants_count: u64,
    /// The number of descendants that have more than one child and have odd depth (i.e. children or 3rd level descendants or ...)
    pub odd_depth_conflicts: u64,
    /// The number of descendants that have more than one child and have even depth (i.e. the manifest itself or 2nd level descendants or ...)
    pub even_depth_conflicts: u64,
}

impl Rollup<CcsmEntry> for CcsmRollupCounts {
    fn rollup(entry: Option<&CcsmEntry>, child_rollup_data: Vec<Self>) -> Self {
        child_rollup_data.into_iter().fold(
            entry.map_or(
                CcsmRollupCounts {
                    descendants_count: 0,
                    odd_depth_conflicts: 0,
                    even_depth_conflicts: 0,
                },
                |entry| entry.rollup_counts(),
            ),
            |acc, child| CcsmRollupCounts {
                descendants_count: acc.descendants_count + child.descendants_count,
                odd_depth_conflicts: acc.odd_depth_conflicts + child.odd_depth_conflicts,
                even_depth_conflicts: acc.even_depth_conflicts + child.even_depth_conflicts,
            },
        )
    }
}

impl CaseConflictSkeletonManifest {
    pub fn empty() -> Self {
        Self {
            subentries: ShardedMapV2Node::default(),
        }
    }

    pub async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        name: &MPathElement,
    ) -> Result<Option<CcsmEntry>> {
        self.subentries.lookup(ctx, blobstore, name.as_ref()).await
    }

    pub fn into_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
    ) -> BoxStream<'a, Result<(MPathElement, CcsmEntry)>> {
        self.subentries
            .into_entries(ctx, blobstore)
            .and_then(|(k, v)| async move { anyhow::Ok((MPathElement::from_smallvec(k)?, v)) })
            .boxed()
    }

    pub fn into_subentries_skip<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        skip: usize,
    ) -> BoxStream<'a, Result<(MPathElement, CcsmEntry)>> {
        self.subentries
            .into_entries_skip(ctx, blobstore, skip)
            .and_then(|(k, v)| async move { anyhow::Ok((MPathElement::from_smallvec(k)?, v)) })
            .boxed()
    }

    pub fn into_prefix_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        prefix: &'a [u8],
    ) -> BoxStream<'a, Result<(MPathElement, CcsmEntry)>> {
        self.subentries
            .into_prefix_entries(ctx, blobstore, prefix)
            .map(|res| res.and_then(|(k, v)| anyhow::Ok((MPathElement::from_smallvec(k)?, v))))
            .boxed()
    }

    pub fn into_prefix_subentries_after<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        prefix: &'a [u8],
        after: &'a [u8],
    ) -> BoxStream<'a, Result<(MPathElement, CcsmEntry)>> {
        self.subentries
            .into_prefix_entries_after(ctx, blobstore, prefix, after)
            .map(|res| res.and_then(|(k, v)| anyhow::Ok((MPathElement::from_smallvec(k)?, v))))
            .boxed()
    }

    /// Finds two case conflicting paths in the manifest that weren't already
    /// present in any of the parent manifests. The paths returned will not
    /// start with any of the prefixes specified by `excluded_paths`.
    ///
    /// Returns `None` if no new case conflicts were found.
    pub async fn find_new_case_conflict(
        self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        parent_manifests: Vec<Self>,
        excluded_paths: &PrefixTrie,
    ) -> Result<Option<(NonRootMPath, NonRootMPath)>> {
        bounded_traversal::bounded_traversal(
            100,
            (MPath::ROOT, self, parent_manifests),
            |(path, manifest, parent_manifests)| {
                async move {
                    // Path is excluded from case conflict checking.
                    if excluded_paths.contains_prefix(path.as_ref()) {
                        return Ok((None, vec![]));
                    }

                    // If the manifest has no odd depth descendants with more than one child, then
                    // there are no case conflicts.
                    if manifest.rollup_counts().odd_depth_conflicts == 0 {
                        return Ok((None, vec![]));
                    }

                    // Diff the current manifest against the parent manifests. This
                    // diff is happening on the level of the lowercased path elements.
                    let difference_items = manifest
                        .subentries
                        .difference_stream(
                            ctx,
                            blobstore,
                            parent_manifests
                                .into_iter()
                                .map(|manifest| manifest.subentries)
                                .collect(),
                            100,
                        )
                        .try_collect::<Vec<_>>()
                        .await?;

                    let mut recurse_subentries = vec![];

                    for (_, difference_item) in difference_items {
                        let child_manifest = match difference_item.current_value {
                            CcsmEntry::Directory(child_manifest) => child_manifest,
                            CcsmEntry::File => continue,
                        };

                        // If this lowercased path element has more than one child, then
                        // there's a case conflict. If none of the parent manifests have
                        // more than one child, then this is a new case conflict.
                        if child_manifest.subentries.size() > 1
                            && difference_item.previous_values.iter().all(|parent_entry| {
                                parent_entry
                                    .dir()
                                    .map_or(true, |dir| dir.subentries.size() <= 1)
                            })
                        {
                            // Load the first two children of the child manifest as
                            // any two will form a case conflict.
                            let mut subentries = child_manifest
                                .into_subentries(ctx, blobstore)
                                .take(2)
                                .try_collect::<Vec<_>>()
                                .await?
                                .into_iter();

                            let (first_path_element, _) = subentries.next().unwrap();
                            let (second_path_element, _) = subentries.next().unwrap();

                            let first_path = path.join_into_non_root_mpath(&first_path_element);
                            let second_path = path.join_into_non_root_mpath(&second_path_element);

                            return Ok((Some((first_path, second_path)), vec![]));
                        } else {
                            recurse_subentries.push((
                                child_manifest,
                                difference_item
                                    .previous_values
                                    .into_iter()
                                    .flat_map(|parent_entry| parent_entry.into_dir())
                                    .collect::<Vec<_>>(),
                            ));
                        }
                    }

                    let recurse_subentries = stream::iter(recurse_subentries)
                        .map(|(manifest, parent_manifests)| {
                            cloned!(path);
                            async move {
                                // Diff the current manifest against the parent manifests. This diff
                                // is happening on the level of the original path elements so there
                                // will usually be only one child as case conflicts are rare.
                                let difference_items = manifest
                                    .subentries
                                    .difference_stream(
                                        ctx,
                                        blobstore,
                                        parent_manifests
                                            .into_iter()
                                            .map(|manifest| manifest.subentries)
                                            .collect(),
                                        100,
                                    )
                                    .try_collect::<Vec<_>>()
                                    .await?;

                                anyhow::Ok(
                                    difference_items
                                        .into_iter()
                                        .map(|(element, item)| {
                                            let element = MPathElement::from_smallvec(element)?;

                                            let path = path.join_element(Some(&element));
                                            let manifest = match item.current_value.into_dir() {
                                                Some(manifest) => manifest,
                                                None => return Ok(None),
                                            };

                                            let parent_manifests = item
                                                .previous_values
                                                .into_iter()
                                                .filter_map(|entry| entry.into_dir())
                                                .collect::<Vec<_>>();

                                            Ok(Some((path, manifest, parent_manifests)))
                                        })
                                        .collect::<Result<Vec<_>>>()?
                                        .into_iter()
                                        .flatten()
                                        .collect::<Vec<_>>(),
                                )
                            }
                        })
                        .buffered(100)
                        .try_collect::<Vec<_>>()
                        .await?
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>();

                    Ok((None, recurse_subentries))
                }
                .boxed()
            },
            |maybe_conflict: Option<(NonRootMPath, NonRootMPath)>, child_conflicts| {
                async move {
                    Ok(
                        child_conflicts.fold(maybe_conflict, |conflict, child_conflict| {
                            conflict.or(child_conflict)
                        }),
                    )
                }
                .boxed()
            },
        )
        .await
    }

    pub fn rollup_counts(&self) -> CcsmRollupCounts {
        let sharded_map_rollup_data = self.subentries.rollup_data();
        CcsmRollupCounts {
            descendants_count: sharded_map_rollup_data.descendants_count + 1,
            // even depth conflicts for children are odd depth conflicts for this manifest
            odd_depth_conflicts: sharded_map_rollup_data.even_depth_conflicts,
            // odd depth conflicts for children are even depth conflicts for this manifest
            // plus one if this manifest itself has more than one child.
            even_depth_conflicts: if self.subentries.size() > 1 {
                sharded_map_rollup_data.odd_depth_conflicts + 1
            } else {
                sharded_map_rollup_data.odd_depth_conflicts
            },
        }
    }
}

impl BlobstoreValue for CaseConflictSkeletonManifest {
    type Key = CaseConflictSkeletonManifestId;

    fn into_blob(self) -> CaseConflictSkeletonManifestBlob {
        let data = self.into_bytes();
        let id = CaseConflictSkeletonManifestContext::id_from_data(&data);
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data())
    }
}
