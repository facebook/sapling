/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::{format_err, Error};
use blobstore::Blobstore;
use blobstore_factory::{
    make_blobstore, make_blobstore_multiplexed, make_sql_factory, ReadOnlyStorage, SqlFactory,
};
use fbinit::FacebookInit;
use futures::{
    self,
    future::{self, Future},
};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use inlinable_string::InlinableString;
use metaconfig_types::{BlobConfig, BlobstoreId, ScrubAction, StorageConfig};
use multiplexedblob::{LoggingScrubHandler, ScrubHandler};
use prefixblob::PrefixBlobstore;
use slog::Logger;
use sql_ext::MysqlOptions;
use std::{convert::From, sync::Arc};

fn get_blobconfig(
    blob_config: BlobConfig,
    inner_blobstore_id: Option<u64>,
) -> Result<BlobConfig, Error> {
    match inner_blobstore_id {
        None => Ok(blob_config),
        Some(inner_blobstore_id) => match blob_config {
            BlobConfig::Multiplexed { blobstores, .. } => {
                let seeked_id = BlobstoreId::new(inner_blobstore_id);
                blobstores
                    .into_iter()
                    .find_map(|(blobstore_id, blobstore)| {
                        if blobstore_id == seeked_id {
                            Some(blobstore)
                        } else {
                            None
                        }
                    })
                    .ok_or(format_err!(
                        "could not find a blobstore with id {}",
                        inner_blobstore_id
                    ))
            }
            _ => Err(format_err!(
                "inner-blobstore-id supplied but blobstore is not multiplexed"
            )),
        },
    }
}

pub fn open_blobstore(
    fb: FacebookInit,
    mysql_options: MysqlOptions,
    storage_config: StorageConfig,
    inner_blobstore_id: Option<u64>,
    // TODO(ahornby) take multiple prefix for when scrubbing multiple repos
    prefix: Option<String>,
    readonly_storage: ReadOnlyStorage,
    scrub_action: Option<ScrubAction>,
    logger: Logger,
) -> BoxFuture<(BoxFuture<Arc<dyn Blobstore>, Error>, SqlFactory), Error> {
    // Allow open of just one inner store
    let mut blobconfig =
        try_boxfuture!(get_blobconfig(storage_config.blobstore, inner_blobstore_id));

    let scrub_handler = scrub_action.map(|scrub_action| {
        blobconfig.set_scrubbed(scrub_action);
        Arc::new(LoggingScrubHandler::new(false)) as Arc<dyn ScrubHandler>
    });

    let datasources = make_sql_factory(
        fb,
        storage_config.dbconfig,
        mysql_options,
        readonly_storage,
        logger,
    )
    .and_then(move |sql_factory| {
        let blobstore = match (scrub_handler, blobconfig) {
            (
                Some(scrub_handler),
                BlobConfig::Scrub {
                    scuba_table,
                    blobstores,
                    scrub_action,
                },
            ) => make_blobstore_multiplexed(
                fb,
                &scuba_table,
                &blobstores,
                Some(&sql_factory),
                mysql_options,
                readonly_storage,
                Some((scrub_handler, scrub_action)),
            ),
            (
                None,
                BlobConfig::Multiplexed {
                    scuba_table,
                    blobstores,
                },
            ) => make_blobstore_multiplexed(
                fb,
                &scuba_table,
                &blobstores,
                Some(&sql_factory),
                mysql_options,
                readonly_storage,
                None,
            ),
            (None, blobconfig) => make_blobstore(
                fb,
                &blobconfig,
                &sql_factory,
                mysql_options,
                readonly_storage,
            ),
            (Some(_), _) => {
                future::err(format_err!("Scrub action passed for non-scrubbable store")).boxify()
            }
        };
        future::ok((blobstore, sql_factory))
    });

    datasources
        .map(move |(storage, sql_factory)| {
            // Only need to prefix at this level if not using via blob repo, e.g. GC
            let maybe_prefixed = match prefix {
                Some(prefix) => storage
                    .map(|s| {
                        Arc::new(PrefixBlobstore::new(s, InlinableString::from(prefix)))
                            as Arc<dyn Blobstore>
                    })
                    .left_future(),
                None => storage.right_future(),
            };

            // Redaction would go here if needed
            (maybe_prefixed.boxify(), sql_factory)
        })
        .boxify()
}
