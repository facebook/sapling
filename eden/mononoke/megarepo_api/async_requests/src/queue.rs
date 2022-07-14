/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use blobstore::Blobstore;
use blobstore::PutBehaviour;
use blobstore::Storable;
use bookmarks::BookmarkName;
use context::CoreContext;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use megarepo_error::MegarepoError;
use memblob::Memblob;
use mononoke_types::BlobstoreKey as BlobstoreKeyTrait;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;
use requests_table::BlobstoreKey;
pub use requests_table::ClaimedBy;
use requests_table::LongRunningRequestEntry;
use requests_table::LongRunningRequestsQueue;
pub use requests_table::RequestId;
use requests_table::RequestStatus;
use requests_table::RequestType;
pub use requests_table::RowId;
use requests_table::SqlLongRunningRequestsQueue;
use sql_construct::SqlConstruct;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use crate::types::MegarepoAsynchronousRequestParams;
use crate::types::MegarepoAsynchronousRequestResult;
use crate::types::Request;
use crate::types::ThriftParams;
use crate::types::Token;

const INITIAL_POLL_DELAY_MS: u64 = 1000;
const MAX_POLL_DURATION: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct AsyncMethodRequestQueue {
    blobstore: Arc<dyn Blobstore>,
    table: Arc<dyn LongRunningRequestsQueue>,
}

impl AsyncMethodRequestQueue {
    pub fn new(table: Arc<dyn LongRunningRequestsQueue>, blobstore: Arc<dyn Blobstore>) -> Self {
        Self { blobstore, table }
    }

    pub fn new_test_in_memory() -> Result<Self, Error> {
        let blobstore: Arc<dyn Blobstore> = Arc::new(Memblob::new(PutBehaviour::IfAbsent));
        let table: Arc<dyn LongRunningRequestsQueue> =
            Arc::new(SqlLongRunningRequestsQueue::with_sqlite_in_memory()?);

        Ok(Self { blobstore, table })
    }

    pub async fn enqueue<P: ThriftParams>(
        &self,
        ctx: CoreContext,
        thrift_params: P,
    ) -> Result<<P::R as Request>::Token, Error> {
        let request_type = RequestType(P::R::NAME.to_owned());
        let target = thrift_params.target().clone();
        let rust_params: MegarepoAsynchronousRequestParams = thrift_params.into();
        let params_object_id = rust_params.store(&ctx, &self.blobstore).await?;
        let blobstore_key = BlobstoreKey(params_object_id.blobstore_key());
        let table_id = self
            .table
            .add_request(
                &ctx,
                &request_type,
                &RepositoryId::new(i32::try_from(target.repo_id)?),
                &BookmarkName::new(&target.bookmark)?,
                &blobstore_key,
            )
            .await?;
        let token = <P::R as Request>::Token::from_db_id_and_target(table_id, target);
        Ok(token)
    }

    pub async fn dequeue(
        &self,
        ctx: &CoreContext,
        claimed_by: &ClaimedBy,
        supported_repos: &[RepositoryId],
    ) -> Result<Option<(RequestId, MegarepoAsynchronousRequestParams)>, MegarepoError> {
        let entry = self
            .table
            .claim_and_get_new_request(ctx, claimed_by, supported_repos)
            .await?;

        if let Some(entry) = entry {
            let thrift_params = MegarepoAsynchronousRequestParams::load_from_key(
                ctx,
                &self.blobstore,
                &entry.args_blobstore_key.0,
            )
            .await?;
            let req_id = RequestId(entry.id, entry.request_type);
            Ok(Some((req_id, thrift_params)))
        } else {
            // empty queue
            Ok(None)
        }
    }

    pub async fn complete(
        &self,
        ctx: &CoreContext,
        req_id: &RequestId,
        result: MegarepoAsynchronousRequestResult,
    ) -> Result<bool, MegarepoError> {
        let result_object_id = result.store(ctx, &self.blobstore).await?;
        let blobstore_key = BlobstoreKey(result_object_id.blobstore_key());
        Ok(self.table.mark_ready(ctx, req_id, blobstore_key).await?)
    }

    async fn poll_once<R: Request>(
        &self,
        ctx: &CoreContext,
        req_id: &RequestId,
    ) -> Result<Option<<R as Request>::ThriftResult>, MegarepoError> {
        let maybe_result_blobstore_key = match self.table.poll(ctx, req_id).await? {
            None => return Ok(None),
            Some((_, entry)) => entry.result_blobstore_key,
        };

        let result_blobstore_key = match maybe_result_blobstore_key {
            Some(rbk) => rbk,
            None => {
                return Err(MegarepoError::internal(anyhow!(
                    "Programming error: successful poll with empty result_blobstore_key for {:?}",
                    req_id
                )));
            }
        };

        let result: MegarepoAsynchronousRequestResult =
            MegarepoAsynchronousRequestResult::load_from_key(
                ctx,
                &self.blobstore,
                &result_blobstore_key.0,
            )
            .await?;
        Ok(Some(result.try_into()?))
    }

