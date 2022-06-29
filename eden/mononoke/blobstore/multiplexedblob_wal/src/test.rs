/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use blobstore::BlobstorePutOps;
use blobstore_sync_queue::SqlBlobstoreWal;
use blobstore_test_utils::Tickable;
use fbinit::FacebookInit;
use metaconfig_types::BlobstoreId;
use metaconfig_types::MultiplexId;
use sql_construct::SqlConstruct;
use std::sync::Arc;

use crate::WalMultiplexedBlobstore;

#[fbinit::test]
async fn test_quorum_is_valid(_fb: FacebookInit) -> Result<()> {
    let wal = Arc::new(SqlBlobstoreWal::with_sqlite_in_memory()?);

    // Check the quorum cannot be zero
    {
        // no main-stores, no write-mostly
        let quorum = 0;
        let result =
            WalMultiplexedBlobstore::new(MultiplexId::new(0), wal.clone(), vec![], vec![], quorum);

        assert!(result.is_err());
    }

    // Check creating multiplex fails if there are no enough main blobstores
    {
        let stores = (0..2)
            .map(|id| {
                (
                    BlobstoreId::new(id),
                    Arc::new(Tickable::new()) as Arc<dyn BlobstorePutOps>,
                )
            })
            .collect();
        // write-mostly don't count into the quorum
        let write_mostly = (2..4)
            .map(|id| {
                (
                    BlobstoreId::new(id),
                    Arc::new(Tickable::new()) as Arc<dyn BlobstorePutOps>,
                )
            })
            .collect();
        let quorum = 3;
        let result = WalMultiplexedBlobstore::new(
            MultiplexId::new(0),
            wal.clone(),
            stores,
            write_mostly,
            quorum,
        );

        assert!(result.is_err());
    }

    // Check creating multiplex succeeds with the same amount of stores as the quorum
    {
        let stores = (0..3)
            .map(|id| {
                (
                    BlobstoreId::new(id),
                    Arc::new(Tickable::new()) as Arc<dyn BlobstorePutOps>,
                )
            })
            .collect();
        // no write-mostly
        let quorum = 3;
        let result = WalMultiplexedBlobstore::new(MultiplexId::new(0), wal, stores, vec![], quorum);

        assert!(result.is_ok());
    }

    Ok(())
}
