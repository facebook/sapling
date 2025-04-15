/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fmt::Write;
use std::sync::Arc;

use anyhow::Result;
use anyhow::anyhow;
use blame::FetchOutcome;
use blame::fetch_content_for_blame;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use bounded_traversal::Iter;
use bounded_traversal::bounded_traversal_dag;
use clap::Args;
use cloned::cloned;
use context::CoreContext;
use futures::FutureExt;
use manifest::ManifestOps;
use mononoke_app::args::ChangesetArgs;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::FileUnodeId;
use mononoke_types::NonRootMPath;
use mononoke_types::blame_v2::BlameParent;
use mononoke_types::blame_v2::BlameParentId;
use mononoke_types::blame_v2::BlameRejected;
use mononoke_types::blame_v2::BlameV2;
use repo_blobstore::RepoBlobstoreArc;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use unodes::RootUnodeManifestId;

use super::Repo;

#[derive(Args)]
pub(super) struct ComputeArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,

    #[clap(short, long)]
    path: String,

    #[clap(short, long)]
    line_number: bool,
}

pub(super) async fn compute(ctx: &CoreContext, repo: &Arc<Repo>, args: ComputeArgs) -> Result<()> {
    let cs_id = args.changeset_args.resolve_changeset(ctx, repo).await?;
    let path = NonRootMPath::new(&args.path)?;
    let line_number = args.line_number;

    let blobstore = repo.repo_blobstore_arc();
    let file_unode_id = find_leaf(ctx.clone(), repo.clone(), cs_id, path.clone()).await?;
    let (_, _, content, blame) = bounded_traversal_dag(
        256,
        (None, path.clone(), file_unode_id),
        {
            // unfold operator traverses all parents of a given unode, accounting for
            // renames and treating them as another parent.
            |(parent_index, path, file_unode_id)| {
                cloned!(ctx, repo, blobstore);
                async move {
                    let file_unode = file_unode_id.load(&ctx, &blobstore).await?;
                    let csid = *file_unode.linknode();
                    let bonsai = csid.load(&ctx, &blobstore).await?;
                    let mut parents = Vec::new();
                    if bonsai.parents().count() == 1 {
                        // The bonsai changeset only has a single parent, so
                        // we can assume that is where the file came from.
                        for parent_unode_id in file_unode.parents().iter() {
                            parents.push((Some(0), path.clone(), *parent_unode_id));
                        }
                    } else {
                        // We must work out which is the first changeset parent that the
                        // parent unodes came from.
                        let mut parent_indexes = HashMap::new();
                        for (parent_index, parent_csid) in bonsai.parents().enumerate() {
                            if let Some(parent_file_unode_id) =
                                try_find_leaf(ctx.clone(), repo.clone(), parent_csid, path.clone())
                                    .await?
                            {
                                parent_indexes.insert(parent_file_unode_id, parent_index);
                            }
                        }
                        for parent_unode_id in file_unode.parents().iter() {
                            parents.push((
                                parent_indexes.get(parent_unode_id).copied(),
                                path.clone(),
                                *parent_unode_id,
                            ));
                        }
                    }
                    let copy_from = bonsai
                        .file_changes_map()
                        .get(&path)
                        .and_then(|file_change| match file_change {
                            FileChange::Change(tc) => Some(tc.copy_from().clone()?),
                            FileChange::Deletion
                            | FileChange::UntrackedDeletion
                            | FileChange::UntrackedChange(_) => None,
                        });
                    if let Some((r_path, r_csid)) = copy_from {
                        let r_parent_index = bonsai
                            .parents()
                            .position(|csid| csid == *r_csid)
                            .ok_or_else(|| {
                                anyhow!(
                                    "commit {} path {} has copy-from with invalid parent {}",
                                    csid,
                                    path,
                                    r_csid,
                                )
                            })?;
                        let r_unode_id =
                            find_leaf(ctx.clone(), repo, *r_csid, r_path.clone()).await?;
                        parents.push((Some(r_parent_index), r_path.clone(), r_unode_id))
                    };
                    anyhow::Ok(((csid, parent_index, path, file_unode_id), parents))
                }
                .boxed()
            }
        },
        {
            |(csid, parent_index, path, file_unode_id),
             parents: Iter<
                Result<(Option<usize>, NonRootMPath, bytes::Bytes, BlameV2), BlameRejected>,
            >| {
                cloned!(ctx, repo);
                async move {
                    match fetch_content_for_blame(&ctx, &repo, file_unode_id).await? {
                        FetchOutcome::Rejected(rejected) => Ok(Err(rejected)),
                        FetchOutcome::Fetched(content) => {
                            let parents = parents
                                .into_iter()
                                .filter_map(|parent| match parent {
                                    Ok((Some(parent_index), parent_path, content, blame)) => {
                                        Some(BlameParent::new(
                                            BlameParentId::ChangesetParent(parent_index),
                                            parent_path,
                                            content,
                                            blame,
                                        ))
                                    }
                                    _ => None,
                                })
                                .collect();
                            let blame = BlameV2::new(csid, path.clone(), content.clone(), parents)?;
                            Ok(Ok((parent_index, path, content, blame)))
                        }
                    }
                }
                .boxed()
            }
        },
    )
    .await?
    .ok_or_else(|| anyhow!("cycle found"))??;
    let annotate = blame_hg_annotate(ctx.clone(), repo, content, blame, line_number).await?;
    println!("{}", annotate);
    Ok(())
}

