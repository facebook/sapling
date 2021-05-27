/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{anyhow, Error};
use blobstore::PutBehaviour;
use blobstore::{Blobstore, Loadable, Storable};
use bookmarks::BookmarkName;
use context::CoreContext;
use megarepo_error::MegarepoError;
use memblob::Memblob;
use mononoke_types::RepositoryId;
use requests_table::{
    BlobstoreKey, LongRunningRequestsQueue, RequestId, RequestType, SqlLongRunningRequestsQueue,
};
use sql_construct::SqlConstruct;
use std::convert::TryFrom;
use std::convert::TryInto;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::types::{BlobstoreKeyWrapper, Request, ThriftParams, Token};

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
        let rust_params: <P::R as Request>::Params = thrift_params.try_into()?;
        let params_object_id: <P::R as Request>::ParamsId =
            rust_params.store(&ctx, &self.blobstore).await?;
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

    async fn poll_once<R: Request>(
        &self,
        ctx: &CoreContext,
        req_id: &RequestId,
    ) -> Result<Option<<R as Request>::StoredResult>, MegarepoError> {
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

        let result_blobstore_id =
            <R as Request>::StoredResultId::parse_blobstore_key(&result_blobstore_key.0)?;
        let result_obj: <R as Request>::StoredResult =
            result_blobstore_id.load(&ctx, &self.blobstore).await?;
        Ok(Some(result_obj))
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
            let maybe_stored_result: Option<<T::R as Request>::StoredResult> =
                self.poll_once::<T::R>(&ctx, &req_id).await?;
            let next_sleep = Duration::from_millis(rand::random::<u64>() % backoff_ms);

            match maybe_stored_result {
                Some(stored_result) => {
                    // Nice, the result is ready!
                    return <T::R as Request>::stored_result_into_poll_response(stored_result);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use blobstore::{Loadable, Storable};
    use context::CoreContext;
    use fbinit::FacebookInit;
    use requests_table::{ClaimedBy, RequestStatus};

    use source_control::{
        MegarepoAddTargetParams as ThriftMegarepoAddTargetParams,
        MegarepoChangeTargetConfigParams as ThriftMegarepoChangeTargetConfigParams,
        MegarepoRemergeSourceParams as ThriftMegarepoRemergeSourceParams,
        MegarepoSyncChangesetParams as ThriftMegarepoSyncChangesetParams,
    };

    use crate::types::{
        MegarepoAddTargetParamsId, MegarepoChangeTargetConfigParamsId,
        MegarepoRemergeSourceParamsId, MegarepoSyncChangesetParamsId,
    };
    use crate::types::{
        MegarepoAddTargetResult, MegarepoChangeTargetConfigResult, MegarepoRemergeSourceResult,
        MegarepoSyncChangesetResult,
    };
    use crate::types::{
        MegarepoAddTargetToken, MegarepoChangeTargetConfigToken, MegarepoRemergeSourceToken,
        MegarepoSyncChangesetToken,
    };

    macro_rules! test_enqueue_and_poll_once {
        {
            $fn_name: ident,
            $thrift_params: ident,
            $token: ident,
            $params_id: ident,
            $result: ident,
            $request_type: expr,
        } => {
            #[fbinit::test]
            async fn $fn_name(fb: FacebookInit) -> Result<(), Error> {
                let q = AsyncMethodRequestQueue::new_test_in_memory().unwrap();
                let ctx = CoreContext::test_mock(fb);

                // Enqueue a request
                let params: $thrift_params = Default::default();
                let token: $token = q.enqueue(ctx.clone(), params.clone()).await?;

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
                assert_eq!(
                    entry.request_type,
                    RequestType($request_type.to_string())
                );

                // Verify that request params are in the blobstore
                let id = $params_id::parse_blobstore_key(&entry.args_blobstore_key.0)?;
                let args: $thrift_params = id.load(&ctx, &q.blobstore).await?.into();
                assert_eq!(args, params);

                let req_id = RequestId(row_id, entry.request_type);

                // Verify that poll_once on this request in a "new" state
                // returns None
                let new_poll = q.poll_once::<<$token as Token>::R>(&ctx, &req_id).await?;
                assert!(new_poll.is_none());

                // Verify that poll_once on this request in a "in_progress" state
                // returns None
                q.table.mark_in_progress(&ctx, &req_id, &ClaimedBy("test".to_string())).await?;
                let in_progress_poll = q.poll_once::<<$token as Token>::R>(&ctx,  &req_id).await?;
                assert!(in_progress_poll.is_none());

                // Inject a result for this request
                // Verify that poll_once on this request in a "in_progress" state
                // returns injected result
                let fake_result = $result::from_thrift(Default::default());
                let fake_result_id = fake_result.clone().store(&ctx, &q.blobstore).await?;
                let fake_result_key = BlobstoreKey(fake_result_id.blobstore_key());
                q.table.mark_ready(&ctx, &req_id, fake_result_key).await?;
                let ready_poll = q.poll_once::<<$token as Token>::R>(&ctx, &req_id).await?;
                assert_eq!(ready_poll, Some(fake_result));

                // After a successful poll, request is marked as polled
                let entry = q.table.test_get_request_entry_by_id(&ctx, &req_id.0).await?.unwrap();
                assert_eq!(entry.status, RequestStatus::Polled);

                Ok(())
            }
        }
    }

    test_enqueue_and_poll_once! {
        test_enqueue_and_poll_once_add_target,
        ThriftMegarepoAddTargetParams,
        MegarepoAddTargetToken,
        MegarepoAddTargetParamsId,
        MegarepoAddTargetResult,
        "megarepo_add_sync_target",
    }

    test_enqueue_and_poll_once! {
        test_enqueue_and_poll_once_sync_changeset,
        ThriftMegarepoSyncChangesetParams,
        MegarepoSyncChangesetToken,
        MegarepoSyncChangesetParamsId,
        MegarepoSyncChangesetResult,
        "megarepo_sync_changeset",
    }

    test_enqueue_and_poll_once! {
        test_enqueue_and_poll_once_change_config,
        ThriftMegarepoChangeTargetConfigParams,
        MegarepoChangeTargetConfigToken,
        MegarepoChangeTargetConfigParamsId,
        MegarepoChangeTargetConfigResult,
        "megarepo_change_target_config",
    }

    test_enqueue_and_poll_once! {
        test_enqueue_and_poll_once_remerge_source,
        ThriftMegarepoRemergeSourceParams,
        MegarepoRemergeSourceToken,
        MegarepoRemergeSourceParamsId,
        MegarepoRemergeSourceResult,
        "megarepo_remerge_source",
    }
}