    pub async fn poll<T: Token>(
        &self,
        ctx: CoreContext,
        token: T,
    ) -> Result<<T::R as Request>::PollResponse, MegarepoError> {
        let mut backoff_ms = INITIAL_POLL_DELAY_MS;
        let before = Instant::now();
        let (row_id, _target) = token.to_db_id_and_target()?;
        let req_id = RequestId(row_id, RequestType(T::R::NAME.to_owned()));

        loop {
            let maybe_thrift_result: Option<<T::R as Request>::ThriftResult> =
                self.poll_once::<T::R>(&ctx, &req_id).await?;
            let next_sleep = Duration::from_millis(rand::random::<u64>() % backoff_ms);

            match maybe_thrift_result {
                Some(thrift_result) => {
                    // Nice, the result is ready!
                    return Ok(<T::R as Request>::thrift_result_into_poll_response(
                        thrift_result,
                    ));
                }
                None if before.elapsed() + next_sleep > MAX_POLL_DURATION => {
                    // The result is not yet ready, but we're out of time
                    return Ok(T::R::empty_poll_response());
                }
                None => {
                    // The result is not yet ready and we can wait a little longer
                    tokio::time::sleep(next_sleep).await;
                    backoff_ms *= 2;
                }
            }
        }
    }

    pub async fn update_in_progress_timestamp(
        &self,
        ctx: &CoreContext,
        req_id: &RequestId,
    ) -> Result<bool, MegarepoError> {
        Ok(self.table.update_in_progress_timestamp(ctx, req_id).await?)
    }

    pub async fn find_abandoned_requests(
        &self,
        ctx: &CoreContext,
        repo_ids: &[RepositoryId],
        abandoned_timestamp: Timestamp,
    ) -> Result<Vec<RequestId>, MegarepoError> {
        Ok(self
            .table
            .find_abandoned_requests(ctx, repo_ids, abandoned_timestamp)
            .await?)
    }

    pub async fn mark_abandoned_request_as_new(
        &self,
        ctx: &CoreContext,
        request_id: RequestId,
        abandoned_timestamp: Timestamp,
    ) -> Result<bool, MegarepoError> {
        Ok(self
            .table
            .mark_abandoned_request_as_new(ctx, request_id, abandoned_timestamp)
            .await?)
    }

    pub async fn requeue(
        &self,
        ctx: &CoreContext,
        request_id: RequestId,
    ) -> Result<bool, MegarepoError> {
        Ok(self.table.mark_new(ctx, &request_id).await?)
    }

    pub async fn list_requests(
        &self,
        ctx: &CoreContext,
        repo_ids: &[RepositoryId],
        statuses: &[RequestStatus],
        last_update_newer_than: Option<&Timestamp>,
    ) -> Result<
        Vec<(
            RequestId,
            LongRunningRequestEntry,
            MegarepoAsynchronousRequestParams,
        )>,
        MegarepoError,
    > {
        let entries = self
            .table
            .list_requests(ctx, repo_ids, statuses, last_update_newer_than)
            .await?;

        stream::iter(entries)
            .map(|entry| async {
                let thrift_params = MegarepoAsynchronousRequestParams::load_from_key(
                    ctx,
                    &self.blobstore,
                    &entry.args_blobstore_key.0,
                )
                .await?;
                let req_id = RequestId(entry.id.clone(), entry.request_type.clone());
                Ok::<_, MegarepoError>((req_id, entry, thrift_params))
            })
            .buffer_unordered(10)
            .try_collect()
            .await
    }

