/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use blobstore::BlobstoreGetData;
use blobstore_stats::OperationType;
use context::CoreContext;
use futures::stream::StreamExt;
use multiplexedblob::base::ErrorKind;
use multiplexedblob::ScrubWriteMostly;

use crate::multiplex;
use crate::WalMultiplexedBlobstore;

impl WalMultiplexedBlobstore {
    #[allow(dead_code)]
    async fn scrub_get(
        &self,
        ctx: &CoreContext,
        key: &str,
        write_mostly: ScrubWriteMostly,
    ) -> Result<Option<BlobstoreGetData>, ErrorKind> {
        let mut scuba = self.scuba.clone();
        scuba.sampled();

        let results = multiplexedblob::base::scrub_get_results(
            || {
                multiplex::inner_multi_get(
                    ctx,
                    self.blobstores.clone(),
                    key,
                    OperationType::ScrubGet,
                    &scuba,
                )
                .collect::<Vec<_>>()
            },
            || {
                multiplex::inner_multi_get(
                    ctx,
                    self.write_mostly_blobstores.clone(),
                    key,
                    OperationType::ScrubGet,
                    &scuba,
                )
                .collect::<Vec<_>>()
            },
            self.write_mostly_blobstores.iter().map(|b| *b.id()),
            write_mostly,
        )
        .await;

        multiplexedblob::base::scrub_parse_results(results, self.blobstores.iter().map(|b| *b.id()))
    }
}
