/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use basename_suffix_skeleton_manifest_v3::RootBssmV3DirectoryId;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use fsnodes::RootFsnodeId;
use futures::Stream;
use futures::StreamExt;
use futures::future::try_join_all;
use futures::stream::TryStreamExt;
use itertools::EitherOrBoth;
use manifest::ManifestOps;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::MPath;
use mononoke_types::NonRootMPath;
use mononoke_types::fsnode::FsnodeFile;
use mononoke_types::inferred_copy_from::InferredCopyFrom;
use mononoke_types::inferred_copy_from::InferredCopyFromEntry;
use vec1::Vec1;

const BASENAME_MATCH_MAX_CANDIDATES: usize = 10_000;

struct CopyFromCandidate {
    cs_id: ChangesetId,
    path: MPath,
    #[allow(unused)]
    fsnode: FsnodeFile,
}

// It's possible to have multiple source files that match,
// pick the one with the smallest path
fn pick_source_from_candidates(candidates: &[CopyFromCandidate]) -> &CopyFromCandidate {
    candidates
        .iter()
        .min_by_key(|c| c.path.clone())
        .unwrap_or_else(|| panic!("There should be at least one candidate"))
}

fn flatten_candidates(
    maps: Vec<HashMap<ContentId, Vec<CopyFromCandidate>>>,
) -> HashMap<ContentId, Vec<CopyFromCandidate>> {
    let mut merged = HashMap::new();
    for map in maps {
        for (content_id, candidates) in map {
            merged
                .entry(content_id)
                .or_insert(vec![])
                .extend(candidates)
        }
    }
    merged
}

async fn get_candidates_from_changeset(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    cs_id: ChangesetId,
    paths: Vec<NonRootMPath>,
) -> Result<HashMap<ContentId, Vec<CopyFromCandidate>>> {
    let mut content_to_candidates = HashMap::new();

    let entries = derivation_ctx
        .fetch_dependency::<RootFsnodeId>(ctx, cs_id)
        .await?
        .fsnode_id()
        .find_entries(ctx.clone(), derivation_ctx.blobstore().clone(), paths)
        .try_collect::<Vec<_>>()
        .await?;

    for (path, entry) in entries {
        if let Some(fsnode) = entry.into_leaf() {
            content_to_candidates
                .entry(fsnode.content_id().clone())
                .or_insert(vec![])
                .push(CopyFromCandidate {
                    cs_id,
                    path,
                    fsnode,
                });
        }
    }
    Ok(content_to_candidates)
}

async fn get_matched_paths_by_basenames_from_changeset(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    cs_id: ChangesetId,
    basenames: Vec<String>,
    path_prefixes: Vec<MPath>,
) -> Result<impl Stream<Item = Result<MPath, Error>>> {
    derivation_ctx
        .fetch_dependency::<RootBssmV3DirectoryId>(ctx, cs_id)
        .await?
        .find_files_filter_basenames(
            ctx,
            derivation_ctx.blobstore().clone(),
            path_prefixes,
            EitherOrBoth::Left(Vec1::try_from_vec(basenames)?),
            None,
        )
        .await
}

