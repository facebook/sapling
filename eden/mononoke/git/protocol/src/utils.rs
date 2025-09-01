/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use blobstore::Loadable;
use bonsai_tag_mapping::BonsaiTagMappingEntry;
use cloned::cloned;
use commit_graph::AncestorsStreamBuilder;
use commit_graph_types::frontier::AncestorsWithinDistance;
use context::CoreContext;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use futures::stream::BoxStream;
use git_types::DeltaObjectKind;
use git_types::GitDeltaManifestEntryOps;
use git_types::ObjectDeltaOps;
use gix_hash::ObjectId;
use gix_object::bstr::ByteSlice;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use mononoke_types::hash::GitSha1;
use repo_blobstore::ArcRepoBlobstore;
use rustc_hash::FxHashSet;

use crate::HEADS_PREFIX;
use crate::REF_PREFIX;
use crate::Repo;
use crate::TAGS_PREFIX;
use crate::bookmarks_provider::bookmarks;
use crate::store::fetch_nested_tags;
use crate::types::ChainBreakingMode;
use crate::types::DeltaInclusion;
use crate::types::FetchFilter;
use crate::types::RefTarget;
use crate::types::RefsSource;
use crate::types::RequestedRefs;
use crate::types::ShallowInfoResponse;
use crate::types::SymrefFormat;

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
    chain_breaking_mode: ChainBreakingMode,
) -> Option<&(dyn ObjectDeltaOps + Sync)> {
    if let ChainBreakingMode::Stochastic = chain_breaking_mode {
        // Periodically break delta chains since resolving very long delta chains
        // is expensive on client side
        let byte_sum = entry.full_object_oid().first_byte() as u16
            + entry.full_object_oid().as_bytes().last_byte().unwrap_or(0) as u16;
        if byte_sum % 250 == 0 {
            return None;
        }
    }
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
                let kind = delta.base_object_kind();
                let size = delta.base_object_size();
                // Is the delta defined in terms of itself (i.e. A as delta of A)? If yes, then we
                // should use the full object to avoid cycle
                let is_self_delta = delta.base_object_oid() == entry.full_object_oid();
                // Only use the delta if it is below the threshold and passes the filter
                delta_below_threshold(*delta, entry.full_object_size(), inclusion_threshold)
                    && filter_object(filter, entry.path(), kind, size)
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
    chain_breaking_mode: ChainBreakingMode,
) -> usize {
    let delta = delta_base(entry, delta_inclusion, filter, chain_breaking_mode);
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

pub(crate) fn to_git_object_stream(
    git_objects: Vec<ObjectId>,
) -> BoxStream<'static, Result<ObjectId>> {
    stream::iter(git_objects.into_iter().map(Ok)).boxed()
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
        Some(shallow_info) => Ok(shallow_info
            .packfile_commits
            .commits
            .iter()
            .map(|entry| entry.csid())
            .collect()),
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
        .map_ok(async |entry| {
            let tag_hash = entry.tag_hash.to_object_id()?;
            // If the target is tag, make sure to fetch all the nested tags
            if entry.target_is_tag {
                fetch_nested_tags(&ctx, &blobstore, tag_hash.clone())
                    .await
                    .with_context(|| format!("Error in fetching nested tags for entry {:?}", entry))
            } else {
                Ok(vec![tag_hash])
            }
        })
        .try_buffer_unordered(tag_concurrency)
        .try_fold(
            FxHashSet::default(), // Dedupe based on tag hashes so we don't double count
            async |mut output_hashes, tag_hashes| {
                for tag_hash in tag_hashes.into_iter() {
                    let tag_sha = GitSha1::from_object_id(tag_hash.as_ref())?;
                    output_hashes.insert(tag_sha);
                }
                anyhow::Ok(output_hashes)
            },
        )
        .await
}

/// Function responsible for fetching the ancestors of the input heads that have creation time greater than the input
/// time
pub(crate) async fn ancestors_after_time(
    ctx: &CoreContext,
    repo: &impl Repo,
    heads: Vec<ChangesetId>,
    time: usize,
) -> Result<AncestorsWithinDistance> {
    let commit_graph = Arc::new(repo.commit_graph().clone());
    let blobstore = repo.repo_blobstore().clone();
    let inner_ctx = ctx.clone();
    let ancestors = AncestorsStreamBuilder::new(commit_graph, ctx.clone(), heads.clone())
        .with(move |csid| {
            cloned!(inner_ctx as ctx, time, blobstore);
            async move {
                let changeset = csid.load(&ctx, &blobstore).await?;
                let to_include = changeset
                    .committer_date()
                    .is_some_and(|date| date.timestamp_secs() > time as i64);
                Ok(to_include)
            }
        })
        .build()
        .await?
        .try_collect::<Vec<_>>()
        .await?;
    let ancestors_with_boundaries = ancestors_with_boundaries(ctx, repo, ancestors).await?;
    if ancestors_with_boundaries.is_empty() {
        return Err(anyhow::anyhow!(
            "No commits selected for shallow requests with committer time greater than {}",
            time
        ));
    }
    Ok(ancestors_with_boundaries)
}

/// Function responsible for fetching the ancestors of the input heads that are not also the
/// ancestors of input excluded_heads
pub(crate) async fn ancestors_excluding(
    ctx: &CoreContext,
    repo: &impl Repo,
    heads: Vec<ChangesetId>,
    excluded_refs: Vec<String>,
) -> Result<AncestorsWithinDistance> {
    // Sanitize the vec of excluded refs
    let excluded_refs = excluded_refs
        .into_iter()
        .map(|head| match head.strip_prefix(REF_PREFIX) {
            Some(stripped) => stripped.to_string(),
            None => head,
        })
        .collect::<HashSet<_>>();
    if excluded_refs.iter().any(|excluded_ref| {
        !excluded_ref.starts_with(TAGS_PREFIX) && !excluded_ref.starts_with(HEADS_PREFIX)
    }) {
        anyhow::bail!(
            "Refs for `shallow-exclude` should be provided with tags/ or heads/ prefix as appropriate"
        )
    }
    // Convert the refs into changesets to be used with commit graph
    let excluded_heads = bookmarks(
        ctx,
        repo,
        &RequestedRefs::Included(excluded_refs),
        RefsSource::WarmBookmarksCache,
    )
    .await?
    .entries
    .values()
    .cloned()
    .collect::<Vec<_>>();

    // Find the ancestors that need to be returned
    let ancestors = repo
        .commit_graph()
        .ancestors_difference(ctx, heads, excluded_heads)
        .await
        .context("Error in getting stream of commits between heads and bases during fetch")?;
    let ancestors_with_boundaries = ancestors_with_boundaries(ctx, repo, ancestors).await?;
    if ancestors_with_boundaries.is_empty() {
        return Err(anyhow::anyhow!(
            "No commits selected for shallow requests with shallow-exclude"
        ));
    }
    Ok(ancestors_with_boundaries)
}

async fn ancestors_with_boundaries(
    ctx: &CoreContext,
    repo: &impl Repo,
    ancestors: Vec<ChangesetId>,
) -> Result<AncestorsWithinDistance> {
    // From the list of ancestors, get the boundary of commits that Git will mark as shallow for the client
    let boundaries = repo
        .commit_graph()
        .find_boundary(ctx, ancestors.clone())
        .await?;
    // Ancestor commits cannot include boundary commits, so filter them out
    let ancestors = ancestors
        .into_iter()
        .filter(|csid| !boundaries.contains(csid))
        .collect::<Vec<_>>();
    Ok(AncestorsWithinDistance {
        ancestors,
        boundaries,
    })
}
