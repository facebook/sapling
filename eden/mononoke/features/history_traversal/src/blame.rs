/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Error;
use async_recursion::async_recursion;
use blame::fetch_blame_compat;
use blame::fetch_content_for_blame;
use blame::BlameError;
use blame::CompatBlame;
use bytes::Bytes;
use context::CoreContext;
use futures::stream;
use futures::stream::TryStreamExt;
use futures::try_join;
use manifest::ManifestOps;
use mononoke_types::blame_v2::BlameParent;
use mononoke_types::blame_v2::BlameV2;
use mononoke_types::ChangesetId;
use mononoke_types::FileUnodeId;
use mononoke_types::MPath;
use reachabilityindex::ReachabilityIndex;
use std::collections::HashSet;
use unodes::RootUnodeManifestId;

use crate::common::find_possible_mutable_ancestors;
use crate::Repo;

#[async_recursion]
async fn fetch_mutable_blame(
    ctx: &CoreContext,
    repo: &impl Repo,
    my_csid: ChangesetId,
    path: &MPath,
    seen: &mut HashSet<ChangesetId>,
) -> Result<(CompatBlame, FileUnodeId), BlameError> {
    let mutable_renames = repo.mutable_renames();

    if !seen.insert(my_csid) {
        return Err(anyhow!("Infinite loop in mutable blame").into());
    }

    // First case. Fix up blame directly if I have a mutable rename attached
    let my_mutable_rename = mutable_renames
        .get_rename(ctx, my_csid, Some(path.clone()))
        .await?;
    if let Some(rename) = my_mutable_rename {
        // We have a mutable rename, which replaces our p1 and our path.
        // Recurse to fetch a fully mutated blame for the new p1 parent
        // and path.
        //
        // This covers the case where we are a in the immutable history:
        // a
        // |
        // b  e
        // |  |
        // c  d
        // and there is a mutable rename saying that a's parent should be e, not b.
        // After this, because we did the blame a->e, and we fetched a mutant blame
        // for e, we're guaranteed to be done, even if there are mutations in e's history.
        let src_path = rename
            .src_path()
            .ok_or_else(|| anyhow!("Mutable rename points file to root directory"))?
            .clone();
        let (compat_blame, src_content) =
            blame_with_content(ctx, repo, rename.src_cs_id(), rename.src_path(), true).await?;
        let src_blame = extract_blame_v2_from_compat(compat_blame)?;

        let blobstore = repo.repo_blobstore_arc();
        let unode = repo
            .repo_derived_data()
            .derive::<RootUnodeManifestId>(ctx, my_csid)
            .await?
            .manifest_unode_id()
            .find_entry(ctx.clone(), blobstore, Some(path.clone()))
            .await?
            .context("Unode missing")?
            .into_leaf()
            .ok_or_else(|| BlameError::IsDirectory(path.clone()))?;
        let my_content = fetch_content_for_blame(ctx, repo.as_blob_repo(), unode)
            .await?
            .into_bytes()?;

        // And reblame directly against the parent mutable renames gave us.
        let blame_parent = BlameParent::new(0, src_path, src_content, src_blame);
        let blame = BlameV2::new(my_csid, path.clone(), my_content, vec![blame_parent])?;
        return Ok((CompatBlame::V2(blame), unode));
    }

    // Second case. We don't have a mutable rename attached, so we're going to look
    // at the set of mutable renames for this path, and if any of those renames are ancestors
    // of this commit, we'll apply a mutated blame via BlameV2::apply_mutable_blame to
    // get the final blame result.

    // Check for historic mutable renames - those attached to commits older than us.
    // Given our history graph:
    // a
    // |
    // b
    // |
    // c
    // |\
    // d e
    // where we are b, this looks to see any if c, d, e (etc) has a mutable rename attached to
    // it that affects our current path.
    //
    // We then filter down to remove mutable renames that are ancestors of the currently handled
    // mutable rename, since recursing to get blame will fix those. We can then apply mutation
    // for each blame in any order, because the mutated blame will only affect one ancestry path.
    //
    // For example, if c has a mutable rename for our path, then we do not want to consider mutable
    // renames attached to d or e; however, if c does not, but d and e do, then we want to consider
    // the mutable renames for both d and e.
    let mut possible_mutable_ancestors =
        find_possible_mutable_ancestors(ctx, repo, my_csid, Some(path)).await?;

    // Fetch the immutable blame, which we're going to mutate
    let (blame, unode) = fetch_immutable_blame(ctx, repo, my_csid, path).await?;
    let mut my_blame = extract_blame_v2_from_compat(blame)?;

    // We now have a stack of possible mutable ancestors, sorted so that the highest generation
    // is last. We now pop the last entry from the stack (highest generation) and apply mutation
    // based on that entry. Once that's done, we remove all ancestors of the popped entry
    // from the stack, so that we don't attempt to double-apply a mutation.
    //
    // This will mutate our blame to have all appropriate mutations from ancestors applied
    // If we have mutable blame down two ancestors of a merge, we'd expect that the order
    // of applying those mutations will not affect the final result
    let skiplist_index = repo.skiplist_index();
    while let Some((_, mutated_csid)) = possible_mutable_ancestors.pop() {
        // Apply mutation for mutated_csid
        let ((mutated_blame, _), (original_blame, _)) = try_join!(
            fetch_mutable_blame(ctx, repo, mutated_csid, path, seen),
            fetch_immutable_blame(ctx, repo, mutated_csid, path)
        )?;
        let original_blame = extract_blame_v2_from_compat(original_blame)?;
        let mutated_blame = extract_blame_v2_from_compat(mutated_blame)?;
        my_blame.apply_mutable_change(&original_blame, &mutated_blame)?;

        // Rebuild possible_mutable_ancestors without anything that's an ancestor
        // of mutated_csid. This must preserve order, so that we deal with the most
        // recent mutation entries first (which may well remove older mutation entries
        // from the stack)
        possible_mutable_ancestors =
            stream::iter(possible_mutable_ancestors.into_iter().map(anyhow::Ok))
                .try_filter_map({
                    move |(gen, csid)| async move {
                        if skiplist_index
                            .query_reachability(
                                ctx,
                                &repo.changeset_fetcher_arc(),
                                mutated_csid,
                                csid,
                            )
                            .await?
                        {
                            anyhow::Ok(None)
                        } else {
                            Ok(Some((gen, csid)))
                        }
                    }
                })
                .try_collect()
                .await?;
    }

    Ok((CompatBlame::V2(my_blame), unode))
}

