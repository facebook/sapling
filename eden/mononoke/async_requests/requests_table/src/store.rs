/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkName;
use context::CoreContext;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::mononoke_queries;
use sql_ext::SqlConnections;

use crate::BlobstoreKey;
use crate::ClaimedBy;
use crate::LongRunningRequestEntry;
use crate::LongRunningRequestsQueue;
use crate::RequestId;
use crate::RequestStatus;
use crate::RequestType;
use crate::RowId;

mononoke_queries! {
    read TestGetRequest(id: RowId) -> (
        RowId,
        RequestType,
        RepositoryId,
        BookmarkName,
        BlobstoreKey,
        Option<BlobstoreKey>,
        Timestamp,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        RequestStatus,
        Option<ClaimedBy>,
    ) {
        "SELECT id,
            request_type,
            repo_id,
            bookmark,
            args_blobstore_key,
            result_blobstore_key,
            created_at,
            started_processing_at,
            inprogress_last_updated_at,
            ready_at,
            polled_at,
            status,
            claimed_by
        FROM long_running_request_queue
        WHERE id = {id}"
    }

    read GetRequest(id: RowId, request_type: RequestType) -> (
        RowId,
        RequestType,
        RepositoryId,
        BookmarkName,
        BlobstoreKey,
        Option<BlobstoreKey>,
        Timestamp,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        RequestStatus,
        Option<ClaimedBy>,
    ) {
        "SELECT id,
            request_type,
            repo_id,
            bookmark,
            args_blobstore_key,
            result_blobstore_key,
            created_at,
            started_processing_at,
            inprogress_last_updated_at,
            ready_at,
            polled_at,
            status,
            claimed_by
        FROM long_running_request_queue
        WHERE id = {id} AND request_type = {request_type}"
    }

    read GetOneNewRequestForAnyRepo() -> (
        RowId,
        RequestType,
        RepositoryId,
        BookmarkName,
        BlobstoreKey,
        Option<BlobstoreKey>,
        Timestamp,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        RequestStatus,
        Option<ClaimedBy>,
    ) {
        "SELECT id,
            request_type,
            repo_id,
            bookmark,
            args_blobstore_key,
            result_blobstore_key,
            created_at,
            started_processing_at,
            inprogress_last_updated_at,
            ready_at,
            polled_at,
            status,
            claimed_by
        FROM long_running_request_queue
        WHERE status = 'new'
        ORDER BY created_at ASC
        LIMIT 1
        "
    }

    read GetOneNewRequestForRepos(>list supported_repo_ids: RepositoryId) -> (
        RowId,
        RequestType,
        RepositoryId,
        BookmarkName,
        BlobstoreKey,
        Option<BlobstoreKey>,
        Timestamp,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        RequestStatus,
        Option<ClaimedBy>,
    ) {
        "SELECT id,
            request_type,
            repo_id,
            bookmark,
            args_blobstore_key,
            result_blobstore_key,
            created_at,
            started_processing_at,
            inprogress_last_updated_at,
            ready_at,
            polled_at,
            status,
            claimed_by
        FROM long_running_request_queue
        WHERE status = 'new' AND repo_id IN {supported_repo_ids}
        ORDER BY created_at ASC
        LIMIT 1
        "
    }

    write AddRequest(request_type: RequestType, repo_id: RepositoryId, bookmark: BookmarkName, args_blobstore_key: BlobstoreKey, created_at: Timestamp) {
        none,
        "INSERT INTO long_running_request_queue
         (request_type, repo_id, bookmark, args_blobstore_key, status, created_at)
         VALUES ({request_type}, {repo_id}, {bookmark}, {args_blobstore_key}, 'new', {created_at})
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
        RepositoryId,
        BookmarkName,
        BlobstoreKey,
        Option<BlobstoreKey>,
        Timestamp,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        RequestStatus,
        Option<ClaimedBy>,
    ) {
        "SELECT id,
            request_type,
            repo_id,
            bookmark,
            args_blobstore_key,
            result_blobstore_key,
            created_at,
            started_processing_at,
            inprogress_last_updated_at,
            ready_at,
            polled_at,
            status,
            claimed_by
        FROM long_running_request_queue
        WHERE (
            inprogress_last_updated_at > {last_update_newer_than} OR
            (status = 'new' AND created_at > {last_update_newer_than})
        )"
    }

    read ListRequestsForRepos(last_update_newer_than: Timestamp, >list repo_ids: RepositoryId) -> (
        RowId,
        RequestType,
        RepositoryId,
        BookmarkName,
        BlobstoreKey,
        Option<BlobstoreKey>,
        Timestamp,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        RequestStatus,
        Option<ClaimedBy>,
    ) {
        "SELECT id,
            request_type,
            repo_id,
            bookmark,
            args_blobstore_key,
            result_blobstore_key,
            created_at,
            started_processing_at,
            inprogress_last_updated_at,
            ready_at,
            polled_at,
            status,
            claimed_by
        FROM long_running_request_queue
        WHERE repo_id IN {repo_ids} AND (
            inprogress_last_updated_at > {last_update_newer_than} OR
            (status = 'new' AND created_at > {last_update_newer_than})
        )"
    }
}

