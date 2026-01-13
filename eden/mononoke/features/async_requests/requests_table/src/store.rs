/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use async_trait::async_trait;
use context::CoreContext;
use metaconfig_types::OssRemoteDatabaseConfig;
use metaconfig_types::OssRemoteMetadataDatabaseConfig;
use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::Connection;
use sql_ext::SqlConnections;
use sql_ext::mononoke_queries;

use crate::BlobstoreKey;
use crate::ClaimedBy;
use crate::LongRunningRequestEntry;
use crate::LongRunningRequestsQueue;
use crate::RequestId;
use crate::RequestStatus;
use crate::RequestType;
use crate::RowId;
use crate::types::QueueStats;
use crate::types::QueueStatsEntry;

mononoke_queries! {
    read TestGetRequest(id: RowId) -> (
        RowId,
        RequestType,
        Option<RepositoryId>,
        BlobstoreKey,
        Option<BlobstoreKey>,
        Timestamp,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        RequestStatus,
        Option<ClaimedBy>,
        Option<u8>,
        Option<Timestamp>,
    ) {
        "SELECT id,
            request_type,
            repo_id,
            args_blobstore_key,
            result_blobstore_key,
            created_at,
            started_processing_at,
            inprogress_last_updated_at,
            ready_at,
            polled_at,
            status,
            claimed_by,
            num_retries,
            failed_at
        FROM long_running_request_queue
        WHERE id = {id}"
    }

    read GetRequest(id: RowId, request_type: RequestType) -> (
        RowId,
        RequestType,
        Option<RepositoryId>,
        BlobstoreKey,
        Option<BlobstoreKey>,
        Timestamp,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        RequestStatus,
        Option<ClaimedBy>,
        Option<u8>,
        Option<Timestamp>,
    ) {
        "SELECT id,
            request_type,
            repo_id,
            args_blobstore_key,
            result_blobstore_key,
            created_at,
            started_processing_at,
            inprogress_last_updated_at,
            ready_at,
            polled_at,
            status,
            claimed_by,
            num_retries,
            failed_at
        FROM long_running_request_queue
        WHERE id = {id} AND request_type = {request_type}"
    }

    read GetOneNewRequestForGlobalQueue() -> (
        RowId,
        RequestType,
        Option<RepositoryId>,
        BlobstoreKey,
        Option<BlobstoreKey>,
        Timestamp,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        RequestStatus,
        Option<ClaimedBy>,
        Option<u8>,
        Option<Timestamp>,
    ) {
        mysql("SELECT id,
            request_type,
            repo_id,
            args_blobstore_key,
            result_blobstore_key,
            created_at,
            started_processing_at,
            inprogress_last_updated_at,
            ready_at,
            polled_at,
            status,
            claimed_by,
            num_retries,
            failed_at
        FROM long_running_request_queue
        WHERE status = 'new' AND repo_id IS NULL
        ORDER BY created_at ASC
        LIMIT 1
        ")
        sqlite("SELECT id,
            request_type,
            repo_id,
            args_blobstore_key,
            result_blobstore_key,
            created_at,
            started_processing_at,
            inprogress_last_updated_at,
            ready_at,
            polled_at,
            status,
            claimed_by,
            num_retries,
            failed_at
        FROM long_running_request_queue
        WHERE status = 'new' AND repo_id IS NULL
        ORDER BY created_at ASC
        LIMIT 1
        ")
    }

    read GetOneNewRequestForRepos(>list supported_repo_ids: RepositoryId) -> (
        RowId,
        RequestType,
        Option<RepositoryId>,
        BlobstoreKey,
        Option<BlobstoreKey>,
        Timestamp,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        RequestStatus,
        Option<ClaimedBy>,
        Option<u8>,
        Option<Timestamp>,
    ) {
        mysql("SELECT id,
            request_type,
            repo_id,
            args_blobstore_key,
            result_blobstore_key,
            created_at,
            started_processing_at,
            inprogress_last_updated_at,
            ready_at,
            polled_at,
            status,
            claimed_by,
            num_retries,
            failed_at
        FROM long_running_request_queue
        WHERE status = 'new' AND repo_id IN {supported_repo_ids}
        ORDER BY created_at ASC
        LIMIT 1
        ")
        sqlite("SELECT id,
            request_type,
            repo_id,
            args_blobstore_key,
            result_blobstore_key,
            created_at,
            started_processing_at,
            inprogress_last_updated_at,
            ready_at,
            polled_at,
            status,
            claimed_by,
            num_retries,
            failed_at
        FROM long_running_request_queue
        WHERE status = 'new' AND repo_id IN {supported_repo_ids}
        ORDER BY created_at ASC
        LIMIT 1
        ")
    }

    write AddRequestWithRepo(request_type: RequestType, repo_id: RepositoryId, args_blobstore_key: BlobstoreKey, created_at: Timestamp) {
        none,
        "INSERT INTO long_running_request_queue
         (request_type, repo_id, args_blobstore_key, status, created_at)
         VALUES ({request_type}, {repo_id}, {args_blobstore_key}, 'new', {created_at})
        "
    }

    write AddRequest(request_type: RequestType, args_blobstore_key: BlobstoreKey, created_at: Timestamp) {
        none,
        "INSERT INTO long_running_request_queue
         (request_type, args_blobstore_key, status, created_at)
         VALUES ({request_type}, {args_blobstore_key}, 'new', {created_at})
        "
    }

    read FindAbandonedRequestsForAnyRepo(abandoned_timestamp: Timestamp) -> (RowId, RequestType) {
        "
        SELECT id, request_type
        FROM long_running_request_queue
        WHERE status = 'inprogress' AND inprogress_last_updated_at <= {abandoned_timestamp}
        "
    }

    read FindAbandonedRequestsForRepos(
        abandoned_timestamp: Timestamp,
        >list repo_ids: RepositoryId
    ) -> (RowId, RequestType) {
        "
        SELECT id, request_type
        FROM long_running_request_queue
        WHERE repo_id IN {repo_ids} AND status = 'inprogress' AND inprogress_last_updated_at <= {abandoned_timestamp}
        "
    }

    write MarkRequestAsNewAgainIfAbandoned(
        id: RowId,
        request_type: RequestType,
        abandoned_timestamp: Timestamp,
    )
    {
        none,
        "UPDATE long_running_request_queue
         SET status = 'new', claimed_by = NULL, inprogress_last_updated_at = NULL
         WHERE id = {id} AND request_type = {request_type} AND status = 'inprogress' AND inprogress_last_updated_at <= {abandoned_timestamp}
        "
    }

    write MarkRequestInProgress(
        id: RowId,
        request_type: RequestType,
        started_processing_at: Timestamp,
        claimed_by: ClaimedBy,
    ) {
        none,
        "UPDATE long_running_request_queue
         SET started_processing_at = {started_processing_at},
             inprogress_last_updated_at = {started_processing_at},
             status = 'inprogress',
             claimed_by = {claimed_by}
         WHERE id = {id} AND request_type = {request_type} AND status = 'new'
        "
    }

    write UpdateInProgressTimestamp(
        id: RowId,
        request_type: RequestType,
        inprogress_last_updated_at: Timestamp,
    ) {
        none,
        "UPDATE long_running_request_queue
         SET inprogress_last_updated_at = {inprogress_last_updated_at}
         WHERE id = {id} AND request_type = {request_type} AND status = 'inprogress'
        "
    }

    write MarkRequestReady(id: RowId, request_type: RequestType, ready_at: Timestamp, result_blobstore_key: BlobstoreKey) {
        none,
        "UPDATE long_running_request_queue
         SET ready_at = {ready_at}, status = 'ready', result_blobstore_key = {result_blobstore_key}
         WHERE id = {id} AND request_type = {request_type} AND status = 'inprogress'
        "
    }

    write MarkRequestAsNew(id: RowId, request_type: RequestType) {
        none,
        "UPDATE long_running_request_queue
         SET status = 'new'
         WHERE id = {id} AND request_type = {request_type}
        "
    }

    write MarkRequestPolled(id: RowId, request_type: RequestType, polled_at: Timestamp) {
        none,
        "UPDATE long_running_request_queue
         SET polled_at = {polled_at}, status = 'polled'
         WHERE id = {id} AND request_type = {request_type} AND status = 'ready'
        "
    }

    write MarkRequestFailed(id: RowId, request_type: RequestType, failed_at: Timestamp) {
        none,
        "
        UPDATE long_running_request_queue
        SET status = 'failed', failed_at = {failed_at}
        WHERE id = {id} AND request_type = {request_type} AND status = 'inprogress'
        "
    }

    write MarkRequestAsNewForRetry(id: RowId, request_type: RequestType, num_retries: u8) {
        none,
        "
        UPDATE long_running_request_queue
        SET status = 'new', claimed_by = NULL, inprogress_last_updated_at = NULL, num_retries = {num_retries}
        WHERE id = {id} AND request_type = {request_type} AND status = 'inprogress'
        "
    }

    write TestMark(id: RowId, status: RequestStatus) {
        none,
        "UPDATE long_running_request_queue
         SET status = {status}
         WHERE id = {id}
        "
    }

    read ListRequestsForAnyRepo(last_update_newer_than: Timestamp) -> (
        RowId,
        RequestType,
        Option<RepositoryId>,
        BlobstoreKey,
        Option<BlobstoreKey>,
        Timestamp,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        RequestStatus,
        Option<ClaimedBy>,
        Option<u8>,
        Option<Timestamp>,
    ) {
       mysql( "SELECT id,
            request_type,
            repo_id,
            args_blobstore_key,
            result_blobstore_key,
            created_at,
            started_processing_at,
            inprogress_last_updated_at,
            ready_at,
            polled_at,
            status,
            claimed_by,
            num_retries,
            failed_at
        FROM long_running_request_queue
        FORCE INDEX (list_requests_any)
        WHERE (
            inprogress_last_updated_at > {last_update_newer_than} OR
            (status = 'new' AND created_at > {last_update_newer_than})
        )")
        sqlite( "SELECT id,
            request_type,
            repo_id,
            args_blobstore_key,
            result_blobstore_key,
            created_at,
            started_processing_at,
            inprogress_last_updated_at,
            ready_at,
            polled_at,
            status,
            claimed_by,
            num_retries,
            failed_at
        FROM long_running_request_queue
        WHERE (
            inprogress_last_updated_at > {last_update_newer_than} OR
            (status = 'new' AND created_at > {last_update_newer_than})
        )")
    }

    read ListRequestsForRepos(last_update_newer_than: Timestamp, >list repo_ids: RepositoryId) -> (
        RowId,
        RequestType,
        Option<RepositoryId>,
        BlobstoreKey,
        Option<BlobstoreKey>,
        Timestamp,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        RequestStatus,
        Option<ClaimedBy>,
        Option<u8>,
        Option<Timestamp>,
    ) {
        mysql("SELECT id,
            request_type,
            repo_id,
            args_blobstore_key,
            result_blobstore_key,
            created_at,
            started_processing_at,
            inprogress_last_updated_at,
            ready_at,
            polled_at,
            status,
            claimed_by,
            num_retries,
            failed_at
        FROM long_running_request_queue
        FORCE INDEX (list_requests)
        WHERE repo_id IN {repo_ids} AND (
            inprogress_last_updated_at > {last_update_newer_than} OR
            (status = 'new' AND created_at > {last_update_newer_than})
        )")
        sqlite("SELECT id,
            request_type,
            repo_id,
            args_blobstore_key,
            result_blobstore_key,
            created_at,
            started_processing_at,
            inprogress_last_updated_at,
            ready_at,
            polled_at,
            status,
            claimed_by,
            num_retries,
            failed_at
        FROM long_running_request_queue
        WHERE repo_id IN {repo_ids} AND (
            inprogress_last_updated_at > {last_update_newer_than} OR
            (status = 'new' AND created_at > {last_update_newer_than})
        )")
    }

    read GetQueueLengthForRepos(>list repo_ids: RepositoryId) -> (
        RequestStatus, u64
    ) {
        "SELECT status, count(*) FROM long_running_request_queue WHERE repo_id IN {repo_ids} GROUP BY status"
    }

    read GetQueueLengthByRepoForRepos(>list repo_ids: RepositoryId) -> (
        Option<RepositoryId>, RequestStatus, u64
    ) {
        "SELECT repo_id, status, count(*) FROM long_running_request_queue WHERE repo_id IN {repo_ids} GROUP BY repo_id, status"
    }

    read GetQueueLengthForAllRepos() -> (
        RequestStatus, u64
    ) {
        "SELECT status, count(*) FROM long_running_request_queue GROUP BY status"
    }

    read GetQueueLengthByRepoForAllRepos() -> (
        Option<RepositoryId>, RequestStatus, u64
    ) {
        "SELECT repo_id, status, count(*) FROM long_running_request_queue GROUP BY repo_id, status"
    }

    read GetQueueAgeForRepos(>list repo_ids: RepositoryId) -> (
        RequestStatus, u64, Option<u64>, Option<u64>
    ) {
        "SELECT status, min(created_at), min(inprogress_last_updated_at), min(ready_at)
        FROM long_running_request_queue
        WHERE repo_id IN {repo_ids} AND status NOT IN ('polled', 'failed')
        GROUP BY status
        "
    }

    read GetQueueAgeByRepoForRepos(>list repo_ids: RepositoryId) -> (
        Option<RepositoryId>, RequestStatus, u64, Option<u64>, Option<u64>
    ) {
        "SELECT repo_id, status, min(created_at), min(inprogress_last_updated_at), min(ready_at)
        FROM long_running_request_queue
        WHERE repo_id IN {repo_ids} AND status NOT IN ('polled', 'failed')
        GROUP BY repo_id, status
        "
    }

    read GetQueueAgeForAllRepos() -> (
        RequestStatus, u64, Option<u64>, Option<u64>
    ) {
        "SELECT status, min(created_at), min(inprogress_last_updated_at), min(ready_at)
        FROM long_running_request_queue
        WHERE status NOT IN ('polled', 'failed')
        GROUP BY status
        "
    }

    read GetQueueAgeByRepoForAllRepos() -> (
        Option<RepositoryId>, RequestStatus, u64, Option<u64>, Option<u64>
    ) {
        "SELECT repo_id, status, min(created_at), min(inprogress_last_updated_at), min(ready_at)
        FROM long_running_request_queue
        WHERE status NOT IN ('polled', 'failed')
        GROUP BY repo_id, status
        "
    }
}

fn row_to_entry(
    row: (
        RowId,
        RequestType,
        Option<RepositoryId>,
        BlobstoreKey,
        Option<BlobstoreKey>,
        Timestamp,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        RequestStatus,
        Option<ClaimedBy>,
        Option<u8>,
        Option<Timestamp>,
    ),
) -> LongRunningRequestEntry {
    let (
        id,
        request_type,
        repo_id,
        args_blobstore_key,
        result_blobstore_key,
        created_at,
        started_processing_at,
        inprogress_last_updated_at,
        ready_at,
        polled_at,
        status,
        claimed_by,
        num_retries,
        failed_at,
    ) = row;
    LongRunningRequestEntry {
        id,
        repo_id,
        request_type,
        args_blobstore_key,
        result_blobstore_key,
        created_at,
        started_processing_at,
        inprogress_last_updated_at,
        ready_at,
        polled_at,
        status,
        claimed_by,
        num_retries,
        failed_at,
    }
}

#[derive(Clone)]
pub struct SqlLongRunningRequestsQueue {
    pub(crate) connections: SqlConnections,
}

#[async_trait]
impl LongRunningRequestsQueue for SqlLongRunningRequestsQueue {
    async fn add_request(
        &self,
        ctx: &CoreContext,
        request_type: &RequestType,
        repo_id: Option<&RepositoryId>,
        args_blobstore_key: &BlobstoreKey,
    ) -> Result<RowId> {
        let res = match &repo_id {
            Some(repo_id) => {
                AddRequestWithRepo::query(
                    &self.connections.write_connection,
                    ctx.sql_query_telemetry(),
                    request_type,
                    repo_id,
                    args_blobstore_key,
                    &Timestamp::now(),
                )
                .await?
            }
            None => {
                AddRequest::query(
                    &self.connections.write_connection,
                    ctx.sql_query_telemetry(),
                    request_type,
                    args_blobstore_key,
                    &Timestamp::now(),
                )
                .await?
            }
        };

        match res.last_insert_id() {
            Some(last_insert_id) if res.affected_rows() == 1 => Ok(RowId(last_insert_id)),
            _ => bail!("Failed to insert a new request of type {}", request_type),
        }
    }

    /// Claim one of new requests. Mark it as in-progress and return it.
    async fn claim_and_get_new_request(
        &self,
        ctx: &CoreContext,
        claimed_by: &ClaimedBy,
        supported_repos: Option<&[RepositoryId]>,
    ) -> Result<Option<LongRunningRequestEntry>> {
        // Spin until we win the race or there's nothing to do.
        loop {
            let connection = &self.connections.read_master_connection; // reaching DB master improves our chances.
            let rows = match supported_repos {
                Some(repos) => {
                    GetOneNewRequestForRepos::query(connection, ctx.sql_query_telemetry(), repos)
                        .await
                }
                None => {
                    GetOneNewRequestForGlobalQueue::query(connection, ctx.sql_query_telemetry())
                        .await
                }
            }
            .context("claiming new request")?;
            let mut entry = match rows.into_iter().next() {
                None => {
                    return Ok(None);
                }
                Some(row) => row_to_entry(row),
            };
            if self
                .mark_in_progress(
                    ctx,
                    &RequestId(entry.id, entry.request_type.clone()),
                    claimed_by,
                )
                .await?
            {
                // Success, we won the race!
                entry.status = RequestStatus::InProgress;
                return Ok(Some(entry));
            }
            // Failure, let's try again.
        }
    }

    async fn test_get_request_entry_by_id(
        &self,
        ctx: &CoreContext,
        id: &RowId,
    ) -> Result<Option<LongRunningRequestEntry>> {
        let rows = TestGetRequest::query(
            &self.connections.read_connection,
            ctx.sql_query_telemetry(),
            id,
        )
        .await?;
        match rows.into_iter().next() {
            None => Ok(None),
            Some(row) => Ok(Some(row_to_entry(row))),
        }
    }

    async fn mark_in_progress(
        &self,
        ctx: &CoreContext,
        req_id: &RequestId,
        claimed_by: &ClaimedBy,
    ) -> Result<bool> {
        let res = MarkRequestInProgress::query(
            &self.connections.write_connection,
            ctx.sql_query_telemetry(),
            &req_id.0,
            &req_id.1,
            &Timestamp::now(),
            claimed_by,
        )
        .await?;
        Ok(res.affected_rows() > 0)
    }

    async fn update_in_progress_timestamp(
        &self,
        ctx: &CoreContext,
        req_id: &RequestId,
    ) -> Result<bool> {
        let res = UpdateInProgressTimestamp::query(
            &self.connections.write_connection,
            ctx.sql_query_telemetry(),
            &req_id.0,
            &req_id.1,
            &Timestamp::now(),
        )
        .await?;
        Ok(res.affected_rows() > 0)
    }

    async fn find_abandoned_requests(
        &self,
        ctx: &CoreContext,
        repo_ids: Option<&[RepositoryId]>,
        abandoned_timestamp: Timestamp,
    ) -> Result<Vec<RequestId>> {
        let rows = match repo_ids {
            Some(repos) => {
                FindAbandonedRequestsForRepos::query(
                    &self.connections.write_connection,
                    ctx.sql_query_telemetry(),
                    &abandoned_timestamp,
                    repos,
                )
                .await
            }
            None => {
                FindAbandonedRequestsForAnyRepo::query(
                    &self.connections.write_connection,
                    ctx.sql_query_telemetry(),
                    &abandoned_timestamp,
                )
                .await
            }
        }
        .context("finding abandoned requests")?;
        Ok(rows.into_iter().map(|(id, ty)| RequestId(id, ty)).collect())
    }

    async fn mark_abandoned_request_as_new(
        &self,
        ctx: &CoreContext,
        request_id: RequestId,
        abandoned_timestamp: Timestamp,
    ) -> Result<bool> {
        let res = MarkRequestAsNewAgainIfAbandoned::query(
            &self.connections.write_connection,
            ctx.sql_query_telemetry(),
            &request_id.0,
            &request_id.1,
            &abandoned_timestamp,
        )
        .await?;

        Ok(res.affected_rows() > 0)
    }

    async fn mark_ready(
        &self,
        ctx: &CoreContext,
        req_id: &RequestId,
        blobstore_result_key: BlobstoreKey,
    ) -> Result<bool> {
        let res = MarkRequestReady::query(
            &self.connections.write_connection,
            ctx.sql_query_telemetry(),
            &req_id.0,
            &req_id.1,
            &Timestamp::now(),
            &blobstore_result_key,
        )
        .await?;

        Ok(res.affected_rows() > 0)
    }

    async fn mark_new(&self, ctx: &CoreContext, req_id: &RequestId) -> Result<bool> {
        let res = MarkRequestAsNew::query(
            &self.connections.write_connection,
            ctx.sql_query_telemetry(),
            &req_id.0,
            &req_id.1,
        )
        .await?;

        Ok(res.affected_rows() > 0)
    }

    async fn test_mark(
        &self,
        ctx: &CoreContext,
        row_id: &RowId,
        status: RequestStatus,
    ) -> Result<bool> {
        let res = TestMark::query(
            &self.connections.write_connection,
            ctx.sql_query_telemetry(),
            row_id,
            &status,
        )
        .await?;
        Ok(res.affected_rows() > 0)
    }

    async fn poll(
        &self,
        ctx: &CoreContext,
        req_id: &RequestId,
    ) -> Result<Option<(bool, LongRunningRequestEntry)>> {
        let txn = self
            .connections
            .write_connection
            .start_transaction(ctx.sql_query_telemetry())
            .await?;

        let (mut txn, rows) = GetRequest::query_with_transaction(txn, &req_id.0, &req_id.1).await?;
        let entry = match rows.into_iter().next() {
            None => bail!("unknown request polled: {:?}", req_id),
            Some(row) => {
                let mut entry = row_to_entry(row);

                match &entry.status {
                    RequestStatus::Ready | RequestStatus::Polled
                        if entry.result_blobstore_key.is_none() =>
                    {
                        bail!(
                            "no result stored for {:?} request {:?}",
                            entry.status,
                            req_id
                        );
                    }
                    RequestStatus::Ready => {
                        txn = MarkRequestPolled::query_with_transaction(
                            txn,
                            &req_id.0,
                            &req_id.1,
                            &Timestamp::now(),
                        )
                        .await?
                        .0;

                        entry.status = RequestStatus::Polled;
                        Some((true, entry))
                    }
                    RequestStatus::Polled => Some((false, entry)),
                    _ => None,
                }
            }
        };
        txn.commit().await?;
        Ok(entry)
    }

    async fn list_requests(
        &self,
        ctx: &CoreContext,
        repo_ids: Option<&[RepositoryId]>,
        last_update_newer_than: Option<&Timestamp>,
    ) -> Result<Vec<LongRunningRequestEntry>> {
        let entries = match repo_ids {
            Some(repos) => {
                ListRequestsForRepos::query(
                    &self.connections.read_connection,
                    ctx.sql_query_telemetry(),
                    last_update_newer_than.unwrap_or(&Timestamp::from_timestamp_nanos(0)),
                    repos,
                )
                .await
            }
            None => {
                ListRequestsForAnyRepo::query(
                    &self.connections.read_connection,
                    ctx.sql_query_telemetry(),
                    last_update_newer_than.unwrap_or(&Timestamp::from_timestamp_nanos(0)),
                )
                .await
            }
        }
        .context("listing requests")?
        .into_iter()
        .map(row_to_entry)
        .collect();
        Ok(entries)
    }

    async fn get_queue_stats(
        &self,
        ctx: &CoreContext,
        repo_ids: Option<&[RepositoryId]>,
    ) -> Result<QueueStats> {
        Ok(QueueStats {
            queue_length_by_status: get_queue_length(
                ctx,
                &self.connections.read_connection,
                repo_ids,
            )
            .await?,
            queue_age_by_status: get_queue_age(ctx, &self.connections.read_connection, repo_ids)
                .await?,
            queue_length_by_repo_and_status: get_queue_length_by_repo(
                ctx,
                &self.connections.read_connection,
                repo_ids,
            )
            .await?,
            queue_age_by_repo_and_status: get_queue_age_by_repo(
                ctx,
                &self.connections.read_connection,
                repo_ids,
            )
            .await?,
        })
    }

    async fn update_for_retry_or_fail(
        &self,
        ctx: &CoreContext,
        req_id: &RequestId,
        max_retry_allowed: u8,
    ) -> Result<bool> {
        let txn = self
            .connections
            .write_connection
            .start_transaction(ctx.sql_query_telemetry())
            .await?;

        let (mut txn, rows) = GetRequest::query_with_transaction(txn, &req_id.0, &req_id.1).await?;
        let will_retry = match rows.into_iter().next() {
            None => bail!("Failed to get request: {:?}", req_id),
            Some(row) => {
                let entry = row_to_entry(row);
                match &entry.status {
                    RequestStatus::InProgress => {
                        let next_retry = entry.num_retries.unwrap_or(0) + 1;
                        if next_retry > max_retry_allowed {
                            txn = MarkRequestFailed::query_with_transaction(
                                txn,
                                &req_id.0,
                                &req_id.1,
                                &Timestamp::now(),
                            )
                            .await?
                            .0;
                            Ok(false)
                        } else {
                            txn = MarkRequestAsNewForRetry::query_with_transaction(
                                txn,
                                &req_id.0,
                                &req_id.1,
                                &next_retry,
                            )
                            .await?
                            .0;
                            Ok(true)
                        }
                    }
                    _ => bail!(
                        "Request {:?} is not in progress, it can't be retried",
                        req_id
                    ),
                }
            }
        };
        txn.commit().await?;

        will_retry
    }
}

async fn get_queue_length(
    ctx: &CoreContext,
    conn: &Connection,
    repo_ids: Option<&[RepositoryId]>,
) -> Result<Vec<(RequestStatus, u64)>> {
    Ok(match repo_ids {
        Some(repos) => GetQueueLengthForRepos::query(conn, ctx.sql_query_telemetry(), repos).await,
        None => GetQueueLengthForAllRepos::query(conn, ctx.sql_query_telemetry()).await,
    }
    .context("fetching queue length stats")?
    .into_iter()
    .collect())
}

async fn get_queue_length_by_repo(
    ctx: &CoreContext,
    conn: &Connection,
    repo_ids: Option<&[RepositoryId]>,
) -> Result<Vec<(QueueStatsEntry, u64)>> {
    Ok(match repo_ids {
        Some(repos) => {
            GetQueueLengthByRepoForRepos::query(conn, ctx.sql_query_telemetry(), repos).await
        }
        None => GetQueueLengthByRepoForAllRepos::query(conn, ctx.sql_query_telemetry()).await,
    }
    .context("fetching queue length stats")?
    .into_iter()
    .map(|(repo_id, status, count)| (QueueStatsEntry { repo_id, status }, count))
    .collect())
}
async fn get_queue_age(
    ctx: &CoreContext,
    conn: &Connection,
    repo_ids: Option<&[RepositoryId]>,
) -> Result<Vec<(RequestStatus, Timestamp)>> {
    Ok(match repo_ids {
        Some(repos) => GetQueueAgeForRepos::query(conn, ctx.sql_query_telemetry(), repos).await,
        None => GetQueueAgeForAllRepos::query(conn, ctx.sql_query_telemetry()).await,
    }
    .context("fetching queue age stats")?
    .into_iter()
    .map(
        |(status, created_at, inprogress_last_updated_at, ready_at)| {
            match &status {
                RequestStatus::New => (status, created_at),
                RequestStatus::InProgress => (status, inprogress_last_updated_at.unwrap_or(0)),
                RequestStatus::Ready => (status, ready_at.unwrap_or(0)),
                RequestStatus::Polled | RequestStatus::Failed => (status, 0), // should not happen, but if it does we'll ignore
            }
        },
    )
    .map(|(status, timestamp)| (status, Timestamp::from_timestamp_nanos(timestamp as i64)))
    .collect())
}

async fn get_queue_age_by_repo(
    ctx: &CoreContext,
    conn: &Connection,
    repo_ids: Option<&[RepositoryId]>,
) -> Result<Vec<(QueueStatsEntry, Timestamp)>> {
    Ok(match repo_ids {
        Some(repos) => {
            GetQueueAgeByRepoForRepos::query(conn, ctx.sql_query_telemetry(), repos).await
        }
        None => GetQueueAgeByRepoForAllRepos::query(conn, ctx.sql_query_telemetry()).await,
    }
    .context("fetching queue age stats")?
    .into_iter()
    .map(
        |(repo_id, status, created_at, inprogress_last_updated_at, ready_at)| {
            match &status {
                RequestStatus::New => (repo_id, status, created_at),
                RequestStatus::InProgress => {
                    (repo_id, status, inprogress_last_updated_at.unwrap_or(0))
                }
                RequestStatus::Ready => (repo_id, status, ready_at.unwrap_or(0)),
                RequestStatus::Polled | RequestStatus::Failed => (repo_id, status, 0), // should not happen, but if it does we'll ignore
            }
        },
    )
    .map(|(repo_id, status, timestamp)| {
        (
            QueueStatsEntry { repo_id, status },
            Timestamp::from_timestamp_nanos(timestamp as i64),
        )
    })
    .collect())
}

impl SqlConstruct for SqlLongRunningRequestsQueue {
    const LABEL: &'static str = "long_running_requests_queue";

    const CREATION_QUERY: &'static str =
        include_str!("../schemas/sqlite-long_running_requests_queue.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlLongRunningRequestsQueue {
    fn remote_database_config(
        remote: &RemoteMetadataDatabaseConfig,
    ) -> Option<&RemoteDatabaseConfig> {
        Some(&remote.production)
    }
    fn oss_remote_database_config(
        remote: &OssRemoteMetadataDatabaseConfig,
    ) -> Option<&OssRemoteDatabaseConfig> {
        Some(&remote.production)
    }
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use fbinit::FacebookInit;
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::fbinit_test]
    async fn test_claim_and_get_new_request_for_global_queue(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let queue = SqlLongRunningRequestsQueue::with_sqlite_in_memory()?;
        let id = queue
            .add_request(
                &ctx,
                &RequestType("type".to_string()),
                None,
                &BlobstoreKey("key".to_string()),
            )
            .await?;

        let request = queue.test_get_request_entry_by_id(&ctx, &id).await?;
        assert!(request.is_some());
        let request = request.unwrap();
        assert!(request.inprogress_last_updated_at.is_none());

        let result = queue
            .claim_and_get_new_request(&ctx, &ClaimedBy("me".to_string()), None)
            .await;
        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.is_some());
        let result = result.unwrap();
        assert!(result.id == id);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_claim_and_get_new_request_by_repo_id(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let queue = SqlLongRunningRequestsQueue::with_sqlite_in_memory()?;
        let id = queue
            .add_request(
                &ctx,
                &RequestType("type".to_string()),
                Some(&RepositoryId::new(0)),
                &BlobstoreKey("key".to_string()),
            )
            .await?;

        let request = queue.test_get_request_entry_by_id(&ctx, &id).await?;
        assert!(request.is_some());
        let request = request.unwrap();
        assert!(request.inprogress_last_updated_at.is_none());

        // passing None does *not* match any repo id; it only matches global queue
        let result = queue
            .claim_and_get_new_request(&ctx, &ClaimedBy("me".to_string()), None)
            .await;
        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.is_none());

        // different repo id
        let result = queue
            .claim_and_get_new_request(
                &ctx,
                &ClaimedBy("me".to_string()),
                Some(&[RepositoryId::new(1)]),
            )
            .await;
        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.is_none());

        // correct repo id
        let result = queue
            .claim_and_get_new_request(
                &ctx,
                &ClaimedBy("me".to_string()),
                Some(&[
                    RepositoryId::new(0),
                    RepositoryId::new(1),
                    RepositoryId::new(2),
                ]),
            )
            .await;
        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.is_some());
        let result = result.unwrap();
        assert!(result.id == id);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_mark_inprogress(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let queue = SqlLongRunningRequestsQueue::with_sqlite_in_memory()?;
        let id = queue
            .add_request(
                &ctx,
                &RequestType("type".to_string()),
                None,
                &BlobstoreKey("key".to_string()),
            )
            .await?;

        let request = queue.test_get_request_entry_by_id(&ctx, &id).await?;
        assert!(request.is_some());
        let request = request.unwrap();
        assert!(request.inprogress_last_updated_at.is_none());

        queue
            .claim_and_get_new_request(&ctx, &ClaimedBy("me".to_string()), None)
            .await?;

        let request = queue.test_get_request_entry_by_id(&ctx, &id).await?;
        assert!(request.is_some());
        let request = request.unwrap();
        assert!(request.inprogress_last_updated_at.is_some());

        let timestamp = request.inprogress_last_updated_at.unwrap();

        tokio::time::sleep(Duration::from_secs(3)).await;

        let updated = queue
            .update_in_progress_timestamp(&ctx, &RequestId(request.id, request.request_type))
            .await?;
        assert!(updated);
        let request = queue.test_get_request_entry_by_id(&ctx, &id).await?;
        // Check that timestamp was updated
        assert!(request.unwrap().inprogress_last_updated_at.unwrap() > timestamp);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_find_abandoned_requests(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let queue = SqlLongRunningRequestsQueue::with_sqlite_in_memory()?;
        let repo_id = RepositoryId::new(1);
        let id = queue
            .add_request(
                &ctx,
                &RequestType("type".to_string()),
                Some(&repo_id),
                &BlobstoreKey("key".to_string()),
            )
            .await?;

        // This claims new request from queue and makes it inprogress
        let req = queue
            .claim_and_get_new_request(&ctx, &ClaimedBy("me".to_string()), Some(&[repo_id]))
            .await?;
        assert!(req.is_some());

        tokio::time::sleep(Duration::from_secs(3)).await;

        let now = Timestamp::now();
        let abandoned_timestamp = Timestamp::from_timestamp_secs(now.timestamp_seconds() - 1);
        // Search in any repo
        let abandoned = queue
            .find_abandoned_requests(&ctx, None, abandoned_timestamp)
            .await?;
        assert_eq!(abandoned.len(), 1);
        assert_eq!(abandoned[0].0, id);

        // Search in the wrong repo
        let now = Timestamp::now();
        let abandoned_timestamp = Timestamp::from_timestamp_secs(now.timestamp_seconds() - 1);
        let abandoned = queue
            .find_abandoned_requests(&ctx, Some(&[RepositoryId::new(1)]), abandoned_timestamp)
            .await?;
        assert_eq!(abandoned.len(), 1);
        assert_eq!(abandoned[0].0, id);

        // Search in a set of repos
        let now = Timestamp::now();
        let abandoned_timestamp = Timestamp::from_timestamp_secs(now.timestamp_seconds() - 1);
        let abandoned = queue
            .find_abandoned_requests(
                &ctx,
                Some(&[
                    RepositoryId::new(1),
                    RepositoryId::new(2),
                    RepositoryId::new(5),
                ]),
                abandoned_timestamp,
            )
            .await?;
        assert_eq!(abandoned.len(), 1);
        assert_eq!(abandoned[0].0, id);

        // Now update timestamp of the request, and check that it's not considered
        // abandoned anymore
        let updated = queue
            .update_in_progress_timestamp(&ctx, &abandoned[0])
            .await?;
        assert!(updated);
        assert_eq!(
            queue
                .find_abandoned_requests(&ctx, None, abandoned_timestamp)
                .await?,
            vec![]
        );

        // Now mark ready first, then make sure that update_in_progress_timestamp does nothing
        assert!(
            queue
                .mark_ready(&ctx, &abandoned[0], BlobstoreKey("key".to_string()))
                .await?
        );
        assert!(
            !queue
                .update_in_progress_timestamp(&ctx, &abandoned[0])
                .await?
        );

        // And also check that marking as new does nothing on a completed request
        tokio::time::sleep(Duration::from_secs(3)).await;
        let now = Timestamp::now();
        let abandoned_timestamp = Timestamp::from_timestamp_secs(now.timestamp_seconds() - 1);
        assert!(
            !queue
                .mark_abandoned_request_as_new(&ctx, abandoned[0].clone(), abandoned_timestamp)
                .await?
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_mark_as_new(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let queue = SqlLongRunningRequestsQueue::with_sqlite_in_memory()?;
        let repo_id = RepositoryId::new(0);
        let id = queue
            .add_request(
                &ctx,
                &RequestType("type".to_string()),
                Some(&repo_id),
                &BlobstoreKey("key".to_string()),
            )
            .await?;

        // This claims new request from queue and makes it inprogress
        let req = queue
            .claim_and_get_new_request(&ctx, &ClaimedBy("me".to_string()), Some(&[repo_id]))
            .await?;
        assert!(req.is_some());

        tokio::time::sleep(Duration::from_secs(3)).await;
        let now = Timestamp::now();
        let abandoned_timestamp = Timestamp::from_timestamp_secs(now.timestamp_seconds() - 1);
        let abandoned = queue
            .find_abandoned_requests(&ctx, Some(&[repo_id]), abandoned_timestamp)
            .await?;
        assert_eq!(abandoned.len(), 1);
        assert_eq!(abandoned[0].0, id);

        let res = queue
            .mark_abandoned_request_as_new(&ctx, abandoned[0].clone(), abandoned_timestamp)
            .await?;
        assert!(res);

        let request = queue.test_get_request_entry_by_id(&ctx, &id).await?;
        assert!(request.is_some());
        let request = request.unwrap();
        assert_eq!(request.status, RequestStatus::New);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_get_stats(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let queue = SqlLongRunningRequestsQueue::with_sqlite_in_memory()?;
        let repo_id = RepositoryId::new(0);
        let now = Timestamp::now();
        let _ = queue
            .add_request(
                &ctx,
                &RequestType("type".to_string()),
                Some(&repo_id),
                &BlobstoreKey("key".to_string()),
            )
            .await?;

        let stats = queue.get_queue_stats(&ctx, Some(&[repo_id])).await?;
        assert_eq!(stats.queue_length_by_status.len(), 1);
        let entry = &stats.queue_length_by_status[0];
        assert_eq!(entry.0, RequestStatus::New);
        assert_eq!(entry.1, 1);

        assert_eq!(stats.queue_age_by_status.len(), 1);
        let entry = &stats.queue_age_by_status[0];
        assert_eq!(entry.0, RequestStatus::New);
        assert!((entry.1.since_seconds() - now.since_seconds()) < 1);

        // This claims new request from queue and makes it inprogress
        let now = Timestamp::now();
        let req = queue
            .claim_and_get_new_request(&ctx, &ClaimedBy("me".to_string()), Some(&[repo_id]))
            .await?;
        assert!(req.is_some());

        tokio::time::sleep(Duration::from_secs(3)).await;

        let stats = queue.get_queue_stats(&ctx, Some(&[repo_id])).await?;
        assert_eq!(stats.queue_length_by_status.len(), 1);
        let entry = &stats.queue_length_by_status[0];
        assert_eq!(entry.0, RequestStatus::InProgress);
        assert_eq!(entry.1, 1);

        assert_eq!(stats.queue_age_by_status.len(), 1);
        let entry = &stats.queue_age_by_status[0];
        assert_eq!(entry.0, RequestStatus::InProgress);
        assert!((entry.1.since_seconds() - now.since_seconds()) < 1);

        assert_eq!(stats.queue_length_by_repo_and_status.len(), 1);
        let entry = &stats.queue_length_by_repo_and_status[0];
        assert_eq!(entry.0.repo_id.unwrap(), repo_id);
        assert_eq!(entry.0.status, RequestStatus::InProgress);
        assert_eq!(entry.1, 1);

        assert_eq!(stats.queue_age_by_repo_and_status.len(), 1);
        let entry = &stats.queue_age_by_repo_and_status[0];
        assert_eq!(entry.0.repo_id.unwrap(), repo_id);
        assert_eq!(entry.0.status, RequestStatus::InProgress);
        assert!((entry.1.since_seconds() - now.since_seconds()) < 1);

        Ok(())
    }
}