/// Finds a leaf that should exist.  Returns an error if the path is not
/// a file in this changeset.
async fn find_leaf(
    ctx: CoreContext,
    repo: Arc<Repo>,
    csid: ChangesetId,
    path: NonRootMPath,
) -> Result<FileUnodeId> {
    let mf_root = repo
        .repo_derived_data()
        .derive::<RootUnodeManifestId>(&ctx, csid)
        .await?;
    let entry_opt = mf_root
        .manifest_unode_id()
        .clone()
        .find_entry(ctx, repo.repo_blobstore().clone(), path.clone().into())
        .await?;
    let entry = entry_opt.ok_or_else(|| anyhow!("No such path: {}", path))?;
    match entry.into_leaf() {
        None => Err(anyhow!("Blame is not available for directories: {}", path)),
        Some(file_unode_id) => Ok(file_unode_id),
    }
}

/// Attempts to find a leaf, but returns `None` if the path is not a file.
async fn try_find_leaf(
    ctx: CoreContext,
    repo: Arc<Repo>,
    csid: ChangesetId,
    path: NonRootMPath,
) -> Result<Option<FileUnodeId>> {
    let mf_root = repo
        .repo_derived_data()
        .derive::<RootUnodeManifestId>(&ctx, csid)
        .await?;
    let entry_opt = mf_root
        .manifest_unode_id()
        .clone()
        .find_entry(ctx, repo.repo_blobstore().clone(), path.clone().into())
        .await?;
    Ok(entry_opt.and_then(|entry| entry.into_leaf()))
}

/// Format blame the same way `hg blame` does
async fn blame_hg_annotate<C: AsRef<[u8]> + 'static + Send>(
    ctx: CoreContext,
    repo: &Arc<Repo>,
    content: C,
    blame: BlameV2,
    show_line_number: bool,
) -> Result<String> {
    if content.as_ref().is_empty() {
        return Ok(String::new());
    }
    let content = String::from_utf8_lossy(content.as_ref());
    let mut result = String::new();
    let csids: Vec<_> = blame.changeset_ids()?.map(|(csid, _)| csid).collect();
    let mapping = repo.get_hg_bonsai_mapping(ctx, csids).await?;
    let mapping: HashMap<_, _> = mapping.into_iter().map(|(k, v)| (v, k)).collect();

    for (line, blame_line) in content.lines().zip(blame.lines()?) {
        let hg_csid = mapping
            .get(blame_line.changeset_id)
            .ok_or_else(|| anyhow!("unresolved bonsai csid: {}", blame_line.changeset_id))?;
        write!(
            result,
            "{:>5} ",
            format!("#{}", blame_line.changeset_index + 1)
        )?;
        result.push_str(&hg_csid.to_string()[..12]);
        result.push(':');
        if show_line_number {
            write!(&mut result, "{:>4}:", blame_line.origin_offset + 1)?;
        }
        result.push(' ');
        result.push_str(line);
        result.push('\n');
    }

    Ok(result)
}
