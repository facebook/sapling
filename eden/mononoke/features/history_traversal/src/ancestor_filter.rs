/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Generic ancestor filtering with time window support and user-provided predicate.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Result;
use blobstore::Loadable;
use cloned::cloned;
use commit_graph::AncestorsStreamBuilder;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;

/// Narrower repo trait for ancestor filtering — does not require MutableRenamesRef.
pub trait AncestorFilterRepo:
    RepoBlobstoreRef + CommitGraphRef + RepoDerivedDataRef + Clone + Send + Sync + 'static
{
}

impl<T> AncestorFilterRepo for T where
    T: RepoBlobstoreRef + CommitGraphRef + RepoDerivedDataRef + Clone + Send + Sync + 'static
{
}

/// Options controlling the ancestor stream traversal bounds.
pub struct AncestorFilterOptions {
    /// Stop traversal when author_date timestamp is before this value (unix timestamp).
    /// This is a monotonic property: if a commit is too old, its ancestors are too.
    pub until_timestamp: Option<i64>,
    /// Only include descendants of this commit.
    pub descendants_of: Option<ChangesetId>,
    /// Exclude this changeset and all its ancestors from the stream.
    pub exclude_changeset_and_ancestors: Option<ChangesetId>,
}

/// Returns a stream of ancestor `ChangesetId`s of `head` that match the given
/// `predicate`, bounded by the options in `opts`.
///
/// The predicate receives a loaded `BonsaiChangeset` and returns whether the
/// changeset should be included in the output stream. All ancestors are still
/// traversed — the predicate only controls inclusion, not pruning.
///
/// When `until_timestamp` is set, the traversal is pruned via
/// `AncestorsStreamBuilder::with()` so entire subtrees older than the
/// threshold are skipped. The predicate result is cached during the `with()`
/// callback so the downstream filter can reuse it without a redundant
/// blobstore load.
pub async fn matching_ancestors_stream(
    ctx: &CoreContext,
    repo: &impl AncestorFilterRepo,
    head: ChangesetId,
    opts: AncestorFilterOptions,
    predicate: Arc<dyn Fn(&BonsaiChangeset) -> bool + Send + Sync>,
) -> Result<BoxStream<'static, Result<ChangesetId>>> {
    let mut builder = AncestorsStreamBuilder::new(
        Arc::new(repo.commit_graph().parents_graph()),
        ctx.clone(),
        vec![head],
    );

    if let Some(descendants_of) = opts.descendants_of {
        builder = builder.descendants_of(descendants_of);
    }

    if let Some(exclude) = opts.exclude_changeset_and_ancestors {
        builder = builder.exclude_ancestors_of(vec![exclude]);
    }

    // When a timestamp filter is set, use `with()` for traversal pruning
    // and evaluate the user predicate in the same load. Results are cached
    // in a shared map so the downstream `try_filter_map` can look them up
    // without reloading.
    if let Some(until_timestamp) = opts.until_timestamp {
        let predicate_cache: Arc<Mutex<HashMap<ChangesetId, bool>>> =
            Arc::new(Mutex::new(HashMap::new()));

        builder = builder.with({
            cloned!(ctx, repo, predicate, predicate_cache);
            move |cs_id| {
                cloned!(ctx, repo, predicate, predicate_cache);
                async move {
                    let bonsai = cs_id.load(&ctx, repo.repo_blobstore()).await?;
                    let in_window = bonsai.author_date().as_chrono().timestamp() >= until_timestamp;
                    if in_window {
                        predicate_cache
                            .lock()
                            .map_err(|e| anyhow::anyhow!("predicate_cache lock poisoned: {}", e))?
                            .insert(cs_id, predicate(&bonsai));
                    }
                    Ok(in_window)
                }
            }
        });

        let cs_ids_stream = builder.build().await?;

        let stream = cs_ids_stream
            .try_filter_map(move |cs_id| {
                let predicate_cache = predicate_cache.clone();
                async move {
                    let matches = predicate_cache
                        .lock()
                        .map_err(|e| anyhow::anyhow!("predicate_cache lock poisoned: {}", e))?
                        .remove(&cs_id)
                        .unwrap_or(false);
                    Ok(if matches { Some(cs_id) } else { None })
                }
            })
            .boxed();

        return Ok(stream);
    }

    // No timestamp filter — just apply the predicate by loading each changeset.
    let cs_ids_stream = builder.build().await?;

    let ctx = ctx.clone();
    let repo = repo.clone();
    let stream = cs_ids_stream
        .try_filter_map(move |cs_id| {
            cloned!(ctx, repo, predicate);
            async move {
                let bonsai = cs_id.load(&ctx, repo.repo_blobstore()).await?;
                if predicate(&bonsai) {
                    Ok(Some(cs_id))
                } else {
                    Ok(None)
                }
            }
        })
        .boxed();

    Ok(stream)
}
