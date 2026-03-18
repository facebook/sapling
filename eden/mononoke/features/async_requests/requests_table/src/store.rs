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
        Option<RowId>,
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
            failed_at,
            root_request_id
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
        Option<RowId>,
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
            failed_at,
            root_request_id
        FROM long_running_request_queue
        WHERE id = {id} AND request_type = {request_type}"
    }

    read GetOneNewRequestForGlobalQueueWithDeps() -> (
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
        Option<RowId>,
    ) {
        "SELECT
           q.id,
           q.request_type,
           q.repo_id,
           q.args_blobstore_key,
           q.result_blobstore_key,
           q.created_at,
           q.started_processing_at,
           q.inprogress_last_updated_at,
           q.ready_at,
           q.polled_at,
           q.status,
           q.claimed_by,
           q.num_retries,
           q.failed_at,
           q.root_request_id
         FROM long_running_request_queue q
         WHERE q.status = 'new'
           AND q.repo_id IS NULL
           AND NOT EXISTS (
             SELECT 1 FROM long_running_request_dependencies dep
             JOIN long_running_request_queue parent
               ON dep.depends_on_request_id = parent.id
             WHERE dep.request_id = q.id
               AND parent.status NOT IN ('ready', 'polled')
            )
          ORDER BY q.created_at ASC
          LIMIT 1
        "
    }

    read GetOneNewRequestForReposWithDeps(>list supported_repo_ids: RepositoryId) -> (
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
        Option<RowId>,
    ) {
        "SELECT
           q.id,
           q.request_type,
           q.repo_id,
           q.args_blobstore_key,
           q.result_blobstore_key,
           q.created_at,
           q.started_processing_at,
           q.inprogress_last_updated_at,
           q.ready_at,
           q.polled_at,
           q.status,
           q.claimed_by,
           q.num_retries,
           q.failed_at,
           q.root_request_id
         FROM long_running_request_queue q
         WHERE q.status = 'new'
           AND q.repo_id IN {supported_repo_ids}
           AND NOT EXISTS (
             SELECT 1 FROM long_running_request_dependencies dep
             JOIN long_running_request_queue parent
               ON dep.depends_on_request_id = parent.id
             WHERE dep.request_id = q.id
               AND parent.status NOT IN ('ready', 'polled')
            )
          ORDER BY q.created_at ASC
          LIMIT 1
        "
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

    write AddRequestWithRepoAndRoot(request_type: RequestType, repo_id: RepositoryId, args_blobstore_key: BlobstoreKey, created_at: Timestamp, root_request_id: RowId) {
        none,
        "INSERT INTO long_running_request_queue
         (request_type, repo_id, args_blobstore_key, status, created_at, root_request_id)
         VALUES ({request_type}, {repo_id}, {args_blobstore_key}, 'new', {created_at}, {root_request_id})
        "
    }

    write AddRequestWithRoot(request_type: RequestType, args_blobstore_key: BlobstoreKey, created_at: Timestamp, root_request_id: RowId) {
        none,
        "INSERT INTO long_running_request_queue
         (request_type, args_blobstore_key, status, created_at, root_request_id)
         VALUES ({request_type}, {args_blobstore_key}, 'new', {created_at}, {root_request_id})
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
        Option<RowId>,
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
            failed_at,
            root_request_id
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
            failed_at,
            root_request_id
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
        Option<RowId>,
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
            failed_at,
            root_request_id
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
            failed_at,
            root_request_id
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

    // Variants excluding derived data backfill request types
    read GetQueueLengthForReposExcludingBackfill(>list repo_ids: RepositoryId) -> (
        RequestStatus, u64
    ) {
        "SELECT status, count(*) FROM long_running_request_queue
        WHERE repo_id IN {repo_ids}
        AND request_type NOT IN ('derive_boundaries', 'derive_slice', 'derive_backfill', 'derive_backfill_repo')
        GROUP BY status"
    }

    read GetQueueLengthByRepoForReposExcludingBackfill(>list repo_ids: RepositoryId) -> (
        Option<RepositoryId>, RequestStatus, u64
    ) {
        "SELECT repo_id, status, count(*) FROM long_running_request_queue
        WHERE repo_id IN {repo_ids}
        AND request_type NOT IN ('derive_boundaries', 'derive_slice', 'derive_backfill', 'derive_backfill_repo')
        GROUP BY repo_id, status"
    }

    read GetQueueLengthForAllReposExcludingBackfill() -> (
        RequestStatus, u64
    ) {
        "SELECT status, count(*) FROM long_running_request_queue
        WHERE request_type NOT IN ('derive_boundaries', 'derive_slice', 'derive_backfill', 'derive_backfill_repo')
        GROUP BY status"
    }

    read GetQueueLengthByRepoForAllReposExcludingBackfill() -> (
        Option<RepositoryId>, RequestStatus, u64
    ) {
        "SELECT repo_id, status, count(*) FROM long_running_request_queue
        WHERE request_type NOT IN ('derive_boundaries', 'derive_slice', 'derive_backfill', 'derive_backfill_repo')
        GROUP BY repo_id, status"
    }

    read GetQueueAgeForReposExcludingBackfill(>list repo_ids: RepositoryId) -> (
        RequestStatus, u64, Option<u64>, Option<u64>
    ) {
        "SELECT status, min(created_at), min(inprogress_last_updated_at), min(ready_at)
        FROM long_running_request_queue
        WHERE repo_id IN {repo_ids}
        AND status NOT IN ('polled', 'failed')
        AND request_type NOT IN ('derive_boundaries', 'derive_slice', 'derive_backfill', 'derive_backfill_repo')
        GROUP BY status
        "
    }

    read GetQueueAgeByRepoForReposExcludingBackfill(>list repo_ids: RepositoryId) -> (
        Option<RepositoryId>, RequestStatus, u64, Option<u64>, Option<u64>
    ) {
        "SELECT repo_id, status, min(created_at), min(inprogress_last_updated_at), min(ready_at)
        FROM long_running_request_queue
        WHERE repo_id IN {repo_ids}
        AND status NOT IN ('polled', 'failed')
        AND request_type NOT IN ('derive_boundaries', 'derive_slice', 'derive_backfill', 'derive_backfill_repo')
        GROUP BY repo_id, status
        "
    }

    read GetQueueAgeForAllReposExcludingBackfill() -> (
        RequestStatus, u64, Option<u64>, Option<u64>
    ) {
        "SELECT status, min(created_at), min(inprogress_last_updated_at), min(ready_at)
        FROM long_running_request_queue
        WHERE status NOT IN ('polled', 'failed')
        AND request_type NOT IN ('derive_boundaries', 'derive_slice', 'derive_backfill', 'derive_backfill_repo')
        GROUP BY status
        "
    }

    read GetQueueAgeByRepoForAllReposExcludingBackfill() -> (
        Option<RepositoryId>, RequestStatus, u64, Option<u64>, Option<u64>
    ) {
        "SELECT repo_id, status, min(created_at), min(inprogress_last_updated_at), min(ready_at)
        FROM long_running_request_queue
        WHERE status NOT IN ('polled', 'failed')
        AND request_type NOT IN ('derive_boundaries', 'derive_slice', 'derive_backfill', 'derive_backfill_repo')
        GROUP BY repo_id, status
        "
    }

    write AddDependency(
        request_id: RowId,
        depends_on_request_id: RowId,
    ) {
        none,
        "INSERT INTO long_running_request_dependencies
         (request_id, depends_on_request_id)
         VALUES ({request_id}, {depends_on_request_id})
        "
    }

    read GetDependencies(request_id: RowId) -> (RowId,) {
        "SELECT depends_on_request_id
         FROM long_running_request_dependencies
         WHERE request_id = {request_id}
        "
    }

    write FailRequestWithCascade(request_id: RowId, failed_at: Timestamp) {
        none,
        "WITH RECURSIVE to_fail(id) AS (
             SELECT {request_id}
             UNION
             SELECT dep.request_id FROM long_running_request_dependencies dep
             JOIN to_fail tf ON dep.depends_on_request_id = tf.id
         )
         UPDATE long_running_request_queue
         SET status = 'failed', failed_at = {failed_at}
         WHERE id IN (SELECT id FROM to_fail)
           AND status IN ('new', 'inprogress')
        "
    }

    read CountInProgressByTypes(>list request_types: RequestType) -> (i64,) {
        mysql("
            SELECT COUNT(*)
            FROM long_running_request_queue
            WHERE status = 'inprogress'
              AND request_type IN {request_types}
        ")
        sqlite("
            SELECT COUNT(*)
            FROM long_running_request_queue
            WHERE status = 'inprogress'
              AND request_type IN {request_types}
        ")
    }

    read GetRequestsByRootRequestId(root_request_id: RowId) -> (
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
        Option<RowId>,
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
            failed_at,
            root_request_id
        FROM long_running_request_queue
        WHERE root_request_id = {root_request_id}"
    }

    write FailNewRequestsByRootId(root_request_id: RowId, failed_at: Timestamp) {
        none,
        "UPDATE long_running_request_queue
         SET status = 'failed', failed_at = {failed_at}
         WHERE root_request_id = {root_request_id} AND status = 'new'"
    }

    read GetBackfillStatsByStatus(root_request_id: RowId) -> (
        RequestType,
        RequestStatus,
        i64,
    ) {
        "SELECT request_type, status, COUNT(*) as count
         FROM long_running_request_queue
         WHERE root_request_id = {root_request_id}
         GROUP BY request_type, status"
    }

    read GetBackfillStatsByRepo(root_request_id: RowId) -> (
        Option<RepositoryId>,
        RequestStatus,
        i64,
    ) {
        "SELECT repo_id, status, COUNT(*) as count
         FROM long_running_request_queue
         WHERE root_request_id = {root_request_id}
           AND repo_id IS NOT NULL
         GROUP BY repo_id, status"
    }

    read GetBackfillTimingStats(root_request_id: RowId) -> (
        i64,
        Option<f64>,
        Option<Timestamp>,
        Option<Timestamp>,
    ) {
        mysql("SELECT
           COUNT(*) as total_completed,
           AVG(TIMESTAMPDIFF(SECOND, COALESCE(started_processing_at, created_at), ready_at)) as avg_duration_seconds,
           MIN(created_at) as min_created_at,
           MAX(ready_at) as max_ready_at
         FROM long_running_request_queue
         WHERE root_request_id = {root_request_id}
           AND status IN ('ready', 'polled')
           AND ready_at IS NOT NULL")

        sqlite("SELECT
           COUNT(*) as total_completed,
           AVG(ready_at - COALESCE(started_processing_at, created_at)) as avg_duration_seconds,
           MIN(created_at) as min_created_at,
           MAX(ready_at) as max_ready_at
         FROM long_running_request_queue
         WHERE root_request_id = {root_request_id}
           AND status IN ('ready', 'polled')
           AND ready_at IS NOT NULL")
    }

    read ListRecentBackfillsWithRepoCount(min_created_at: Timestamp) -> (
        RowId,
        Timestamp,
        RequestStatus,
        i64,
    ) {
        "SELECT root.id, root.created_at, root.status, COUNT(DISTINCT sub.repo_id) as repo_count
         FROM long_running_request_queue root
         LEFT JOIN long_running_request_queue sub ON sub.root_request_id = root.id
         WHERE root.request_type = 'derive_backfill'
           AND root.root_request_id IS NULL
           AND root.created_at >= {min_created_at}
         GROUP BY root.id, root.created_at, root.status
         ORDER BY root.created_at DESC"
    }

    read GetBackfillRepoStats(root_request_id: RowId, repo_id: RepositoryId) -> (
        RequestType,
        RequestStatus,
        i64,
    ) {
        "SELECT request_type, status, COUNT(*) as count
         FROM long_running_request_queue
         WHERE root_request_id = {root_request_id}
           AND repo_id = {repo_id}
         GROUP BY request_type, status"
    }

    read GetBackfillRootEntry(id: RowId) -> (
        RowId,
        RequestType,
        RequestStatus,
        Timestamp,
        BlobstoreKey,
    ) {
        "SELECT id, request_type, status, created_at, args_blobstore_key
         FROM long_running_request_queue
         WHERE id = {id}
           AND request_type = 'derive_backfill'
           AND root_request_id IS NULL"
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
        Option<RowId>,
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
        root_request_id,
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
        root_request_id,
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
            let txn = self
                .connections
                .write_connection
                .start_transaction(ctx.sql_query_telemetry())
                .await?;

            let (txn, rows) = match supported_repos {
                Some(repos) => {
                    GetOneNewRequestForReposWithDeps::query_with_transaction(txn, repos).await
                }
                None => GetOneNewRequestForGlobalQueueWithDeps::query_with_transaction(txn).await,
            }
            .context("claiming new request")?;
            let mut entry = match rows.into_iter().next() {
                None => {
                    txn.rollback().await?;
                    return Ok(None);
                }
                Some(row) => row_to_entry(row),
            };
            let now = Timestamp::now();
            let (txn, res) = MarkRequestInProgress::query_with_transaction(
                txn,
                &entry.id,
                &entry.request_type,
                &now,
                claimed_by,
            )
            .await?;
            if res.affected_rows() > 0 {
                txn.commit().await?;
                entry.status = RequestStatus::InProgress;
                return Ok(Some(entry));
            }
            // Another worker claimed it between our SELECT and UPDATE, retry.
            txn.rollback().await?;
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
        exclude_backfill: bool,
    ) -> Result<QueueStats> {
        Ok(QueueStats {
            queue_length_by_status: get_queue_length(
                ctx,
                &self.connections.read_connection,
                repo_ids,
                exclude_backfill,
            )
            .await?,
            queue_age_by_status: get_queue_age(
                ctx,
                &self.connections.read_connection,
                repo_ids,
                exclude_backfill,
            )
            .await?,
            queue_length_by_repo_and_status: get_queue_length_by_repo(
                ctx,
                &self.connections.read_connection,
                repo_ids,
                exclude_backfill,
            )
            .await?,
            queue_age_by_repo_and_status: get_queue_age_by_repo(
                ctx,
                &self.connections.read_connection,
                repo_ids,
                exclude_backfill,
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
                            txn = FailRequestWithCascade::query_with_transaction(
                                txn,
                                &req_id.0,
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

    async fn add_request_with_dependencies(
        &self,
        ctx: &CoreContext,
        request_type: &RequestType,
        repo_id: Option<&RepositoryId>,
        args_blobstore_key: &BlobstoreKey,
        depends_on: &[RowId],
    ) -> Result<RowId> {
        let txn = self
            .connections
            .write_connection
            .start_transaction(ctx.sql_query_telemetry())
            .await?;

        let now = Timestamp::now();
        let (mut txn, res) = match &repo_id {
            Some(repo_id) => {
                AddRequestWithRepo::query_with_transaction(
                    txn,
                    request_type,
                    repo_id,
                    args_blobstore_key,
                    &now,
                )
                .await?
            }
            None => {
                AddRequest::query_with_transaction(txn, request_type, args_blobstore_key, &now)
                    .await?
            }
        };

        let row_id = match res.last_insert_id() {
            Some(last_insert_id) if res.affected_rows() == 1 => RowId(last_insert_id),
            _ => bail!("Failed to insert a new request of type {}", request_type),
        };

        for dep_id in depends_on {
            txn = AddDependency::query_with_transaction(txn, &row_id, dep_id)
                .await
                .with_context(|| format!("adding dependency {:?} to request {:?}", dep_id, row_id))?
                .0;
        }

        txn.commit().await?;

        Ok(row_id)
    }

    async fn get_dependencies(&self, ctx: &CoreContext, request_id: &RowId) -> Result<Vec<RowId>> {
        let rows = GetDependencies::query(
            &self.connections.read_connection,
            ctx.sql_query_telemetry(),
            request_id,
        )
        .await
        .context("getting dependencies")?;

        Ok(rows.into_iter().map(|(dep_id,)| dep_id).collect())
    }

    async fn mark_failed_with_cascade(&self, ctx: &CoreContext, req_id: &RowId) -> Result<bool> {
        let now = Timestamp::now();
        let res = FailRequestWithCascade::query(
            &self.connections.write_connection,
            ctx.sql_query_telemetry(),
            req_id,
            &now,
        )
        .await
        .context("marking request and dependents as failed")?;
        Ok(res.affected_rows() > 0)
    }

    async fn count_inprogress_by_types(
        &self,
        ctx: &CoreContext,
        request_types: &[&str],
    ) -> Result<i64> {
        let types: Vec<RequestType> = request_types
            .iter()
            .map(|t| RequestType(t.to_string()))
            .collect();
        let rows = CountInProgressByTypes::query(
            &self.connections.read_connection,
            ctx.sql_query_telemetry(),
            &types[..],
        )
        .await?;
        Ok(rows.first().map(|(count,)| *count).unwrap_or(0))
    }

    async fn add_request_with_root(
        &self,
        ctx: &CoreContext,
        request_type: &RequestType,
        repo_id: Option<&RepositoryId>,
        args_blobstore_key: &BlobstoreKey,
        root_request_id: &RowId,
    ) -> Result<RowId> {
        let now = Timestamp::now();
        let res = match &repo_id {
            Some(repo_id) => {
                AddRequestWithRepoAndRoot::query(
                    &self.connections.write_connection,
                    ctx.sql_query_telemetry(),
                    request_type,
                    repo_id,
                    args_blobstore_key,
                    &now,
                    root_request_id,
                )
                .await?
            }
            None => {
                AddRequestWithRoot::query(
                    &self.connections.write_connection,
                    ctx.sql_query_telemetry(),
                    request_type,
                    args_blobstore_key,
                    &now,
                    root_request_id,
                )
                .await?
            }
        };

        match res.last_insert_id() {
            Some(last_insert_id) if res.affected_rows() == 1 => Ok(RowId(last_insert_id)),
            _ => bail!("Failed to insert a new request of type {}", request_type),
        }
    }

    async fn add_request_with_dependencies_and_root(
        &self,
        ctx: &CoreContext,
        request_type: &RequestType,
        repo_id: Option<&RepositoryId>,
        args_blobstore_key: &BlobstoreKey,
        depends_on: &[RowId],
        root_request_id: &RowId,
    ) -> Result<RowId> {
        let txn = self
            .connections
            .write_connection
            .start_transaction(ctx.sql_query_telemetry())
            .await?;

        let now = Timestamp::now();
        let (mut txn, res) = match &repo_id {
            Some(repo_id) => {
                AddRequestWithRepoAndRoot::query_with_transaction(
                    txn,
                    request_type,
                    repo_id,
                    args_blobstore_key,
                    &now,
                    root_request_id,
                )
                .await?
            }
            None => {
                AddRequestWithRoot::query_with_transaction(
                    txn,
                    request_type,
                    args_blobstore_key,
                    &now,
                    root_request_id,
                )
                .await?
            }
        };

        let row_id = match res.last_insert_id() {
            Some(last_insert_id) if res.affected_rows() == 1 => RowId(last_insert_id),
            _ => bail!("Failed to insert a new request of type {}", request_type),
        };

        for dep_id in depends_on {
            txn = AddDependency::query_with_transaction(txn, &row_id, dep_id)
                .await
                .with_context(|| format!("adding dependency {:?} to request {:?}", dep_id, row_id))?
                .0;
        }

        txn.commit().await?;

        Ok(row_id)
    }

    async fn get_requests_by_root_id(
        &self,
        ctx: &CoreContext,
        root_request_id: &RowId,
    ) -> Result<Vec<LongRunningRequestEntry>> {
        let rows = GetRequestsByRootRequestId::query(
            &self.connections.read_connection,
            ctx.sql_query_telemetry(),
            root_request_id,
        )
        .await?;
        Ok(rows.into_iter().map(row_to_entry).collect())
    }

    async fn fail_new_requests_by_root_id(
        &self,
        ctx: &CoreContext,
        root_request_id: &RowId,
    ) -> Result<u64> {
        let res = FailNewRequestsByRootId::query(
            &self.connections.write_connection,
            ctx.sql_query_telemetry(),
            root_request_id,
            &Timestamp::now(),
        )
        .await?;
        Ok(res.affected_rows())
    }

    async fn get_backfill_stats(
        &self,
        ctx: &CoreContext,
        root_request_id: &RowId,
        repo_id: Option<&RepositoryId>,
    ) -> Result<Vec<(RequestType, RequestStatus, i64)>> {
        let rows = match repo_id {
            Some(repo_id) => {
                GetBackfillRepoStats::query(
                    &self.connections.read_connection,
                    ctx.sql_query_telemetry(),
                    root_request_id,
                    repo_id,
                )
                .await?
            }
            None => {
                GetBackfillStatsByStatus::query(
                    &self.connections.read_connection,
                    ctx.sql_query_telemetry(),
                    root_request_id,
                )
                .await?
            }
        };
        Ok(rows)
    }

    async fn get_backfill_stats_by_repo(
        &self,
        ctx: &CoreContext,
        root_request_id: &RowId,
    ) -> Result<Vec<(Option<RepositoryId>, RequestStatus, i64)>> {
        let rows = GetBackfillStatsByRepo::query(
            &self.connections.read_connection,
            ctx.sql_query_telemetry(),
            root_request_id,
        )
        .await?;
        Ok(rows)
    }

    async fn get_backfill_timing_stats(
        &self,
        ctx: &CoreContext,
        root_request_id: &RowId,
    ) -> Result<(i64, Option<f64>, Option<Timestamp>, Option<Timestamp>)> {
        let rows = GetBackfillTimingStats::query(
            &self.connections.read_connection,
            ctx.sql_query_telemetry(),
            root_request_id,
        )
        .await?;
        rows.into_iter().next().ok_or_else(|| {
            anyhow::anyhow!(
                "No timing stats found for root_request_id {}",
                root_request_id
            )
        })
    }

    async fn list_recent_backfills_with_repo_count(
        &self,
        ctx: &CoreContext,
        min_created_at: &Timestamp,
    ) -> Result<Vec<(RowId, Timestamp, RequestStatus, i64)>> {
        let rows = ListRecentBackfillsWithRepoCount::query(
            &self.connections.read_connection,
            ctx.sql_query_telemetry(),
            min_created_at,
        )
        .await?;
        Ok(rows)
    }

    async fn get_backfill_root_entry(
        &self,
        ctx: &CoreContext,
        id: &RowId,
    ) -> Result<Option<(RowId, RequestType, RequestStatus, Timestamp, BlobstoreKey)>> {
        let rows = GetBackfillRootEntry::query(
            &self.connections.read_connection,
            ctx.sql_query_telemetry(),
            id,
        )
        .await?;
        Ok(rows.into_iter().next())
    }
}

async fn get_queue_length(
    ctx: &CoreContext,
    conn: &Connection,
    repo_ids: Option<&[RepositoryId]>,
    exclude_backfill: bool,
) -> Result<Vec<(RequestStatus, u64)>> {
    Ok(match (repo_ids, exclude_backfill) {
        (Some(repos), false) => {
            GetQueueLengthForRepos::query(conn, ctx.sql_query_telemetry(), repos).await
        }
        (None, false) => GetQueueLengthForAllRepos::query(conn, ctx.sql_query_telemetry()).await,
        (Some(repos), true) => {
            GetQueueLengthForReposExcludingBackfill::query(conn, ctx.sql_query_telemetry(), repos)
                .await
        }
        (None, true) => {
            GetQueueLengthForAllReposExcludingBackfill::query(conn, ctx.sql_query_telemetry()).await
        }
    }
    .context("fetching queue length stats")?
    .into_iter()
    .collect())
}

async fn get_queue_length_by_repo(
    ctx: &CoreContext,
    conn: &Connection,
    repo_ids: Option<&[RepositoryId]>,
    exclude_backfill: bool,
) -> Result<Vec<(QueueStatsEntry, u64)>> {
    Ok(match (repo_ids, exclude_backfill) {
        (Some(repos), false) => {
            GetQueueLengthByRepoForRepos::query(conn, ctx.sql_query_telemetry(), repos).await
        }
        (None, false) => {
            GetQueueLengthByRepoForAllRepos::query(conn, ctx.sql_query_telemetry()).await
        }
        (Some(repos), true) => {
            GetQueueLengthByRepoForReposExcludingBackfill::query(
                conn,
                ctx.sql_query_telemetry(),
                repos,
            )
            .await
        }
        (None, true) => {
            GetQueueLengthByRepoForAllReposExcludingBackfill::query(conn, ctx.sql_query_telemetry())
                .await
        }
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
    exclude_backfill: bool,
) -> Result<Vec<(RequestStatus, Timestamp)>> {
    Ok(match (repo_ids, exclude_backfill) {
        (Some(repos), false) => {
            GetQueueAgeForRepos::query(conn, ctx.sql_query_telemetry(), repos).await
        }
        (None, false) => GetQueueAgeForAllRepos::query(conn, ctx.sql_query_telemetry()).await,
        (Some(repos), true) => {
            GetQueueAgeForReposExcludingBackfill::query(conn, ctx.sql_query_telemetry(), repos)
                .await
        }
        (None, true) => {
            GetQueueAgeForAllReposExcludingBackfill::query(conn, ctx.sql_query_telemetry()).await
        }
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
    exclude_backfill: bool,
) -> Result<Vec<(QueueStatsEntry, Timestamp)>> {
    Ok(match (repo_ids, exclude_backfill) {
        (Some(repos), false) => {
            GetQueueAgeByRepoForRepos::query(conn, ctx.sql_query_telemetry(), repos).await
        }
        (None, false) => GetQueueAgeByRepoForAllRepos::query(conn, ctx.sql_query_telemetry()).await,
        (Some(repos), true) => {
            GetQueueAgeByRepoForReposExcludingBackfill::query(
                conn,
                ctx.sql_query_telemetry(),
                repos,
            )
            .await
        }
        (None, true) => {
            GetQueueAgeByRepoForAllReposExcludingBackfill::query(conn, ctx.sql_query_telemetry())
                .await
        }
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

    const CREATION_QUERY: &'static str = concat!(
        include_str!("../schemas/sqlite-long_running_requests_queue.sql"),
        include_str!("../schemas/sqlite-long_running_request_dependencies.sql"),
    );

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

        let stats = queue.get_queue_stats(&ctx, Some(&[repo_id]), false).await?;
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

        let stats = queue.get_queue_stats(&ctx, Some(&[repo_id]), false).await?;
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

    #[mononoke::fbinit_test]
    async fn test_dependency_blocks_dequeue(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let queue = SqlLongRunningRequestsQueue::with_sqlite_in_memory()?;
        let repo_id = RepositoryId::new(0);

        // Add a parent request (no dependencies)
        let parent_id = queue
            .add_request(
                &ctx,
                &RequestType("parent_type".to_string()),
                Some(&repo_id),
                &BlobstoreKey("parent_key".to_string()),
            )
            .await?;

        // Add a child request that depends on the parent
        let child_id = queue
            .add_request_with_dependencies(
                &ctx,
                &RequestType("child_type".to_string()),
                Some(&repo_id),
                &BlobstoreKey("child_key".to_string()),
                &[parent_id],
            )
            .await?;

        // Both should be in 'new' status
        let parent_entry = queue
            .test_get_request_entry_by_id(&ctx, &parent_id)
            .await?
            .unwrap();
        assert_eq!(parent_entry.status, RequestStatus::New);
        let child_entry = queue
            .test_get_request_entry_by_id(&ctx, &child_id)
            .await?
            .unwrap();
        assert_eq!(child_entry.status, RequestStatus::New);

        // Dequeue should return the parent (no unmet deps), not the child
        let claimed = queue
            .claim_and_get_new_request(&ctx, &ClaimedBy("test".to_string()), Some(&[repo_id]))
            .await?;
        assert!(claimed.is_some());
        let claimed = claimed.unwrap();
        assert_eq!(claimed.id, parent_id);

        // Dequeue again — child should NOT be dequeued (parent is inprogress, not ready/polled)
        let claimed = queue
            .claim_and_get_new_request(&ctx, &ClaimedBy("test".to_string()), Some(&[repo_id]))
            .await?;
        assert!(claimed.is_none());

        // Complete the parent (mark as ready)
        let parent_req_id = RequestId(parent_id, RequestType("parent_type".to_string()));
        queue
            .mark_ready(&ctx, &parent_req_id, BlobstoreKey("result_key".to_string()))
            .await?;

        // Now dequeue should return the child
        let claimed = queue
            .claim_and_get_new_request(&ctx, &ClaimedBy("test".to_string()), Some(&[repo_id]))
            .await?;
        assert!(claimed.is_some());
        let claimed = claimed.unwrap();
        assert_eq!(claimed.id, child_id);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_failure_cascades_to_dependents(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let queue = SqlLongRunningRequestsQueue::with_sqlite_in_memory()?;
        let repo_id = RepositoryId::new(0);

        // Create parent request
        let parent_id = queue
            .add_request(
                &ctx,
                &RequestType("parent_type".to_string()),
                Some(&repo_id),
                &BlobstoreKey("parent_key".to_string()),
            )
            .await?;

        // Create two children depending on parent
        let child1_id = queue
            .add_request_with_dependencies(
                &ctx,
                &RequestType("child_type".to_string()),
                Some(&repo_id),
                &BlobstoreKey("child1_key".to_string()),
                &[parent_id],
            )
            .await?;

        let child2_id = queue
            .add_request_with_dependencies(
                &ctx,
                &RequestType("child_type".to_string()),
                Some(&repo_id),
                &BlobstoreKey("child2_key".to_string()),
                &[parent_id],
            )
            .await?;

        // Fail the parent with cascade
        let failed = queue.mark_failed_with_cascade(&ctx, &parent_id).await?;
        assert!(failed);

        // Parent should be failed
        let parent_entry = queue
            .test_get_request_entry_by_id(&ctx, &parent_id)
            .await?
            .unwrap();
        assert_eq!(parent_entry.status, RequestStatus::Failed);

        // Both children should also be failed
        let child1_entry = queue
            .test_get_request_entry_by_id(&ctx, &child1_id)
            .await?
            .unwrap();
        assert_eq!(child1_entry.status, RequestStatus::Failed);

        let child2_entry = queue
            .test_get_request_entry_by_id(&ctx, &child2_id)
            .await?
            .unwrap();
        assert_eq!(child2_entry.status, RequestStatus::Failed);

        // Nothing should be dequeueable
        let claimed = queue
            .claim_and_get_new_request(&ctx, &ClaimedBy("test".to_string()), Some(&[repo_id]))
            .await?;
        assert!(claimed.is_none());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_serial_slice_chaining(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let queue = SqlLongRunningRequestsQueue::with_sqlite_in_memory()?;
        let repo_id = RepositoryId::new(0);

        // Create a chain: boundary -> slice1 -> slice2 -> slice3
        let boundary_id = queue
            .add_request(
                &ctx,
                &RequestType("derive_boundaries".to_string()),
                Some(&repo_id),
                &BlobstoreKey("boundary_key".to_string()),
            )
            .await?;

        let slice1_id = queue
            .add_request_with_dependencies(
                &ctx,
                &RequestType("derive_slice".to_string()),
                Some(&repo_id),
                &BlobstoreKey("slice1_key".to_string()),
                &[boundary_id],
            )
            .await?;

        let slice2_id = queue
            .add_request_with_dependencies(
                &ctx,
                &RequestType("derive_slice".to_string()),
                Some(&repo_id),
                &BlobstoreKey("slice2_key".to_string()),
                &[boundary_id, slice1_id],
            )
            .await?;

        let slice3_id = queue
            .add_request_with_dependencies(
                &ctx,
                &RequestType("derive_slice".to_string()),
                Some(&repo_id),
                &BlobstoreKey("slice3_key".to_string()),
                &[boundary_id, slice2_id],
            )
            .await?;

        // Verify dependencies were recorded correctly
        let slice1_deps = queue.get_dependencies(&ctx, &slice1_id).await?;
        assert_eq!(slice1_deps, vec![boundary_id]);

        let slice2_deps = queue.get_dependencies(&ctx, &slice2_id).await?;
        assert_eq!(slice2_deps.len(), 2);
        assert!(slice2_deps.contains(&boundary_id));
        assert!(slice2_deps.contains(&slice1_id));

        let slice3_deps = queue.get_dependencies(&ctx, &slice3_id).await?;
        assert_eq!(slice3_deps.len(), 2);
        assert!(slice3_deps.contains(&boundary_id));
        assert!(slice3_deps.contains(&slice2_id));

        // Only boundary should be dequeueable initially
        let claimed = queue
            .claim_and_get_new_request(&ctx, &ClaimedBy("test".to_string()), Some(&[repo_id]))
            .await?;
        assert!(claimed.is_some());
        assert_eq!(claimed.unwrap().id, boundary_id);

        // No more dequeueable (boundary is inprogress, slices blocked)
        let claimed = queue
            .claim_and_get_new_request(&ctx, &ClaimedBy("test".to_string()), Some(&[repo_id]))
            .await?;
        assert!(claimed.is_none());

        // Complete boundary
        let boundary_req_id = RequestId(boundary_id, RequestType("derive_boundaries".to_string()));
        queue
            .mark_ready(
                &ctx,
                &boundary_req_id,
                BlobstoreKey("boundary_result".to_string()),
            )
            .await?;

        // Now slice1 should be dequeueable (boundary is ready)
        let claimed = queue
            .claim_and_get_new_request(&ctx, &ClaimedBy("test".to_string()), Some(&[repo_id]))
            .await?;
        assert!(claimed.is_some());
        assert_eq!(claimed.unwrap().id, slice1_id);

        // slice2 still blocked (slice1 is inprogress)
        let claimed = queue
            .claim_and_get_new_request(&ctx, &ClaimedBy("test".to_string()), Some(&[repo_id]))
            .await?;
        assert!(claimed.is_none());

        // Complete slice1
        let slice1_req_id = RequestId(slice1_id, RequestType("derive_slice".to_string()));
        queue
            .mark_ready(
                &ctx,
                &slice1_req_id,
                BlobstoreKey("slice1_result".to_string()),
            )
            .await?;

        // Now slice2 should be dequeueable
        let claimed = queue
            .claim_and_get_new_request(&ctx, &ClaimedBy("test".to_string()), Some(&[repo_id]))
            .await?;
        assert!(claimed.is_some());
        assert_eq!(claimed.unwrap().id, slice2_id);

        // Complete slice2
        let slice2_req_id = RequestId(slice2_id, RequestType("derive_slice".to_string()));
        queue
            .mark_ready(
                &ctx,
                &slice2_req_id,
                BlobstoreKey("slice2_result".to_string()),
            )
            .await?;

        // Now slice3 should be dequeueable
        let claimed = queue
            .claim_and_get_new_request(&ctx, &ClaimedBy("test".to_string()), Some(&[repo_id]))
            .await?;
        assert!(claimed.is_some());
        assert_eq!(claimed.unwrap().id, slice3_id);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_failure_cascade_multi_level(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let queue = SqlLongRunningRequestsQueue::with_sqlite_in_memory()?;
        let repo_id = RepositoryId::new(0);

        // Create the chain from backfill_enqueue:
        // Boundary (no deps)
        // Slice1 → [Boundary]
        // Slice2 → [Boundary, Slice1]
        // Slice3 → [Boundary, Slice2]
        let boundary_id = queue
            .add_request(
                &ctx,
                &RequestType("derive_boundaries".to_string()),
                Some(&repo_id),
                &BlobstoreKey("boundary_key".to_string()),
            )
            .await?;

        let slice1_id = queue
            .add_request_with_dependencies(
                &ctx,
                &RequestType("derive_slice".to_string()),
                Some(&repo_id),
                &BlobstoreKey("slice1_key".to_string()),
                &[boundary_id],
            )
            .await?;

        let slice2_id = queue
            .add_request_with_dependencies(
                &ctx,
                &RequestType("derive_slice".to_string()),
                Some(&repo_id),
                &BlobstoreKey("slice2_key".to_string()),
                &[boundary_id, slice1_id],
            )
            .await?;

        let slice3_id = queue
            .add_request_with_dependencies(
                &ctx,
                &RequestType("derive_slice".to_string()),
                Some(&repo_id),
                &BlobstoreKey("slice3_key".to_string()),
                &[boundary_id, slice2_id],
            )
            .await?;

        // Complete boundary so slice1 can be dequeued
        let boundary_req_id = RequestId(boundary_id, RequestType("derive_boundaries".to_string()));
        // Claim boundary first so we can mark it ready (needs inprogress status)
        let claimed = queue
            .claim_and_get_new_request(&ctx, &ClaimedBy("test".to_string()), Some(&[repo_id]))
            .await?;
        assert_eq!(claimed.unwrap().id, boundary_id);
        queue
            .mark_ready(
                &ctx,
                &boundary_req_id,
                BlobstoreKey("boundary_result".to_string()),
            )
            .await?;

        // Claim slice1 and then fail it with cascade
        let claimed = queue
            .claim_and_get_new_request(&ctx, &ClaimedBy("test".to_string()), Some(&[repo_id]))
            .await?;
        assert_eq!(claimed.unwrap().id, slice1_id);

        // Fail slice1 — should cascade to slice2 (direct dependent)
        // AND slice3 (transitive dependent via slice2)
        let failed = queue.mark_failed_with_cascade(&ctx, &slice1_id).await?;
        assert!(failed);

        // Slice1 should be failed
        let entry = queue
            .test_get_request_entry_by_id(&ctx, &slice1_id)
            .await?
            .unwrap();
        assert_eq!(entry.status, RequestStatus::Failed);

        // Slice2 should be failed (direct dependent of slice1)
        let entry = queue
            .test_get_request_entry_by_id(&ctx, &slice2_id)
            .await?
            .unwrap();
        assert_eq!(entry.status, RequestStatus::Failed);

        // Slice3 should ALSO be failed (transitive dependent via slice2)
        let entry = queue
            .test_get_request_entry_by_id(&ctx, &slice3_id)
            .await?
            .unwrap();
        assert_eq!(entry.status, RequestStatus::Failed);

        // Nothing should be dequeueable
        let claimed = queue
            .claim_and_get_new_request(&ctx, &ClaimedBy("test".to_string()), Some(&[repo_id]))
            .await?;
        assert!(claimed.is_none());

        Ok(())
    }
}
