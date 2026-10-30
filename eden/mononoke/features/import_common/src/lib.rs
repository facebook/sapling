/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::future::Future;
use std::time::Duration;

use anyhow::Result;
use bulk_derivation::BulkDerivation;
use changesets_creation::save_changesets;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriterRef;
use context::CoreContext;
use futures_retry::retry;
use futures_stats::TimedTryFutureExt;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableType;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use tracing::debug;
use tracing::info;

const BASE_RETRY_DELAY: Duration = Duration::from_secs(1);
const RETRY_ATTEMPTS: usize = 4;

#[derive(Clone, Debug)]
pub enum BackfillDerivation {
    AllConfiguredTypes,
    OnlySpecificTypes(Vec<DerivableType>),
    No,
}

impl BackfillDerivation {
    pub fn types(&self, configured_types: &HashSet<DerivableType>) -> Vec<DerivableType> {
        match self {
            BackfillDerivation::AllConfiguredTypes => configured_types.iter().cloned().collect(),
            BackfillDerivation::OnlySpecificTypes(derived_data_types) => derived_data_types
                .iter()
                .filter(|ty| configured_types.contains(ty))
                .cloned()
                .collect(),
            BackfillDerivation::No => Vec::new(),
        }
    }
}

/// Returns the value and the number of attempts it took.
pub async fn retry_with_backoff<F, Fut, V, E>(func: F) -> Result<(V, usize), E>
where
    F: FnMut(usize) -> Fut + Send,
    Fut: Future<Output = Result<V, E>>,
    V: Send + 'static,
    E: Send + 'static,
{
    retry(func, BASE_RETRY_DELAY)
        .binary_exponential_backoff()
        .max_attempts(RETRY_ATTEMPTS)
        .inspect_err(|attempt, _err| info!("attempt {attempt} of {RETRY_ATTEMPTS} failed"))
        .await
}

/// The batch must be topologically sorted, with every parent either already
/// saved or earlier in the batch: it is saved as-is, without ordering checks.
pub async fn save_and_derive_batch(
    ctx: &CoreContext,
    repo: &(
         impl CommitGraphRef
         + CommitGraphWriterRef
         + RepoBlobstoreRef
         + RepoIdentityRef
         + RepoDerivedDataRef
         + Send
         + Sync
     ),
    changesets: Vec<BonsaiChangeset>,
    derived_data_types: &[DerivableType],
) -> Result<Vec<ChangesetId>> {
    let csids = changesets
        .iter()
        .map(|bcs| bcs.get_changeset_id())
        .collect::<Vec<_>>();

    let ((stats, ()), num_attempts) =
        retry_with_backoff(|_| save_changesets(ctx, repo, changesets.clone()).try_timed()).await?;
    debug!(
        "save_changesets for {} commits in {:?} after {} attempts",
        csids.len(),
        stats.completion_time,
        num_attempts
    );

    derive_batch(ctx, repo, &csids, derived_data_types).await?;
    Ok(csids)
}

/// First-time derivation is idempotent, so retrying after a transient error
/// (e.g. blobstore/MySQL read timeouts) only re-derives what didn't complete.
pub async fn derive_batch(
    ctx: &CoreContext,
    repo: &(impl RepoDerivedDataRef + Send + Sync),
    csids: &[ChangesetId],
    derived_data_types: &[DerivableType],
) -> Result<()> {
    let batch_size = csids.len() as u64;
    retry_with_backoff(|_| {
        repo.repo_derived_data().manager().derive_bulk_locally(
            ctx,
            csids,
            None,
            derived_data_types,
            Some(batch_size),
            None,
        )
    })
    .await?;
    Ok(())
}
