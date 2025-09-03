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
use blobstore::Loadable;
use blobstore::LoadableError;
use blobstore::Storable;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use fsnodes::RootFsnodeId;
use futures::Stream;
use futures::StreamExt;
use futures::future::try_join_all;
use futures::stream;
use futures::stream::TryStreamExt;
use itertools::EitherOrBoth;
use manifest::ManifestOps;
use mononoke_types::BasicFileChange;
use mononoke_types::BlobstoreValue;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::FileContents;
use mononoke_types::FileType;
use mononoke_types::MPath;
use mononoke_types::NonRootMPath;
use mononoke_types::fsnode::FsnodeFile;
use mononoke_types::inferred_copy_from::InferredCopyFrom;
use mononoke_types::inferred_copy_from::InferredCopyFromEntry;
use vec1::Vec1;

use crate::RootInferredCopyFromId;
use crate::similarity::estimate_similarity;

const BASENAME_MATCH_MAX_CANDIDATES: usize = 10_000;
// This roughly follows our file content chunk size
// Ref: https://fburl.com/code/qudh1g07
const PARTIAL_MATCH_MAX_FILE_SIZE: u64 = 4 * 1024 * 1024; // 4MB
// Ref: https://fburl.com/lkfjeka4
const CONTENT_SIMILARITY_RATIO_THRESHOLD: f64 = 0.5;

#[derive(Clone, Debug)]
struct CopyFromCandidate {
    cs_id: ChangesetId,
    path: MPath,
    fsnode: FsnodeFile,
}

// It's possible to have multiple source files that match,
// pick the one with the smallest path
fn pick_source_from_candidates<'a>(
    candidates: impl Iterator<Item = &'a CopyFromCandidate>,
) -> &'a CopyFromCandidate {
    candidates
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

fn should_skip_file_type(file_type: &FileType) -> bool {
    matches!(file_type, FileType::GitSubmodule | FileType::Symlink)
}

// Basic file metadata check to filter out pairs that are unlikely to match.
fn filter_by_metadata(
    dst_file_change: &BasicFileChange,
    src_candidate: &CopyFromCandidate,
) -> bool {
    // Skip LFS files
    // Note that we are only checking dest file as we don't have this
    // info for the source candidate (fsnode doesn't have it)
    if dst_file_change.git_lfs().is_lfs_pointer() {
        return false;
    }

    // Skip submodules or symlinks
    if should_skip_file_type(&dst_file_change.file_type())
        || should_skip_file_type(src_candidate.fsnode.file_type())
    {
        return false;
    }

    let dst_file_size = dst_file_change.size();
    let candidate_file_size = src_candidate.fsnode.size();
    let max_size = dst_file_size.max(candidate_file_size);
    let min_size = dst_file_size.min(candidate_file_size);
    // Skip if files are too large or too different in sizes
    max_size <= PARTIAL_MATCH_MAX_FILE_SIZE
        && (max_size - min_size) as f64 / (max_size as f64) < CONTENT_SIMILARITY_RATIO_THRESHOLD
}

async fn load_file_content(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    content_id: &ContentId,
) -> Result<FileContents, LoadableError> {
    content_id.load(ctx, derivation_ctx.blobstore()).await
}

