/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use metaconfig_types::MultiplexId;
use mononoke_types::Timestamp;
use sql::queries;

use crate::OperationKey;

queries! {
    pub(crate) write WalInsertEntry(values: (
        blobstore_key: String,
        multiplex_id: MultiplexId,
        timestamp: Timestamp,
        operation_key: OperationKey,
        blob_size: Option<u64>,
    )) {
        none,
        "INSERT INTO blobstore_write_ahead_log (blobstore_key, multiplex_id, timestamp, operation_key, blob_size)
         VALUES {values}"
    }

    // In comparison to the sync-queue, we write blobstore keys to the WAL only once
    // during the `put` operation. This way when the healer reads entries from the WAL,
    // it doesn't need to filter out distinct operation keys and then blobstore keys
    // (because each blobstore key can have multiple appearances with the same and
    // with different operation keys).
    // The healer can just read all the entries older than the timestamp and they will
    // represent a set of different put opertions by design.
    pub(crate) read WalReadEntries(multiplex_id: MultiplexId, older_than: Timestamp, limit: usize) -> (
        String,
        MultiplexId,
        Timestamp,
        OperationKey,
        u64,
        Option<u64>,
    ) {
        "SELECT blobstore_key, multiplex_id, timestamp, operation_key, id, blob_size
         FROM blobstore_write_ahead_log
         WHERE multiplex_id = {multiplex_id} AND timestamp <= {older_than}
         LIMIT {limit}
         "
    }
}
