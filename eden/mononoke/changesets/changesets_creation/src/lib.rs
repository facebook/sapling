/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::Result;
use anyhow::anyhow;
use blobstore::Blobstore;
use cloned::cloned;
use commit_graph::ChangesetParents;
use commit_graph::ChangesetSubtreeSources;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriterRef;
use context::CoreContext;
use futures::future::try_join;
use futures::stream::FuturesUnordered;
use futures::stream::TryStreamExt;
use mononoke_types::BlobstoreKey;
use mononoke_types::BlobstoreValue;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use repo_blobstore::RepoBlobstoreRef;
use repo_identity::RepoIdentityRef;
use topo_sort::sort_topological;
use vec1::Vec1;

/// Upload bonsai changesets to the blobstore in parallel, and then store them
/// in the changesets table.
///
/// Parents of the changesets should already by saved in the repository.
pub async fn save_changesets(
    ctx: &CoreContext,
    repo: &(impl CommitGraphRef + CommitGraphWriterRef + RepoBlobstoreRef + RepoIdentityRef),
    bonsai_changesets: Vec<BonsaiChangeset>,
) -> Result<()> {
    let commit_graph = repo.commit_graph();
    let blobstore = repo.repo_blobstore();

    if bonsai_changesets
        .iter()
        .any(|bcs| bcs.has_subtree_changes())
        && !justknobs::eval(
            "scm/mononoke:enable_subtree_changes",
            None,
            Some(repo.repo_identity().name()),
        )
        .unwrap_or(false)
    {
        return Err(anyhow!("Subtree changes are disabled"));
    }

    if bonsai_changesets
        .iter()
        .any(|bcs| bcs.has_manifest_altering_subtree_changes())
        && !justknobs::eval(
            "scm/mononoke:enable_manifest_altering_subtree_changes",
            None,
            Some(repo.repo_identity().name()),
        )
        .unwrap_or(false)
    {
        return Err(anyhow!("Subtree changes that alter manifests are disabled"));
    }

    let mut parents_to_check: HashSet<ChangesetId> = HashSet::new();
    for bcs in &bonsai_changesets {
        parents_to_check.extend(bcs.parents());
    }
    // Remove commits that we are uploading in this batch
    for bcs in &bonsai_changesets {
        parents_to_check.remove(&bcs.get_changeset_id());
    }

    let parents_to_check = parents_to_check
        .into_iter()
        .map({
            |p| async move {
                let exists = commit_graph.exists(ctx, p).await?;
                if exists {
                    Ok(())
                } else {
                    Err(anyhow!("Commit {} does not exist in the repo", p))
                }
            }
        })
        .collect::<FuturesUnordered<_>>()
        .try_collect::<Vec<_>>();

    let bonsai_changesets: HashMap<_, _> = bonsai_changesets
        .into_iter()
        .map(|bcs| (bcs.get_changeset_id(), bcs))
        .collect();

    // Order of inserting entries in changeset table matters though, so we first need to
    // topologically sort commits.
    let mut bcs_deps = HashMap::new();
    let mut bcs_parents_and_subtree_sources = HashMap::new();
    for bcs in bonsai_changesets.values() {
        let parents = bcs.parents().collect::<ChangesetParents>();
        let subtree_sources = bcs.subtree_sources().collect::<ChangesetSubtreeSources>();
        let deps = parents
            .iter()
            .chain(subtree_sources.iter())
            .copied()
            .collect::<Vec<_>>();
        bcs_deps.insert(bcs.get_changeset_id(), deps);
        bcs_parents_and_subtree_sources.insert(bcs.get_changeset_id(), (parents, subtree_sources));
    }

    // Order of inserting bonsai changesets objects doesn't matter, so we can join them
    let bonsai_objects = bonsai_changesets
        .into_iter()
        .map({
            |(_, bcs)| {
                cloned!(blobstore);
                async move {
                    let bonsai_blob = bcs.into_blob();
                    let bcs_id = bonsai_blob.id().clone();
                    let blobstore_key = bcs_id.blobstore_key();
                    blobstore
                        .put(ctx, blobstore_key, bonsai_blob.into())
                        .await?;
                    Ok(())
                }
            }
        })
        .collect::<FuturesUnordered<_>>()
        .try_collect::<Vec<_>>();

    try_join(bonsai_objects, parents_to_check).await?;

    let topo_sorted_commits = sort_topological(&bcs_deps).expect("loop in commit chain!");
    let entries = topo_sorted_commits
        .into_iter()
        .filter_map(|bcs_id| {
            bcs_parents_and_subtree_sources
                .remove(&bcs_id)
                .map(|(parents, subtree_sources)| (bcs_id, parents, subtree_sources))
        })
        .collect::<Vec<_>>();
    if let Ok(entries) = Vec1::try_from(entries) {
        repo.commit_graph_writer().add_many(ctx, entries).await?;
    }

    Ok(())
}