async fn find_best_candidate_by_partial_content_match(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    dst_content_id: ContentId,
    src_content_to_candidates: &HashMap<ContentId, Vec<&CopyFromCandidate>>,
) -> Result<Option<CopyFromCandidate>> {
    let dst_bytes = match load_file_content(ctx, derivation_ctx, &dst_content_id).await {
        Ok(FileContents::Bytes(v)) => v,
        // TODO(lyang): Evaluate whether we need to compare bigger files
        _ => return Ok::<_, Error>(None),
    };
    let similarities = stream::iter(src_content_to_candidates.keys())
        .map(|src_content_id| {
            cloned!(dst_bytes);
            async move {
                let src_bytes = match load_file_content(ctx, derivation_ctx, src_content_id).await {
                    Ok(FileContents::Bytes(v)) => v,
                    _ => return Ok::<_, Error>(None),
                };

                let similarity = tokio::task::spawn_blocking({
                    move || estimate_similarity(&dst_bytes, &src_bytes)
                })
                .await?;
                Ok(similarity
                    .ok()
                    .filter(|&s| s >= CONTENT_SIMILARITY_RATIO_THRESHOLD)
                    .map(|s| (src_content_id, s)))
            }
        })
        .boxed()
        .buffer_unordered(20)
        .try_collect::<Vec<_>>()
        .await?;

    let mut best_matched: Option<(f64, Vec<&CopyFromCandidate>)> = None;
    for entry in similarities.into_iter().flatten() {
        match best_matched {
            Some((best_similarity, _)) if best_similarity > entry.1 => {
                // Do nothing, best is still the best
            }
            Some((best_similarity, mut candidates)) if best_similarity == entry.1 => {
                // On tie, merge the candidates so we can consider them together later
                candidates.extend(src_content_to_candidates[entry.0].clone());
                best_matched = Some((best_similarity, candidates));
            }
            _ => best_matched = Some((entry.1, src_content_to_candidates[entry.0].clone())),
        }
    }

    Ok(best_matched
        .map(|(_, candidates)| pick_source_from_candidates(candidates.into_iter()).clone()))
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
            let from = pick_source_from_candidates(candidates.iter());
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
    let dir_lookup_level = match derivation_ctx.config().inferred_copy_from_config {
        Some(config) => config.dir_level_for_basename_lookup,
        None => 1,
    };

    for (path, file_change) in bonsai.simplified_file_changes() {
        if !paths_to_ignore.contains(path.into()) {
            if let Some(fc) = file_change {
                content_to_paths
                    .entry(fc.content_id())
                    .or_insert(vec![])
                    .push(path.clone());

                basenames.insert(path.basename().to_string());
                // Restrict search to any of the touched N-level directory
                if let Some(path_prefix) = path.take_prefix_components(dir_lookup_level)? {
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
            let from = pick_source_from_candidates(candidates.iter());
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

// Infer copy/renames by comparing file contents.
// Candidates are the files collected during the previous exact matching attempts.
async fn find_partial_matches(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: &BonsaiChangeset,
    paths_to_ignore: &HashSet<MPath>,
    content_to_candidates: &HashMap<ContentId, Vec<CopyFromCandidate>>,
) -> Result<Vec<(MPath, InferredCopyFromEntry)>> {
    let mut content_to_paths = HashMap::new();
    let mut content_to_metadata = HashMap::new();
    for (path, file_change) in bonsai.simplified_file_changes() {
        if !paths_to_ignore.contains(path.into()) {
            if let Some(fc) = file_change {
                content_to_paths
                    .entry(fc.content_id())
                    .or_insert(vec![])
                    .push(path.clone());
                content_to_metadata.entry(fc.content_id()).or_insert(fc);
            }
        }
    }

    let mut matched = vec![];
    for (dst_content_id, fc) in content_to_metadata {
        let dst_paths = content_to_paths.get(&dst_content_id);
        // Trim candidate list by comparing file metadata (e.g. type, size)
        let filtered_content_to_candidates = content_to_candidates
            .iter()
            .filter_map(|(src_content_id, candidates)| {
                let filtered = candidates
                    .iter()
                    .filter(|candidate| {
                        // Make sure we don't include the dest path itself as candidate
                        let candidate_non_root_path =
                            candidate.path.clone().into_optional_non_root_path();
                        if let (Some(dst_paths), Some(candidate_path)) =
                            (dst_paths, candidate_non_root_path)
                        {
                            if dst_paths.contains(&candidate_path) {
                                // Skip if the candidate path is the same as the dest
                                return false;
                            }
                        }
                        // Filter out candidates whose metadata are too different from dest
                        filter_by_metadata(fc, candidate)
                    })
                    .collect::<Vec<_>>();
                if filtered.is_empty() {
                    None
                } else {
                    Some((src_content_id.clone(), filtered))
                }
            })
            .collect::<HashMap<_, _>>();

        if filtered_content_to_candidates.is_empty() {
            continue;
        }

        let candidate = find_best_candidate_by_partial_content_match(
            ctx,
            derivation_ctx,
            dst_content_id,
            &filtered_content_to_candidates,
        )
        .await?;
        if let Some(from) = candidate {
            for path in &content_to_paths[&dst_content_id] {
                matched.push((
                    MPath::from(path.clone()),
                    InferredCopyFromEntry {
                        from_csid: from.cs_id,
                        from_path: from.path.clone(),
                    },
                ));
            }
        }
    }
    Ok(matched)
}

// TODO: add more cases
// Ref: https://github.com/git/git/blob/master/diffcore-rename.c
pub(crate) async fn derive_impl(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: &BonsaiChangeset,
) -> Result<RootInferredCopyFromId> {
    let mut resolved_paths = HashSet::new();

    let (exact_renames, leftover0) = find_exact_renames(ctx, derivation_ctx, bonsai).await?;
    resolved_paths.extend(exact_renames.iter().map(|(path, _)| path.clone()));

    let (basename_matched_copies, leftover1) =
        find_basename_matched_copies(ctx, derivation_ctx, bonsai, &resolved_paths).await?;
    resolved_paths.extend(basename_matched_copies.iter().map(|(path, _)| path.clone()));

    let partial_match_candidates = flatten_candidates(vec![leftover0, leftover1]);
    let partially_matched = find_partial_matches(
        ctx,
        derivation_ctx,
        bonsai,
        &resolved_paths,
        &partial_match_candidates,
    )
    .await?;

    let entries = [exact_renames, basename_matched_copies, partially_matched].concat();
    if entries.is_empty() {
        let empty = InferredCopyFrom::empty();
        Ok(RootInferredCopyFromId(
            empty
                .into_blob()
                .store(ctx, derivation_ctx.blobstore())
                .await
                .context("Failed to store empty InferredCopyFrom blob")?,
        ))
    } else {
        let icf =
            InferredCopyFrom::from_subentries(ctx, derivation_ctx.blobstore(), entries).await?;
        let blob = icf.into_blob();
        Ok(RootInferredCopyFromId(
            blob.store(ctx, derivation_ctx.blobstore())
                .await
                .context("Failed to store InferredCopyFrom blob")?,
        ))
    }
}
