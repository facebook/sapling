/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use bonsai_tag_mapping::BonsaiTagMappingEntry;
use cloned::cloned;
use context::CoreContext;
use futures::stream;
use futures::stream::BoxStream;
use futures::StreamExt;
use futures::TryStreamExt;
use git_types::DeltaObjectKind;
use git_types::GitDeltaManifestEntryOps;
use git_types::ObjectDeltaOps;
use gix_hash::ObjectId;
use mononoke_types::hash::GitSha1;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use repo_blobstore::ArcRepoBlobstore;
use rustc_hash::FxHashSet;

use crate::store::fetch_nested_tags;
use crate::types::DeltaInclusion;
use crate::types::FetchFilter;
use crate::types::RefTarget;
use crate::types::ShallowInfoResponse;
use crate::types::SymrefFormat;
use crate::Repo;

/// Function determining if the current object entry at the given path should be
/// filtered in the resultant packfile
pub(crate) fn filter_object(
    filter: Arc<Option<FetchFilter>>,
    path: &MPath,
    kind: DeltaObjectKind,
    size: u64,
) -> bool {
    match filter.as_ref() {
        Some(filter) => {
            let too_deep =
                (kind.is_tree() || kind.is_blob()) && path.depth() >= filter.max_tree_depth;
            let too_large = kind.is_blob() && size >= filter.max_blob_size;
            let invalid_type = !filter.allowed_object_types.contains(&kind.to_gix_kind());
            // The object passes the filter if its not too deep and not too large and its type is allowed
            !too_deep && !too_large && !invalid_type
        }
        // If there is no filter, then we should not exclude any objects
        None => true,
    }
}

/// Generate the appropriate RefTarget for symref based on the symref format
pub(crate) fn symref_target(
    symref_target: &str,
    commit_id: ObjectId,
    symref_format: SymrefFormat,
) -> RefTarget {
    match symref_format {
        SymrefFormat::NameWithTarget => {
            let metadata = format!("symref-target:{}", symref_target);
            RefTarget::WithMetadata(commit_id, metadata)
        }
        SymrefFormat::NameOnly => RefTarget::Plain(commit_id),
    }
}

/// Function for determining if the delta is below the expected threshold
pub(crate) fn delta_below_threshold(
    delta: &dyn ObjectDeltaOps,
    full_object_size: u64,
    inclusion_threshold: f32,
) -> bool {
    (delta.instructions_compressed_size() as f64)
        < (full_object_size as f64) * inclusion_threshold as f64
}

/// Function for determining the base of the delta based on the input
/// parameters to the function
pub(crate) fn delta_base(
    entry: &(dyn GitDeltaManifestEntryOps + Send),
    delta_inclusion: DeltaInclusion,
    filter: Arc<Option<FetchFilter>>,
) -> Option<&(dyn ObjectDeltaOps + Sync)> {
    match delta_inclusion {
        DeltaInclusion::Include {
            inclusion_threshold,
            ..
        } => entry
            .deltas()
            .min_by(|a, b| {
                a.instructions_compressed_size()
                    .cmp(&b.instructions_compressed_size())
            })
            .filter(|delta| {
                let path = delta.base_object_path();
                let kind = delta.base_object_kind();
                let size = delta.base_object_size();
                // Is the delta defined in terms of itself (i.e. A as delta of A)? If yes, then we
                // should use the full object to avoid cycle
                let is_self_delta = delta.base_object_oid() == entry.full_object_oid();
                // Only use the delta if it is below the threshold and passes the filter
                delta_below_threshold(*delta, entry.full_object_size(), inclusion_threshold)
                    && filter_object(filter, path, kind, size)
                    && !is_self_delta
            }),
        // Can't use the delta variant if the request prevents us from using it
        DeltaInclusion::Exclude => None,
    }
}

/// The stream-relevant weight of the DeltaManifest entry. Useful for determining
/// how many concurrent entries in the stream should be polled
pub(crate) fn entry_weight(
    entry: &(dyn GitDeltaManifestEntryOps + Send),
    delta_inclusion: DeltaInclusion,
    filter: Arc<Option<FetchFilter>>,
) -> usize {
    let delta = delta_base(entry, delta_inclusion, filter);
    let weight = delta.as_ref().map_or(entry.full_object_size(), |delta| {
        delta.instructions_compressed_size()
    });
    weight as usize
}

/// Utility function for converting a vec of commits into a stream of commits
pub(crate) fn to_commit_stream(
    commits: Vec<ChangesetId>,
) -> BoxStream<'static, Result<ChangesetId>> {
    stream::iter(commits.into_iter().map(Ok)).boxed()
}

/// Function responsible for fetching the vec of commits between heads and bases. If the fetch
/// request is shallow, return the shallow commits instead
pub(crate) async fn commits(
    ctx: &CoreContext,
    repo: &impl Repo,
    heads: Vec<ChangesetId>,
    bases: Vec<ChangesetId>,
    shallow_info: &Option<ShallowInfoResponse>,
) -> Result<Vec<ChangesetId>> {
    match shallow_info {
        Some(shallow_info) => Ok(shallow_info.commits.clone()),
        None => {
            repo.commit_graph()
                .ancestors_difference_stream(ctx, heads, bases)
                .await
                .context("Error in getting stream of commits between heads and bases during fetch")?
                .try_collect::<Vec<_>>()
                .await
        }
    }
}

/// Function responsible for converting the input vec of BonsaiTagMappingEntries
/// into unique set of tag hashes while also extending the output with nested tags
/// if applicable
pub(crate) async fn tag_entries_to_hashes(
    tag_entries: Vec<BonsaiTagMappingEntry>,
    ctx: Arc<CoreContext>,
    blobstore: ArcRepoBlobstore,
    tag_concurrency: usize,
) -> Result<FxHashSet<GitSha1>> {
    stream::iter(tag_entries)
        .map(Ok)
        .map_ok(|entry| {
            cloned!(ctx, blobstore);
            async move {
                let tag_hash = entry.tag_hash.to_object_id()?;
                // If the target is tag, make sure to fetch all the nested tags
                if entry.target_is_tag {
                    fetch_nested_tags(&ctx, &blobstore, tag_hash.clone())
                        .await
                        .with_context(|| {
                            format!("Error in fetching nested tags for entry {:?}", entry)
                        })
                } else {
                    Ok(vec![tag_hash])
                }
            }
        })
        .try_buffer_unordered(tag_concurrency)
        .try_fold(
            FxHashSet::default(), // Dedupe based on tag hashes so we don't double count
            |mut output_hashes, tag_hashes| async move {
                for tag_hash in tag_hashes.into_iter() {
                    let tag_sha = GitSha1::from_object_id(tag_hash.as_ref())?;
                    output_hashes.insert(tag_sha);
                }
                anyhow::Ok(output_hashes)
            },
        )
        .await
}