async fn fetch_immutable_blame(
    ctx: &CoreContext,
    repo: &impl Repo,
    csid: ChangesetId,
    path: &MPath,
) -> Result<(CompatBlame, FileUnodeId), BlameError> {
    fetch_blame_compat(ctx, repo.as_blob_repo(), csid, path.clone()).await
}

pub async fn blame(
    ctx: &CoreContext,
    repo: &impl Repo,
    csid: ChangesetId,
    path: Option<&MPath>,
    follow_mutable_file_history: bool,
) -> Result<(CompatBlame, FileUnodeId), BlameError> {
    let path = path.ok_or_else(|| anyhow!("Blame is not available for directory: `/`"))?;
    if follow_mutable_file_history {
        fetch_mutable_blame(ctx, repo, csid, path, &mut HashSet::new()).await
    } else {
        fetch_immutable_blame(ctx, repo, csid, path).await
    }
}

/// Blame metadata for this path, and the content that was blamed.  If the file
/// content is too large or binary data is detected then
//  the fetch may be rejected.
pub async fn blame_with_content(
    ctx: &CoreContext,
    repo: &impl Repo,
    csid: ChangesetId,
    path: Option<&MPath>,
    follow_mutable_file_history: bool,
) -> Result<(CompatBlame, Bytes), BlameError> {
    let (blame, file_unode_id) = blame(ctx, repo, csid, path, follow_mutable_file_history).await?;
    let content = fetch_content_for_blame(ctx, repo.as_blob_repo(), file_unode_id)
        .await?
        .into_bytes()?;
    Ok((blame, content))
}

fn extract_blame_v2_from_compat(blame: CompatBlame) -> Result<BlameV2, Error> {
    if let CompatBlame::V2(blame) = blame {
        Ok(blame)
    } else {
        bail!("Mutable blame only works with blame V2. Ask Source Control oncall for help")
    }
}
