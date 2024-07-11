/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::anyhow;
use anyhow::Result;
use blobstore::Blobstore;
use changesets::ChangesetInsert;
use changesets::ChangesetsRef;
use cloned::cloned;
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
    repo: &(impl ChangesetsRef + RepoBlobstoreRef + RepoIdentityRef),
    bonsai_changesets: Vec<BonsaiChangeset>,
) -> Result<()> {
    let complete_changesets = repo.changesets();
    let blobstore = repo.repo_blobstore();

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
            |p| {
                cloned!(complete_changesets);
                async move {
                    let exists = complete_changesets.exists(ctx, p).await?;
                    if exists {
                        Ok(())
                    } else {
                        Err(anyhow!("Commit {} does not exist in the repo", p))
                    }
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
    let mut bcs_parents = HashMap::new();
    for bcs in bonsai_changesets.values() {
        let parents: Vec<_> = bcs.parents().collect();
        bcs_parents.insert(bcs.get_changeset_id(), parents);
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

    let topo_sorted_commits = sort_topological(&bcs_parents).expect("loop in commit chain!");
    if justknobs::eval(
        "scm/mononoke:save_changesets_use_batch_inserts",
        None,
        Some(repo.repo_identity().name()),
    )? {
        let entries = topo_sorted_commits
            .into_iter()
            .filter_map(|bcs_id| {
                bcs_parents.get(&bcs_id).map(|parents| ChangesetInsert {
                    cs_id: bcs_id,
                    parents: parents.to_vec(),
                })
            })
            .collect::<Vec<_>>();
        if let Ok(entries) = Vec1::try_from(entries) {
            complete_changesets.add_many(ctx, entries).await?;
        }
    } else {
        for bcs_id in topo_sorted_commits {
            if let Some(parents) = bcs_parents.get(&bcs_id) {
                let completion_record = ChangesetInsert {
                    cs_id: bcs_id,
                    parents: parents.to_vec(),
                };
                complete_changesets.add(ctx, completion_record).await?;
            }
        }
    }

    Ok(())
}
