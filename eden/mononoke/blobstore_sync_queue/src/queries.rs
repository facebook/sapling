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
}