// Find exact renames by comparing the content of deleted vs new/changed files
// in the current changeset. If they have the same content, the path pair is
// a rename.
//
// Return a list of inferred renames and the remaining candidates we gathered that
// failed the exact match check. They will be reconsidered for partial content
// matching later.
async fn find_exact_renames(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: &BonsaiChangeset,
) -> Result<(
    Vec<(MPath, InferredCopyFromEntry)>,
    HashMap<ContentId, Vec<CopyFromCandidate>>,
)> {
    let mut content_to_paths = HashMap::new();
    for (path, file_change) in bonsai.simplified_file_changes() {
        if let Some(fc) = file_change {
            content_to_paths
                .entry(fc.content_id())
                .or_insert(vec![])
                .push(path.clone());
        }
    }

    let deleted_paths = bonsai
        .simplified_file_changes()
        .filter_map(|(path, fc)| {
            if fc.is_none() {
                Some(path.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let content_to_candidates_vec = try_join_all(bonsai.parents().map(|parent_cs_id| {
        cloned!(deleted_paths);
        async move {
            get_candidates_from_changeset(ctx, derivation_ctx, parent_cs_id, deleted_paths)
                .await
                .with_context(|| {
                    format!(
                        "Failed to get content for deleted paths from parent {:?}",
                        parent_cs_id
                    )
                })
        }
    }))
    .await?;
    let mut content_to_candidates = flatten_candidates(content_to_candidates_vec);

    let mut renames = vec![];
    for (content_id, paths) in &content_to_paths {
        if let Some(candidates) = content_to_candidates.get(content_id) {
            let from = pick_source_from_candidates(candidates);
            for path in paths {
                renames.push((
                    MPath::from(path.clone()),
                    InferredCopyFromEntry {
                        from_csid: from.cs_id,
                        from_path: from.path.clone(),
                    },
                ));
            }
        }
    }

    // Remove any exact-matched content from the candidate list
    // The remaining will be used for partial matching later
    for content_id in content_to_paths.keys() {
        content_to_candidates.remove(content_id);
    }
    Ok((renames, content_to_candidates))
}

// Infer copies by matching basenames between new/changed files in the
// current changeset and other files in the same repo (with some constraints).
// If the basenames match and the content are the same, the path pair is a copy.
//
// Return a list of inferred copies and the remaining candidates we gathered that
// failed the exact match check. They will be reconsidered for partial content
// matching later.
async fn find_basename_matched_copies(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: &BonsaiChangeset,
    paths_to_ignore: &HashSet<MPath>,
) -> Result<(
    Vec<(MPath, InferredCopyFromEntry)>,
    HashMap<ContentId, Vec<CopyFromCandidate>>,
)> {
    let mut content_to_paths = HashMap::new();
    let mut basenames = HashSet::new();
    let mut path_prefixes = HashSet::new();
    for (path, file_change) in bonsai.simplified_file_changes() {
        if !paths_to_ignore.contains(path.into()) {
            if let Some(fc) = file_change {
                content_to_paths
                    .entry(fc.content_id())
                    .or_insert(vec![])
                    .push(path.clone());

                basenames.insert(path.basename().to_string());
                // Restrict search to any of the touched top-level directory
                if let Some(path_prefix) = path.take_prefix_components(1)? {
                    path_prefixes.insert(MPath::from(path_prefix));
                }
            }
        }
    }
    if basenames.is_empty() {
        return Ok((vec![], HashMap::new()));
    }

    let basenames_vec = basenames.into_iter().collect::<Vec<_>>();
    let path_prefixes_vec = path_prefixes.into_iter().collect::<Vec<_>>();
    let mut content_to_candidates_vec = vec![];

    for parent_cs_id in bonsai.parents() {
        content_to_candidates_vec.push(
            get_matched_paths_by_basenames_from_changeset(
                ctx,
                derivation_ctx,
                parent_cs_id,
                basenames_vec.clone(),
                path_prefixes_vec.clone(),
            )
            .await?
            .try_filter_map(async move |path| Ok(path.into_optional_non_root_path()))
            .take(BASENAME_MATCH_MAX_CANDIDATES)
            .try_chunks(100)
            .try_fold(HashMap::new(), |mut acc, paths| async move {
                let hashmap =
                    get_candidates_from_changeset(ctx, derivation_ctx, parent_cs_id, paths).await;
                if let Ok(hashmap) = hashmap {
                    for (k, v) in hashmap {
                        acc.entry(k).or_insert(vec![]).extend(v);
                    }
                }
                Ok(acc)
            })
            .await?,
        );
    }
    let mut content_to_candidates = flatten_candidates(content_to_candidates_vec);

    let mut copies = vec![];
    for (content_id, paths) in &content_to_paths {
        if let Some(candidates) = content_to_candidates.get(content_id) {
            let from = pick_source_from_candidates(candidates);
            for path in paths {
                copies.push((
                    MPath::from(path.clone()),
                    InferredCopyFromEntry {
                        from_csid: from.cs_id,
                        from_path: from.path.clone(),
                    },
                ));
            }
        }
    }

    // Remove any exact-matched content from the candidate list
    // The remaining will be used for partial matching later
    for content_id in content_to_paths.keys() {
        content_to_candidates.remove(content_id);
    }
    Ok((copies, content_to_candidates))
}

// TODO: add more cases
// Ref: https://github.com/git/git/blob/master/diffcore-rename.c
pub(crate) async fn derive_impl(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: &BonsaiChangeset,
) -> Result<Option<InferredCopyFrom>> {
    let mut resolved_paths = HashSet::new();

    let (exact_renames, _leftover0) = find_exact_renames(ctx, derivation_ctx, bonsai).await?;
    resolved_paths.extend(exact_renames.iter().map(|(path, _)| path.clone()));

    let (basename_matched_copies, _leftover1) =
        find_basename_matched_copies(ctx, derivation_ctx, bonsai, &resolved_paths).await?;
    resolved_paths.extend(basename_matched_copies.iter().map(|(path, _)| path.clone()));

    let entries = [exact_renames, basename_matched_copies].concat();
    if entries.is_empty() {
        Ok(None)
    } else {
        Ok(Some(
            InferredCopyFrom::from_subentries(ctx, derivation_ctx.blobstore(), entries).await?,
        ))
    }
}
