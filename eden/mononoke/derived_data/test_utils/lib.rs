/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bounded_traversal::bounded_traversal_stream;
use context::CoreContext;
use futures::future::FutureExt;
use futures::stream;
use futures::stream::Stream;
use futures::stream::TryStreamExt;
use manifest::Entry;
use manifest::Manifest;
use mercurial_types::HgChangesetId;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use repo_blobstore::RepoBlobstoreRef;

pub async fn bonsai_changeset_from_hg(
    ctx: &CoreContext,
    repo: impl RepoBlobstoreRef + BonsaiHgMappingRef,
    s: &str,
) -> Result<(ChangesetId, BonsaiChangeset)> {
    let hg_cs_id = s.parse::<HgChangesetId>()?;
    let bcs_id = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(ctx, hg_cs_id)
        .await?
        .ok_or_else(|| anyhow!("Failed to find bonsai changeset id for {}", hg_cs_id))?;
    let bcs = bcs_id.load(ctx, repo.repo_blobstore()).await?;
    Ok((bcs_id, bcs))
}

pub fn iterate_all_manifest_entries<'a, MfId, LId>(
    ctx: &'a CoreContext,
    repo: impl RepoBlobstoreRef + Copy + Send + Sync + 'a,
    entry: Entry<MfId, LId>,
) -> impl Stream<Item = Result<(Option<MPath>, Entry<MfId, LId>)>> + 'a
where
    MfId: Loadable + Send + Sync + Clone + 'a,
    LId: Send + Clone + 'static,
    <MfId as Loadable>::Value: Manifest<TreeId = MfId, LeafId = LId>,
{
    bounded_traversal_stream(256, Some((None, entry)), move |(path, entry)| {
        async move {
            match entry {
                Entry::Leaf(_) => Ok((vec![(path, entry.clone())], vec![])),
                Entry::Tree(tree) => {
                    let mf = tree.load(ctx, repo.repo_blobstore()).await?;
                    let recurse = mf
                        .list()
                        .map(|(basename, new_entry)| {
                            let path = MPath::join_opt_element(path.as_ref(), &basename);
                            (Some(path), new_entry)
                        })
                        .collect();

                    Ok::<_, Error>((vec![(path, Entry::Tree(tree))], recurse))
                }
            }
        }
        .boxed()
    })
    .map_ok(|entries| stream::iter(entries.into_iter().map(Ok::<_, Error>)))
    .try_flatten()
}