    pub async fn get_request_by_id(
        &self,
        ctx: &CoreContext,
        row_id: &RowId,
    ) -> Result<
        Option<(
            RequestId,
            LongRunningRequestEntry,
            MegarepoAsynchronousRequestParams,
            Option<MegarepoAsynchronousRequestResult>,
        )>,
        MegarepoError,
    > {
        let entry = self.table.test_get_request_entry_by_id(ctx, row_id).await?;

        if let Some(entry) = entry {
            let thrift_params = MegarepoAsynchronousRequestParams::load_from_key(
                ctx,
                &self.blobstore,
                &entry.args_blobstore_key.0,
            )
            .await?;
            let req_id = RequestId(entry.id.clone(), entry.request_type.clone());
            let thrift_result = if let Some(result_blobstore_key) = &entry.result_blobstore_key {
                Some(
                    MegarepoAsynchronousRequestResult::load_from_key(
                        ctx,
                        &self.blobstore,
                        &result_blobstore_key.0,
                    )
                    .await?,
                )
            } else {
                None
            };
            Ok(Some((req_id, entry, thrift_params, thrift_result)))
        } else {
            // empty queue
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use requests_table::ClaimedBy;
    use requests_table::RequestStatus;

    use source_control::MegarepoAddBranchingTargetParams as ThriftMegarepoAddBranchingTargetParams;
    use source_control::MegarepoAddTargetParams as ThriftMegarepoAddTargetParams;
    use source_control::MegarepoChangeTargetConfigParams as ThriftMegarepoChangeTargetConfigParams;
    use source_control::MegarepoRemergeSourceParams as ThriftMegarepoRemergeSourceParams;
    use source_control::MegarepoSyncChangesetParams as ThriftMegarepoSyncChangesetParams;

    use crate::types::MegarepoAsynchronousRequestResult;

    use source_control::MegarepoAddBranchingTargetResult;
    use source_control::MegarepoAddTargetResult;
    use source_control::MegarepoChangeTargetConfigResult;
    use source_control::MegarepoRemergeSourceResult;
    use source_control::MegarepoSyncChangesetResult;

    use crate::types::MegarepoAddBranchingSyncTarget;
    use crate::types::MegarepoAddSyncTarget;
    use crate::types::MegarepoChangeTargetConfig;
    use crate::types::MegarepoRemergeSource;
    use crate::types::MegarepoSyncChangeset;

    macro_rules! test_enqueue_dequeue_and_poll_once {
        {
            $fn_name: ident,
            $request_struct: ident,
            $thrift_params: ident,
            $result: ident,
            $request_type: expr,
        } => {
            #[fbinit::test]
            async fn $fn_name(fb: FacebookInit) -> Result<(), Error> {
                let q = AsyncMethodRequestQueue::new_test_in_memory().unwrap();
                let ctx = CoreContext::test_mock(fb);

                // Enqueue a request
                let params: $thrift_params = Default::default();
                let token = q.enqueue(ctx.clone(), params.clone()).await?;

                // Verify that request metadata is in the db and has expected values
                let (row_id, _) = token.to_db_id_and_target()?;
                let entry = q
                    .table
                    .test_get_request_entry_by_id(&ctx, &row_id)
                    .await?
                    .expect("Request is mising in the DB");
                assert_eq!(entry.status, RequestStatus::New);
                assert_eq!(entry.started_processing_at, None);
                assert_eq!(entry.ready_at, None);
                assert_eq!(entry.polled_at, None);
                assert_eq!(entry.repo_id,  RepositoryId::new(0));
                assert_eq!(
                    entry.request_type,
                    RequestType($request_type.to_string())
                );
                let req_id = RequestId(row_id, entry.request_type);

                // Verify that poll_once on this request in a "new" state
                // returns None
                let new_poll = q.poll_once::<$request_struct>(&ctx, &req_id).await?;
                assert!(new_poll.is_none());

                // Simulate the tailer and grab the element from the queue, this should return the params
                // back and flip its state back to "in_progress"
                let (req_id, params_from_store) = q.dequeue(&ctx, &ClaimedBy("tests".to_string()), &[entry.repo_id]).await?.unwrap();

                // Verify that request params from blobstore match what we put there
                assert_eq!(params_from_store, params.into());

                // Verify that request params are in the blobstore

                // Verify that poll_once on this request in a "in_progress" state
                // returns None
                let in_progress_poll = q.poll_once::<$request_struct>(&ctx,  &req_id).await?;
                assert!(in_progress_poll.is_none());

                // Inject a result for this request
                // Verify that poll_once on this request in a "in_progress" state
                // returns injected result
                let fake_specific_result: $result = Default::default();
                let fake_result: MegarepoAsynchronousRequestResult = fake_specific_result.clone().into();
                q.complete(&ctx, &req_id, fake_result).await?;
                let ready_poll = q.poll_once::<$request_struct>(&ctx, &req_id).await?;
                let ready_poll_response = ready_poll.unwrap();
                assert_eq!(ready_poll_response, fake_specific_result);

                // After a successful poll, request is marked as polled
                let entry = q.table.test_get_request_entry_by_id(&ctx, &req_id.0).await?.unwrap();
                assert_eq!(entry.status, RequestStatus::Polled);

                Ok(())
            }
        }
    }

    test_enqueue_dequeue_and_poll_once! {
        test_enqueue_dequeue_and_poll_once_add_target,
        MegarepoAddSyncTarget,
        ThriftMegarepoAddTargetParams,
        MegarepoAddTargetResult,
        "megarepo_add_sync_target",
    }

    test_enqueue_dequeue_and_poll_once! {
        test_enqueue_dequeue_and_poll_once_add_branching_target,
        MegarepoAddBranchingSyncTarget,
        ThriftMegarepoAddBranchingTargetParams,
        MegarepoAddBranchingTargetResult,
        "megarepo_add_branching_sync_target",
    }

    test_enqueue_dequeue_and_poll_once! {
        test_enqueue_dequeue_and_poll_once_sync_changeset,
        MegarepoSyncChangeset,
        ThriftMegarepoSyncChangesetParams,
        MegarepoSyncChangesetResult,
        "megarepo_sync_changeset",
    }

    test_enqueue_dequeue_and_poll_once! {
        test_enqueue_dequeue_and_poll_once_change_config,
        MegarepoChangeTargetConfig,
        ThriftMegarepoChangeTargetConfigParams,
        MegarepoChangeTargetConfigResult,
        "megarepo_change_target_config",
    }

    test_enqueue_dequeue_and_poll_once! {
        test_enqueue_dequeue_and_poll_once_remerge_source,
        MegarepoRemergeSource,
        ThriftMegarepoRemergeSourceParams,
        MegarepoRemergeSourceResult,
        "megarepo_remerge_source",
    }
}
