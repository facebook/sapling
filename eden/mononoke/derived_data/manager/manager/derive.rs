/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::future;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_recursion::async_recursion;
use blobstore::Loadable;
use borrowed::borrowed;
use cloned::cloned;
use context::CoreContext;
use derived_data_service_if::DerivationType;
use derived_data_service_if::DeriveRequest;
use derived_data_service_if::DeriveResponse;
use derived_data_service_if::DeriveUnderived;
use derived_data_service_if::DerivedDataType;
use derived_data_service_if::RequestStatus;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::future::Shared;
use futures::future::TryFutureExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::Future;
use futures_stats::TimedFutureExt;
use futures_stats::TimedTryFutureExt;
use lock_ext::LockExt;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableType;
use scuba_ext::FutureStatsScubaExt;
use slog::debug;

use super::DerivationAssignment;
use super::DerivedDataManager;
use crate::context::DerivationContext;
use crate::derivable::BonsaiDerivable;
use crate::derivable::DerivationDependencies;
use crate::error::DerivationError;
use crate::error::SharedDerivationError;

/// Trait to allow determination of rederivation.
pub trait Rederivation: Send + Sync + 'static {
    /// Determine whether a changeset needs rederivation of
    /// a particular derived data type.
    ///
    /// If this function returns `None`, then it will only be
    /// derived if it isn't already derived.
    fn needs_rederive(&self, derivable_type: DerivableType, csid: ChangesetId) -> Option<bool>;

    /// Marks a changeset as having been derived.  After this
    /// is called, `needs_rederive` should not return `true` for
    /// this changeset.
    fn mark_derived(&self, derivable_type: DerivableType, csid: ChangesetId);
}

impl Rederivation for Mutex<HashSet<ChangesetId>> {
    fn needs_rederive(&self, _derivable_type: DerivableType, csid: ChangesetId) -> Option<bool> {
        if self.with(|rederive| rederive.contains(&csid)) {
            Some(true)
        } else {
            None
        }
    }

    fn mark_derived(&self, _derivable_type: DerivableType, csid: ChangesetId) {
        self.with(|rederive| rederive.remove(&csid));
    }
}

pub type VisitedDerivableTypesMapStatic<OkType, ErrType> =
    Arc<Mutex<HashMap<DerivableType, Shared<BoxFuture<'static, Result<OkType, ErrType>>>>>>;

pub type VisitedDerivableTypesMap<'a, OkType, ErrType> =
    Arc<Mutex<HashMap<DerivableType, Shared<BoxFuture<'a, Result<OkType, ErrType>>>>>>;