fn row_to_entry(
    row: (
        RowId,
        RequestType,
        RepositoryId,
        BookmarkName,
        BlobstoreKey,
        Option<BlobstoreKey>,
        Timestamp,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        RequestStatus,
        Option<ClaimedBy>,
    ),
) -> LongRunningRequestEntry {
    let (
        id,
        request_type,
        repo_id,
        bookmark,
        args_blobstore_key,
        result_blobstore_key,
        created_at,
        started_processing_at,
        inprogress_last_updated_at,
        ready_at,
        polled_at,
        status,
        claimed_by,
    ) = row;
    LongRunningRequestEntry {
        id,
        repo_id,
        bookmark,
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
        _ctx: &CoreContext,
        request_type: &RequestType,
        repo_id: &RepositoryId,
        bookmark: &BookmarkKey,
        args_blobstore_key: &BlobstoreKey,
    ) -> Result<RowId> {
        let res = AddRequest::query(
            &self.connections.write_connection,
            request_type,
            repo_id,
            bookmark.name(),
            args_blobstore_key,
            &Timestamp::now(),
        )
        .await?;

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
                Some(repos) => GetOneNewRequestForRepos::query(connection, repos).await,
                None => GetOneNewRequestForAnyRepo::query(connection).await,
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
        _ctx: &CoreContext,
        id: &RowId,
    ) -> Result<Option<LongRunningRequestEntry>> {
        let rows = TestGetRequest::query(&self.connections.read_connection, id).await?;
        match rows.into_iter().next() {
            None => Ok(None),
            Some(row) => Ok(Some(row_to_entry(row))),
        }
    }

    async fn mark_in_progress(
        &self,
        _ctx: &CoreContext,
        req_id: &RequestId,
        claimed_by: &ClaimedBy,
    ) -> Result<bool> {
        let res = MarkRequestInProgress::query(
            &self.connections.write_connection,
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
        _ctx: &CoreContext,
        req_id: &RequestId,
    ) -> Result<bool> {
        let res = UpdateInProgressTimestamp::query(
            &self.connections.write_connection,
            &req_id.0,
            &req_id.1,
            &Timestamp::now(),
        )
        .await?;
        Ok(res.affected_rows() > 0)
    }

    async fn find_abandoned_requests(
        &self,
        _ctx: &CoreContext,
        repo_ids: Option<&[RepositoryId]>,
        abandoned_timestamp: Timestamp,
    ) -> Result<Vec<RequestId>> {
        let rows = match repo_ids {
            Some(repos) => {
                FindAbandonedRequestsForRepos::query(
                    &self.connections.write_connection,
                    &abandoned_timestamp,
                    repos,
                )
                .await
            }
            None => {
                FindAbandonedRequestsForAnyRepo::query(
                    &self.connections.write_connection,
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
        _ctx: &CoreContext,
        request_id: RequestId,
        abandoned_timestamp: Timestamp,
    ) -> Result<bool> {
        let res = MarkRequestAsNewAgainIfAbandoned::query(
            &self.connections.write_connection,
            &request_id.0,
            &request_id.1,
            &abandoned_timestamp,
        )
        .await?;

        Ok(res.affected_rows() > 0)
    }

    async fn mark_ready(
        &self,
        _ctx: &CoreContext,
        req_id: &RequestId,
        blobstore_result_key: BlobstoreKey,
    ) -> Result<bool> {
        let res = MarkRequestReady::query(
            &self.connections.write_connection,
            &req_id.0,
            &req_id.1,
            &Timestamp::now(),
            &blobstore_result_key,
        )
        .await?;

        Ok(res.affected_rows() > 0)
    }

    async fn mark_new(&self, _ctx: &CoreContext, req_id: &RequestId) -> Result<bool> {
        let res = MarkRequestAsNew::query(&self.connections.write_connection, &req_id.0, &req_id.1)
            .await?;

        Ok(res.affected_rows() > 0)
    }

    async fn test_mark(
        &self,
        _ctx: &CoreContext,
        row_id: &RowId,
        status: RequestStatus,
    ) -> Result<bool> {
        let res = TestMark::query(&self.connections.write_connection, row_id, &status).await?;
        Ok(res.affected_rows() > 0)
    }

    async fn poll(
        &self,
        _ctx: &CoreContext,
        req_id: &RequestId,
    ) -> Result<Option<(bool, LongRunningRequestEntry)>> {
        let txn = self
            .connections
            .write_connection
            .start_transaction()
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
        _ctx: &CoreContext,
        repo_ids: Option<&[RepositoryId]>,
        last_update_newer_than: Option<&Timestamp>,
    ) -> Result<Vec<LongRunningRequestEntry>> {
        let entries = match repo_ids {
            Some(repos) => {
                ListRequestsForRepos::query(
                    &self.connections.read_connection,
                    last_update_newer_than.unwrap_or(&Timestamp::from_timestamp_nanos(0)),
                    repos,
                )
                .await
            }
            None => {
                ListRequestsForAnyRepo::query(
                    &self.connections.read_connection,
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
}

impl SqlConstruct for SqlLongRunningRequestsQueue {
    const LABEL: &'static str = "long_running_requests_queue";

    const CREATION_QUERY: &'static str =
        include_str!("../schemas/sqlite-long_running_requests_queue.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlLongRunningRequestsQueue {}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use fbinit::FacebookInit;
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::fbinit_test]
    async fn test_claim_and_get_new_request(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let queue = SqlLongRunningRequestsQueue::with_sqlite_in_memory()?;
        let id = queue
            .add_request(
                &ctx,
                &RequestType("type".to_string()),
                &RepositoryId::new(0),
                &BookmarkKey::new("book")?,
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
                &RepositoryId::new(0),
                &BookmarkKey::new("book")?,
                &BlobstoreKey("key".to_string()),
            )
            .await?;

        let request = queue.test_get_request_entry_by_id(&ctx, &id).await?;
        assert!(request.is_some());
        let request = request.unwrap();
        assert!(request.inprogress_last_updated_at.is_none());

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
                &RepositoryId::new(0),
                &BookmarkKey::new("book")?,
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
                &repo_id,
                &BookmarkKey::new("book")?,
                &BlobstoreKey("key".to_string()),
            )
            .await?;

        // This claims new request from queue and makes it inprogress
        let req = queue
            .claim_and_get_new_request(&ctx, &ClaimedBy("me".to_string()), None)
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
                &repo_id,
                &BookmarkKey::new("book")?,
                &BlobstoreKey("key".to_string()),
            )
            .await?;

        // This claims new request from queue and makes it inprogress
        let req = queue
            .claim_and_get_new_request(&ctx, &ClaimedBy("me".to_string()), None)
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
}
