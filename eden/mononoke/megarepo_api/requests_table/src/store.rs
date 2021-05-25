/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{bail, Result};
use async_trait::async_trait;
use bookmarks::BookmarkName;
use context::CoreContext;
use mononoke_types::{RepositoryId, Timestamp};
use sql::queries;
use sql_construct::{SqlConstruct, SqlConstructFromMetadataDatabaseConfig};
use sql_ext::SqlConnections;

use crate::LongRunningRequestsQueue;
use crate::{
    BlobstoreKey, ClaimedBy, LongRunningRequestEntry, RequestId, RequestStatus, RequestType, RowId,
};

queries! {
    read TestGetRequest(id: RowId) -> (
        RequestType,
        RepositoryId,
        BookmarkName,
        BlobstoreKey,
        Option<BlobstoreKey>,
        Timestamp,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        RequestStatus,
        Option<ClaimedBy>,
    ) {
        "SELECT request_type,
            repo_id,
            bookmark,
            args_blobstore_key,
            result_blobstore_key,
            created_at,
            started_processing_at,
            ready_at,
            polled_at,
            status,
            claimed_by
        FROM long_running_request_queue
        WHERE id = {id}"
    }

    read GetRequest(id: RowId, request_type: RequestType) -> (
        RepositoryId,
        BookmarkName,
        BlobstoreKey,
        Option<BlobstoreKey>,
        Timestamp,
        Option<Timestamp>,
        Option<Timestamp>,
        Option<Timestamp>,
        RequestStatus,
        Option<ClaimedBy>,
    ) {
        "SELECT repo_id,
            bookmark,
            args_blobstore_key,
            result_blobstore_key,
            created_at,
            started_processing_at,
            ready_at,
            polled_at,
            status,
            claimed_by
        FROM long_running_request_queue
        WHERE id = {id} AND request_type = {request_type}"
    }

    write AddRequest(request_type: RequestType, repo_id: RepositoryId, bookmark: BookmarkName, args_blobstore_key: BlobstoreKey, created_at: Timestamp) {
        none,
        "INSERT INTO long_running_request_queue
         (request_type, repo_id, bookmark, args_blobstore_key, status, created_at)
         VALUES ({request_type}, {repo_id}, {bookmark}, {args_blobstore_key}, 'new', {created_at})
        "
    }

    write MarkRequestAsNewAgain(id: RowId, request_type: RequestType) {
        none,
        "UPDATE long_running_request_queue
         SET status = 'new', claimed_by = NULL
         WHERE id = {id} AND request_type = {request_type} AND status = 'inprogress'
        "
    }

    write MarkRequestInProgress(id: RowId, request_type: RequestType, started_processing_at: Timestamp, claimed_by: ClaimedBy) {
        none,
        "UPDATE long_running_request_queue
         SET started_processing_at = {started_processing_at}, status = 'inprogress', claimed_by = {claimed_by}
         WHERE id = {id} AND request_type = {request_type} AND status = 'new'
        "
    }

    write MarkRequestReady(id: RowId, request_type: RequestType, ready_at: Timestamp, result_blobstore_key: BlobstoreKey) {
        none,
        "UPDATE long_running_request_queue
         SET ready_at = {ready_at}, status = 'ready', result_blobstore_key = {result_blobstore_key}
         WHERE id = {id} AND request_type = {request_type} AND status = 'inprogress'
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
        bookmark: &BookmarkName,
        args_blobstore_key: &BlobstoreKey,
    ) -> Result<RowId> {
        let res = AddRequest::query(
            &self.connections.write_connection,
            request_type,
            &repo_id,
            &bookmark,
            args_blobstore_key,
            &Timestamp::now(),
        )
        .await?;

        match res.last_insert_id() {
            Some(last_insert_id) if res.affected_rows() == 1 => Ok(RowId(last_insert_id)),
            _ => bail!("Failed to insert a new request of type {}", request_type),
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
            Some(row) => {
                let (
                    request_type,
                    repo_id,
                    bookmark,
                    args_blobstore_key,
                    result_blobstore_key,
                    created_at,
                    started_processing_at,
                    ready_at,
                    polled_at,
                    status,
                    claimed_by,
                ) = row;
                Ok(Some(LongRunningRequestEntry {
                    id: *id,
                    repo_id,
                    bookmark,
                    request_type,
                    args_blobstore_key,
                    result_blobstore_key,
                    created_at,
                    started_processing_at,
                    ready_at,
                    polled_at,
                    status,
                    claimed_by,
                }))
            }
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
            &claimed_by,
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

    async fn test_mark(
        &self,
        _ctx: &CoreContext,
        row_id: &RowId,
        status: RequestStatus,
    ) -> Result<bool> {
        let res = TestMark::query(&self.connections.write_connection, &row_id, &status).await?;
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
                let (
                    repo_id,
                    bookmark,
                    args_blobstore_key,
                    result_blobstore_key,
                    created_at,
                    started_processing_at,
                    ready_at,
                    polled_at,
                    status,
                    claimed_by,
                ) = row;

                match status {
                    RequestStatus::Ready | RequestStatus::Polled
                        if result_blobstore_key.is_none() =>
                    {
                        bail!("no result stored for {:?} request {:?}", status, req_id);
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

                        Some((
                            true,
                            LongRunningRequestEntry {
                                id: req_id.0,
                                repo_id,
                                bookmark,
                                request_type: req_id.1.clone(),
                                args_blobstore_key,
                                result_blobstore_key,
                                created_at,
                                started_processing_at,
                                ready_at,
                                polled_at,
                                status: RequestStatus::Polled,
                                claimed_by,
                            },
                        ))
                    }
                    RequestStatus::Polled => Some((
                        false,
                        LongRunningRequestEntry {
                            id: req_id.0,
                            repo_id,
                            bookmark,
                            request_type: req_id.1.clone(),
                            args_blobstore_key,
                            result_blobstore_key,
                            created_at,
                            started_processing_at,
                            ready_at,
                            polled_at,
                            status: RequestStatus::Polled,
                            claimed_by: None,
                        },
                    )),
                    _ => None,
                }
            }
        };
        txn.commit().await?;
        Ok(entry)
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
