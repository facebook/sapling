/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bonsai_globalrev_mapping::bulk_import_globalrevs;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_globalrev_mapping::SqlBonsaiGlobalrevMappingBuilder;
use bytes::Bytes;
use changesets::deserialize_cs_entries;
use changesets::ChangesetEntry;
use clap_old::Arg;
use cloned::cloned;
use cmdlib::args;
use cmdlib::args::MononokeClapApp;
use cmdlib::helpers::block_execute;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::compat::Future01CompatExt;
use futures::FutureExt;
use futures::TryFutureExt;
use futures_ext::BoxFuture;
use futures_ext::FutureExt as _;
use futures_old::future::Future;
use futures_old::future::IntoFuture;
use futures_old::stream;
use futures_old::stream::Stream;
use std::fs;
use std::path::Path;
use std::sync::Arc;

fn setup_app<'a, 'b>() -> MononokeClapApp<'a, 'b> {
    args::MononokeAppBuilder::new("Tool to upload globalrevs from commits saved in file")
        .build()
        .arg(Arg::from_usage(
            "<IN_FILENAME>  'file with bonsai changesets'",
        ))
}

fn parse_serialized_commits<P: AsRef<Path>>(file: P) -> Result<Vec<ChangesetEntry>, Error> {
    let data = fs::read(file).map_err(Error::from)?;
    deserialize_cs_entries(&Bytes::from(data))
}

pub fn upload<P: AsRef<Path>>(
    ctx: CoreContext,
    repo: BlobRepo,
    in_path: P,
    globalrevs_store: Arc<dyn BonsaiGlobalrevMapping>,
) -> BoxFuture<(), Error> {
    let chunk_size = 1000;
    parse_serialized_commits(in_path)
        .into_future()
        .and_then(move |changesets| {
            stream::iter_ok(changesets)
                .map({
                    cloned!(ctx, repo);
                    move |entry| {
                        cloned!(ctx, repo);
                        async move { entry.cs_id.load(&ctx, repo.blobstore()).await }
                            .boxed()
                            .compat()
                            .from_err()
                    }
                })
                .buffered(chunk_size)
                .chunks(chunk_size)
                .and_then(move |chunk| {
                    let ctx = ctx.clone();
                    let store = globalrevs_store.clone();

                    async move { bulk_import_globalrevs(&ctx, &store, chunk.iter()).await }
                        .boxed()
                        .compat()
                })
                .for_each(|_| Ok(()))
        })
        .boxify()
}
#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let matches = setup_app().get_matches(fb)?;

    let logger = matches.logger();
    let config_store = matches.config_store();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let run = async {
        let repo: BlobRepo = args::open_repo(fb, logger, &matches).await?;
        let globalrevs_store = Arc::new(
            args::open_sql::<SqlBonsaiGlobalrevMappingBuilder>(fb, config_store, &matches)?
                .build(repo.get_repoid()),
        );
        let in_filename = matches.value_of("IN_FILENAME").unwrap();
        upload(ctx, repo, in_filename, globalrevs_store)
            .compat()
            .await?;
        Ok(())
    };

    block_execute(
        run,
        fb,
        "upload_globalrevs",
        logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}