impl DerivedDataManager {
    #[async_recursion]
    /// Returns the appropriate manager to derive given changeset, either this
    /// manager, or some secondary manager in the chain.
    async fn get_manager(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> anyhow::Result<&DerivedDataManager> {
        Ok(if let Some(secondary) = &self.inner.secondary {
            if secondary
                .assigner
                .assign(ctx, vec![cs_id])
                .await?
                .secondary
                .is_empty()
            {
                self
            } else {
                secondary.manager.get_manager(ctx, cs_id).await?
            }
        } else {
            self
        })
    }

    pub fn derivation_context(
        &self,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> DerivationContext {
        self.inner
            .derivation_context
            .with_replaced_rederivation(rederivation)
    }

    pub async fn check_derived<Derivable>(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
    ) -> Result<(), DerivationError>
    where
        Derivable: BonsaiDerivable,
    {
        if self
            .fetch_derived::<Derivable>(ctx, csid, None)
            .await?
            .is_none()
        {
            return Err(
                anyhow!("expected {} already derived for {}", Derivable::NAME, csid).into(),
            );
        }
        Ok(())
    }

    /// Find which ancestors of `heads` are not yet derived, and necessary for
    /// the derivation of `heads` to complete, and derive them.
    /// The derivation will be batched. Unless otherwise configured here with `override_batch_size`, the
    /// batch_size will be read from the configuration for this derived data type.
    /// Dependent types will be derived ahead of time.
    /// Return how many changesets were actually derived to derive the heads.
    pub async fn derive_heads<Derivable>(
        &self,
        ctx: CoreContext,
        heads: Vec<ChangesetId>,
        override_batch_size: Option<u64>,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<u64, SharedDerivationError>
    where
        Derivable: BonsaiDerivable,
    {
        self.clone()
            .derive_heads_with_visited::<Derivable>(
                ctx,
                heads,
                override_batch_size,
                rederivation,
                Default::default(),
            )
            .await
    }

    pub fn derive_heads_with_visited<Derivable>(
        self,
        ctx: CoreContext,
        heads: Vec<ChangesetId>,
        override_batch_size: Option<u64>,
        rederivation: Option<Arc<dyn Rederivation>>,
        visited: VisitedDerivableTypesMapStatic<u64, SharedDerivationError>,
    ) -> impl Future<Output = Result<u64, SharedDerivationError>>
    where
        Derivable: BonsaiDerivable,
    {
        let derivation_future = {
            cloned!(visited, ctx, heads);
            async move {
                Derivable::Dependencies::derive_heads(
                    self.clone(),
                    ctx.clone(),
                    heads.clone(),
                    override_batch_size,
                    rederivation.clone(),
                    visited,
                )
                .await
                .context("failed to derive dependent types")?;

                let derivation_ctx = self.derivation_context(rederivation);

                let last_derived = self
                    .commit_graph()
                    .ancestors_frontier_with(&ctx, heads.clone(), |csid| {
                        borrowed!(ctx, derivation_ctx);
                        async move {
                            Ok(derivation_ctx
                                .fetch_derived::<Derivable>(ctx, csid)
                                .await?
                                .is_some())
                        }
                    })
                    .await
                    .map_err(Into::<DerivationError>::into)?;
                let batch_size =
                    override_batch_size.unwrap_or(derivation_ctx.batch_size::<Derivable>());
                let count = self
                    .commit_graph()
                    .ancestors_difference_segments(&ctx, heads.to_vec(), last_derived.clone())
                    .await?
                    .into_iter()
                    .map(|segment| segment.length)
                    .sum();
                let rederivation = derivation_ctx.rederivation.clone();
                self.commit_graph()
                    .ancestors_difference_segment_slices(
                        &ctx,
                        heads.to_vec(),
                        last_derived,
                        batch_size,
                    )
                    .await
                    .map_err(Into::<DerivationError>::into)?
                    .try_for_each(|batch| {
                        borrowed!(ctx, self as ddm);
                        cloned!(rederivation);
                        async move {
                            ddm.derive_exactly_batch::<Derivable>(
                                ctx,
                                batch.to_vec(),
                                rederivation,
                            )
                            .await?;
                            Ok(())
                        }
                    })
                    .await
                    .map_err(Into::<DerivationError>::into)?;
                Ok(count)
            }
        }
        .map_err(|e: DerivationError| SharedDerivationError::from(e))
        .boxed();

        let mut visited = visited.lock().unwrap();

        visited
            .entry(Derivable::VARIANT)
            .or_insert(derivation_future.shared())
            .clone()
    }

    /// Find which ancestors of `csid` are not yet derived, and necessary for
    /// the derivation of `csid` to complete, and derive them.
    async fn derive_underived<Derivable>(
        &self,
        ctx: &CoreContext,
        target_csid: ChangesetId,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<DerivationOutcome<Derivable>, SharedDerivationError>
    where
        Derivable: BonsaiDerivable,
    {
        let count = self
            .derive_heads::<Derivable>(ctx.clone(), vec![target_csid], None, rederivation.clone())
            .await?;

        let derivation_ctx = self.derivation_context(rederivation);

        let derived = Derivable::fetch(ctx, &derivation_ctx, target_csid)
            .await
            .map_err(DerivationError::from)?
            .ok_or_else(|| {
                DerivationError::from(anyhow!(
                    "We just derived it! Fetching it should not return None"
                ))
            })?;
        Ok(DerivationOutcome { derived, count })
    }

    /// Count how many ancestors of `csid` are not yet derived.
    pub async fn count_underived<Derivable>(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        limit: Option<u64>,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<u64, DerivationError>
    where
        Derivable: BonsaiDerivable,
    {
        self.get_manager(ctx, csid)
            .await?
            .count_underived_impl::<Derivable>(ctx, csid, limit, rederivation)
            .await
    }

    async fn count_underived_impl<Derivable>(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        limit: Option<u64>,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<u64, DerivationError>
    where
        Derivable: BonsaiDerivable,
    {
        self.check_enabled::<Derivable>()?;
        let last_derived = self
            .commit_graph()
            .ancestors_frontier_with(ctx, vec![csid], |csid| {
                borrowed!(self as ddm, ctx);
                cloned!(rederivation);
                async move {
                    Ok(ddm
                        .fetch_derived::<Derivable>(ctx, csid, rederivation)
                        .await?
                        .is_some())
                }
            })
            .await
            .map_err(Into::<DerivationError>::into)?;
        let underived_count = self
            .commit_graph()
            .ancestors_difference_segments(ctx, vec![csid], last_derived)
            .await?
            .into_iter()
            .map(|segment| segment.length)
            .sum();
        // The limit is somewhat ficticious. Since underived_count is cheap to calculate, we don't
        // actually need to do magic to only partially evaluate the sum
        if let Some(limit) = limit {
            if underived_count > limit {
                return Ok(limit);
            }
        }
        Ok(underived_count)
    }

    /// Derive or retrieve derived data for a changeset.
    pub async fn derive<Derivable>(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<Derivable, SharedDerivationError>
    where
        Derivable: BonsaiDerivable,
    {
        if let Some(value) = self.fetch_derived(ctx, csid, rederivation.clone()).await? {
            Ok(value)
        } else if let Some(value) = self
            .derive_remotely(ctx, csid, rederivation.clone())
            .map_err(SharedDerivationError::from)
            .await?
        {
            Ok(value)
        } else {
            self.derive_locally(ctx, csid, rederivation).await
        }
    }

    /// Derive or retrieve derived data for a changeset using other derived data types
    /// without requiring data to be derived for the parents of the changeset.
    pub async fn derive_from_predecessor<Derivable>(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<Derivable, DerivationError>
    where
        Derivable: BonsaiDerivable,
    {
        if let Some(value) = self.fetch_derived(ctx, csid, rederivation.clone()).await? {
            return Ok(value);
        }

        let derivation_ctx = self.derivation_context(rederivation.clone());

        let bonsai = csid
            .load(ctx, derivation_ctx.blobstore())
            .await
            .map_err(Error::from)?;

        let ctx = ctx.clone_and_reset();
        let ctx = self.set_derivation_session_class(ctx);

        let mut derived_data_scuba = self.derived_data_scuba::<Derivable>();
        derived_data_scuba.add_changeset(&bonsai);
        derived_data_scuba.add_metadata(ctx.metadata());

        derived_data_scuba.log_derivation_start(&ctx);

        let predecessor_checks =
            Derivable::PredecessorDependencies::check_dependencies(&ctx, &derivation_ctx, csid)
                .await;
        // If predecessor derived data types are not derived yet, let's derive them
        if let Err(e) = predecessor_checks {
            Derivable::PredecessorDependencies::derive_predecessors(
                self,
                &ctx,
                csid,
                rederivation.clone(),
                &mut HashSet::new(),
            )
            .await
            .context("failed to derive predecessors")
            .context(e)?
        };

        let (derive_stats, derived) =
            Derivable::derive_from_predecessor(&ctx, &derivation_ctx, bonsai)
                .timed()
                .await;
        derivation_ctx.flush(&ctx).await?;

        derived_data_scuba.log_derivation_end(&ctx, &(derive_stats), derived.as_ref().err());

        let derived = derived?;

        let (persist_stats, persisted) = async {
            derived
                .clone()
                .store_mapping(&ctx, &derivation_ctx, csid)
                .await?;
            derivation_ctx.flush(&ctx).await?;
            if let Some(rederivation) = rederivation {
                rederivation.mark_derived(Derivable::VARIANT, csid);
            }
            Ok(())
        }
        .timed()
        .await;

        derived_data_scuba.log_mapping_insertion(
            &ctx,
            None,
            &persist_stats,
            persisted.as_ref().err(),
        );

        persisted?;

        Ok(derived)
    }

    // Derive remotely if possible.
    //
    // Returns `None` if remote derivation is not enabled, failed or timed out, and
    // local derivation should be attempted.
    pub async fn derive_remotely<Derivable>(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<Option<Derivable>, DerivationError>
    where
        Derivable: BonsaiDerivable,
    {
        if let Some(client) = self.derivation_service_client() {
            let mut attempt = 0;
            let started = Instant::now();
            // Total time to wait for remote derivation before giving up (and maybe deriving locally).
            let overall_timeout = Duration::from_millis(justknobs::get_as::<u64>(
                "scm/mononoke_timeouts:remote_derivation_client_timeout_ms",
                None,
            )?);
            // The maximum number of times to try remote derivation before giving up.
            const RETRY_ATTEMPTS_LIMIT: u32 = 10;
            // How long to wait between requests to the remote derivation service, either to
            // check for completion of ongoing remote derivation or after failures.
            let retry_delay = Duration::from_millis(justknobs::get_as::<u64>(
                "scm/mononoke_timeouts:remote_derivation_client_retry_delay_ms",
                None,
            )?);
            let request = DeriveRequest {
                repo_name: self.repo_name().to_string(),
                derived_data_type: DerivedDataType {
                    type_name: Derivable::NAME.to_string(),
                },
                changeset_id: csid.as_ref().to_vec(),
                bubble_id: self.bubble_id().map(|bubble_id| bubble_id.into()),
                config_name: self.config_name(),
                derivation_type: DerivationType::derive_underived(DeriveUnderived {}),
            };
            let mut request_state = DerivationState::NotRequested;
            let mut derived_data_scuba = self.derived_data_scuba::<Derivable>();
            derived_data_scuba.add_changeset_id(csid);

            // Try to perform remote derivation.  Capture the error so that we
            // can decide what to do.
            let derivation_error = loop {
                if justknobs::eval(
                    "scm/mononoke:derived_data_disable_remote_derivation",
                    None,
                    Some(self.repo_name()),
                )
                .unwrap_or_default()
                {
                    // Remote derivation has been disabled, fall back to local derivation.
                    return Ok(None);
                }

                if started.elapsed() >= overall_timeout {
                    derived_data_scuba.log_remote_derivation_end(
                        ctx,
                        Some(format!(
                            "Remote derivation timed out after {:?}",
                            overall_timeout
                        )),
                    );
                    break DerivationError::Timeout(Derivable::NAME, overall_timeout);
                }

                let service_response = match request_state {
                    DerivationState::NotRequested => {
                        // return if already derived
                        if let Some(data) =
                            self.fetch_derived(ctx, csid, rederivation.clone()).await?
                        {
                            return Ok(Some(data));
                        }
                        // not yet derived, so request derivation
                        derived_data_scuba.log_remote_derivation_start(ctx);
                        client.derive_remotely(ctx, &request).await
                    }
                    DerivationState::InProgress => client.poll(ctx, &request).await,
                };

                match service_response {
                    Ok(DeriveResponse { data, status }) => match (status, data) {
                        // Derivation was requested, set state InProgress and wait.
                        (RequestStatus::IN_PROGRESS, _) => {
                            request_state = DerivationState::InProgress;
                            tokio::time::sleep(retry_delay).await
                        }
                        // Derivation succeeded, return.
                        (RequestStatus::SUCCESS, Some(data)) => {
                            derived_data_scuba.log_remote_derivation_end(ctx, None);
                            return Ok(Some(Derivable::from_thrift(data)?));
                        }
                        // Either data was already derived or wasn't requested.
                        // Wait before requesting again.
                        (RequestStatus::DOES_NOT_EXIST, _) => {
                            request_state = DerivationState::NotRequested;
                            tokio::time::sleep(retry_delay).await
                        }
                        // Should not happen, reported success but data wasn't derived.
                        (RequestStatus::SUCCESS, None) => {
                            derived_data_scuba.log_remote_derivation_end(
                                ctx,
                                Some("Request succeeded but derived data is None".to_string()),
                            );
                            return Ok(None);
                        }
                        // Should not happen, derived data service returned an invalid status.
                        (RequestStatus(n), _) => {
                            derived_data_scuba.log_remote_derivation_end(
                                ctx,
                                Some(format!("Response with unknown state: {n}")),
                            );
                            return Ok(None);
                        }
                    },
                    Err(e) => {
                        if attempt >= RETRY_ATTEMPTS_LIMIT {
                            derived_data_scuba
                                .log_remote_derivation_end(ctx, Some(format!("{:#}", e)));
                            break DerivationError::Failed(Derivable::NAME, attempt, e);
                        }
                        attempt += 1;
                        tokio::time::sleep(retry_delay).await;
                    }
                }
            };

            // Derivation has failed or timed out.  Consider falling back to local derivation.
            if justknobs::eval(
                "scm/mononoke:derived_data_enable_remote_derivation_local_fallback",
                None,
                Some(self.repo_name()),
            )
            .unwrap_or_default()
            {
                // Discard the error and fall back to local derivation.
                Ok(None)
            } else {
                Err(derivation_error)
            }
        } else {
            // Derivation is not enabled, perform local derivation.
            Ok(None)
        }
    }

    pub async fn derive_locally<Derivable>(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<Derivable, SharedDerivationError>
    where
        Derivable: BonsaiDerivable,
    {
        self.get_manager(ctx, csid)
            .map_err(|e| SharedDerivationError::from(DerivationError::from(e)))
            .await?
            .derive_impl::<Derivable>(ctx, csid, rederivation)
            .await
    }

    async fn derive_impl<Derivable>(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<Derivable, SharedDerivationError>
    where
        Derivable: BonsaiDerivable,
    {
        self.check_enabled::<Derivable>()?;

        let pc = ctx.clone().fork_perf_counters();

        let (stats, res) = self
            .derive_underived(ctx, csid, rederivation)
            .timed()
            .fuse()
            .await;
        self.log_slow_derivation(ctx, csid, &stats, &pc, &res);
        Ok(res?.derived)
    }

    /// Derive data for exactly all underived changesets in a batch.
    ///
    /// The provided batch of changesets must be in topological
    /// order. The caller must have arranged for the dependencies
    /// and ancestors of the batch to have already been derived. If
    /// any dependency or ancestor is not already derived, an error
    /// will be returned.
    pub async fn derive_exactly_underived_batch<Derivable>(
        &self,
        ctx: &CoreContext,
        csids: Vec<ChangesetId>,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<Duration, DerivationError>
    where
        Derivable: BonsaiDerivable,
    {
        let derived = self
            .fetch_derived_batch::<Derivable>(ctx, csids.clone(), rederivation.clone())
            .await?;
        let underived = csids
            .into_iter()
            .filter(|csid| !derived.contains_key(csid))
            .collect();
        self.derive_exactly_batch::<Derivable>(ctx, underived, rederivation)
            .await
    }

    #[async_recursion]
    /// Derive data for exactly a batch of changesets.
    ///
    /// The provided batch of changesets must be in topological
    /// order.
    ///
    /// The difference between "derive_exactly" and "derive", is that for
    /// deriving exactly, the caller must have arranged for the dependencies
    /// and ancestors of the batch to have already been derived. If
    /// any dependency or ancestor is not already derived, an error
    /// will be returned.
    pub async fn derive_exactly_batch<Derivable>(
        &self,
        ctx: &CoreContext,
        csids: Vec<ChangesetId>,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<Duration, DerivationError>
    where
        Derivable: BonsaiDerivable,
    {
        let (csids, secondary_derivation) = if let Some(secondary_data) = &self.inner.secondary {
            let DerivationAssignment { primary, secondary } =
                secondary_data.assigner.assign(ctx, csids).await?;
            (primary, {
                cloned!(rederivation);
                async move {
                    secondary_data
                        .manager
                        .derive_exactly_batch::<Derivable>(ctx, secondary, rederivation)
                        .await
                }
                .left_future()
            })
        } else {
            (csids, future::ready(Ok(Duration::ZERO)).right_future())
        };
        self.check_enabled::<Derivable>()?;
        let mut derivation_ctx = self.derivation_context(rederivation.clone());

        // Enable write batching, so that writes are stored in memory
        // before being flushed.
        derivation_ctx.enable_write_batching();
        let derivation_ctx_ref = &derivation_ctx;

        let mut scuba = ctx.scuba().clone();
        scuba
            .add("stack_size", csids.len())
            .add("derived_data", Derivable::NAME);
        if let (Some(first), Some(last)) = (csids.first(), csids.last()) {
            scuba
                .add("first_csid", first.to_string())
                .add("last_csid", last.to_string());
        }

        // Load all of the bonsais for this batch.
        let bonsais = stream::iter(csids.iter().cloned().map(|csid| async move {
            let bonsai = csid.load(ctx, derivation_ctx_ref.blobstore()).await?;
            Ok::<_, Error>(bonsai)
        }))
        .buffered(100)
        .try_collect::<Vec<_>>()
        .await?;

        // Dependency checks: check topological order and determine heads
        // and highest ancestors of the batch.
        let mut seen = HashSet::new();
        let mut heads = HashSet::new();
        let mut ancestors = HashSet::new();
        for bonsai in bonsais.iter() {
            let csid = bonsai.get_changeset_id();
            if ancestors.contains(&csid) {
                return Err(anyhow!("batch not in topological order at {}", csid).into());
            }
            for parent in bonsai.parents() {
                if !seen.contains(&parent) {
                    ancestors.insert(parent);
                }
                heads.remove(&parent);
            }
            seen.insert(csid);
            heads.insert(csid);
        }

        // Dependency checks: all ancestors should have this derived
        // data type derived
        stream::iter(ancestors)
            .map(|csid| derivation_ctx_ref.fetch_dependency::<Derivable>(ctx, csid))
            .buffered(100)
            .try_for_each(|_| async { Ok(()) })
            .await
            .with_context(|| {
                format!(
                    "a batch ancestor does not have '{}' derived",
                    Derivable::NAME
                )
            })
            .context(concat!(
                "derive exactly batch pre-condition not satisfied: ",
                "all ancestors' and dependencies' data must already have been derived",
            ))?;

        // All heads should have their dependent data types derived.
        // Let's check if that's the case
        stream::iter(heads)
            .map(|csid| async move {
                Derivable::Dependencies::check_dependencies(ctx, derivation_ctx_ref, csid).await
            })
            .buffered(100)
            .try_for_each(|_| async { Ok(()) })
            .await
            .context("a batch dependency has not been derived")?;

        let ctx = ctx.clone_and_reset();
        let ctx = self.set_derivation_session_class(ctx.clone());
        borrowed!(ctx);

        let csid_range = if let (Some(first), Some(last)) = (bonsais.first(), bonsais.last()) {
            let first_csid = first.get_changeset_id();
            let last_csid = last.get_changeset_id();
            debug!(
                ctx.logger(),
                "derive exactly {} batch from {} to {}",
                Derivable::NAME,
                first_csid,
                last_csid,
            );
            Some((first_csid, last_csid))
        } else {
            None
        };

        let mut derived_data_scuba = self.derived_data_scuba::<Derivable>();
        derived_data_scuba.add_changesets(&bonsais);
        derived_data_scuba.log_batch_derivation_start(ctx);
        derived_data_scuba.add_metadata(ctx.metadata());
        let (overall_stats, result) = async {
            let derivation_ctx_ref = &derivation_ctx;
            let (batch_duration, derived) = {
                let (stats, derived) = Derivable::derive_batch(ctx, derivation_ctx_ref, bonsais)
                    .try_timed()
                    .await
                    .with_context(|| {
                        if let Some((first, last)) = csid_range {
                            format!(
                                "failed to derive {} batch (start:{}, end:{})",
                                Derivable::NAME,
                                first,
                                last
                            )
                        } else {
                            format!("failed to derive empty {} batch", Derivable::NAME)
                        }
                    })?;
                (stats.completion_time, derived)
            };

            // Flush the blobstore.  If it has been set up to cache writes, these
            // must be flushed before we write the mapping.
            derivation_ctx
                .flush(ctx)
                .try_timed()
                .await?
                .log_future_stats(scuba.clone(), "Flushed derived blobs", None);

            let mut derivation_ctx = self.derivation_context(rederivation.clone());
            derivation_ctx.enable_write_batching();
            // Write all mapping values, and flush the blobstore to ensure they
            // are persisted.
            let (persist_stats, persisted) = async {
                let derivation_ctx_ref = &derivation_ctx;
                let csids = stream::iter(derived.into_iter())
                    .map(|(csid, derived)| async move {
                        derived.store_mapping(ctx, derivation_ctx_ref, csid).await?;
                        Ok::<_, Error>(csid)
                    })
                    .buffer_unordered(100)
                    .try_collect::<Vec<_>>()
                    .await?;

                derivation_ctx.flush(ctx).await?;
                if let Some(rederivation) = rederivation {
                    for csid in csids {
                        rederivation.mark_derived(Derivable::VARIANT, csid);
                    }
                }
                Ok::<_, Error>(())
            }
            .timed()
            .await;

            derived_data_scuba.log_mapping_insertion(
                ctx,
                None,
                &persist_stats,
                persisted.as_ref().err(),
            );

            persisted?;

            scuba
                .add_future_stats(&persist_stats)
                .log_with_msg("Flushed mapping", None);

            Ok(batch_duration)
        }
        .timed()
        .await;

        derived_data_scuba.log_batch_derivation_end(ctx, &overall_stats, result.as_ref().err());

        let batch_duration = result?;

        Ok(batch_duration + secondary_derivation.await?)
    }

    /// Fetch derived data for a changeset if it has previously been derived.
    pub async fn fetch_derived<Derivable>(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<Option<Derivable>, DerivationError>
    where
        Derivable: BonsaiDerivable,
    {
        self.get_manager(ctx, csid)
            .await?
            .fetch_derived_impl::<Derivable>(ctx, csid, rederivation)
            .await
    }

    async fn fetch_derived_impl<Derivable>(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<Option<Derivable>, DerivationError>
    where
        Derivable: BonsaiDerivable,
    {
        self.check_enabled::<Derivable>()?;
        let derivation_ctx = self.derivation_context(rederivation);
        let derived = derivation_ctx.fetch_derived::<Derivable>(ctx, csid).await?;
        Ok(derived)
    }

    #[async_recursion]
    /// Fetch derived data for a batch of changesets if they have previously
    /// been derived.
    ///
    /// Returns a hashmap from changeset id to the derived data.  Changesets
    /// for which the data has not previously been derived are omitted.
    pub async fn fetch_derived_batch<Derivable>(
        &self,
        ctx: &CoreContext,
        csids: Vec<ChangesetId>,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Result<HashMap<ChangesetId, Derivable>, DerivationError>
    where
        Derivable: BonsaiDerivable,
    {
        let (csids, secondary_derivation) = if let Some(secondary_data) = &self.inner.secondary {
            let DerivationAssignment { primary, secondary } =
                secondary_data.assigner.assign(ctx, csids).await?;
            (primary, {
                cloned!(rederivation);
                async move {
                    secondary_data
                        .manager
                        .fetch_derived_batch::<Derivable>(ctx, secondary, rederivation)
                        .await
                }
                .left_future()
            })
        } else {
            (csids, future::ready(Ok(HashMap::new())).right_future())
        };
        self.check_enabled::<Derivable>()?;
        let derivation_ctx = self.derivation_context(rederivation);
        let mut derived = derivation_ctx
            .fetch_derived_batch::<Derivable>(ctx, csids)
            .await?;
        derived.extend(secondary_derivation.await?);
        Ok(derived)
    }
}

pub(super) struct DerivationOutcome<Derivable> {
    /// The derived data.
    pub(super) derived: Derivable,

    /// Number of changesets that were derived.
    pub(super) count: u64,
}

enum DerivationState {
    NotRequested,
    InProgress,
}
