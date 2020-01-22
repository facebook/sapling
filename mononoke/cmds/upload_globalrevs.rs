/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use blobrepo::BlobRepo;
use bonsai_globalrev_mapping::{
    bulk_import_globalrevs, BonsaiGlobalrevMapping, SqlBonsaiGlobalrevMapping,
};
use bytes::Bytes;
use changesets::{deserialize_cs_entries, ChangesetEntry};
use clap::{App, Arg};
use cloned::cloned;
use cmdlib::{args, helpers::block_execute};
use context::CoreContext;
use fbinit::FacebookInit;
use futures::future::{Future, IntoFuture};
use futures::stream;
use futures::stream::Stream;
use futures_ext::{BoxFuture, FutureExt};
use futures_preview::compat::Future01CompatExt;
use std::fs;
use std::path::Path;
use std::sync::Arc;

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    args::MononokeApp::new("Tool to upload globalrevs from commits saved in file")
        .build()
        .version("0.0.0")
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
                    move |bcs_entry| repo.get_bonsai_changeset(ctx.clone(), bcs_entry.cs_id)
                })
                .buffered(chunk_size)
                .chunks(chunk_size)
                .and_then(move |chunk| {
                    bulk_import_globalrevs(
                        ctx.clone(),
                        repo.get_repoid(),
                        globalrevs_store.clone(),
                        chunk,
                    )
                })
                .for_each(|_| Ok(()))
        })
        .boxify()
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let matches = setup_app().get_matches();

    args::init_cachelib(fb, &matches);

    let logger = args::init_logging(fb, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let globalrevs_store = args::open_sql::<SqlBonsaiGlobalrevMapping>(fb, &matches);

    let run = args::open_repo(fb, &logger, &matches)
        .join(globalrevs_store)
        .and_then({
            let matches = matches.clone();
            move |(repo, globalrevs_store)| {
                let in_filename = matches.value_of("IN_FILENAME").unwrap();
                let globalrevs_store = Arc::new(globalrevs_store);
                upload(ctx, repo, in_filename, globalrevs_store)
            }
        });

    block_execute(run.compat(), fb, "upload_globalrevs", &logger, &matches)
}
