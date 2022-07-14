/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Tool to regenerate filenodes. It can be used to fix up linknodes -
//! but it should be used with caution! PLEASE RUN IT ONLY IF YOU KNOW WHAT YOU ARE DOING!

use anyhow::bail;
use anyhow::format_err;
use anyhow::Error;
use blobrepo::BlobRepo;
use blobrepo_override::DangerousOverride;
use blobstore::Blobstore;
use blobstore::Loadable;
use cacheblob::MemWritesBlobstore;
use cmdlib::args;
use context::CoreContext;
use derived_data_filenodes::generate_all_filenodes;
use fbinit::FacebookInit;
use futures::future::join_all;
use futures::future::FutureExt;
use mercurial_types::HgChangesetId;
use mercurial_types::HgNodeHash;
use repo_derived_data::RepoDerivedDataRef;
use slog::info;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::str::FromStr;
use std::sync::Arc;

fn convert_to_cs(s: &str) -> Option<HgChangesetId> {
    let nodehash = HgNodeHash::from_str(s).unwrap_or_else(|_| panic!("malformed hash: {}", s));
    nodehash.into_option().map(HgChangesetId::new)
}

async fn regenerate_single_manifest(
    ctx: CoreContext,
    repo: BlobRepo,
    hg_cs: HgChangesetId,
) -> Result<(), Error> {
    // To lower the risk of accidentally overwriting prod data let's use in memory BlobRepo.
    // In theory it's possible to corrupt data in other stores
    // (e.g. bookmarks, bonsai hg mapping).
    // Therefore the code below SHOULD NOT write data to anything other than blobstore and filenodes
    let repo = repo.dangerous_override(|blobstore| -> Arc<dyn Blobstore> {
        Arc::new(MemWritesBlobstore::new(blobstore))
    });

    let maybe_cs_id = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(&ctx, hg_cs)
        .await?;
    let cs_id = maybe_cs_id.ok_or(format_err!("changeset not found {}", hg_cs))?;
    let bonsai = cs_id.load(&ctx, repo.blobstore()).await?;

    let toinsert = generate_all_filenodes(
        &ctx,
        &repo.repo_derived_data().manager().derivation_context(None),
        &bonsai,
    )
    .await?;

    repo.filenodes()
        .add_or_replace_filenodes(&ctx, toinsert)
        .await?
        .do_not_handle_disabled_filenodes()?;

    Ok(())
}

async fn regenerate_all_manifests(
    ctx: CoreContext,
    repo: BlobRepo,
    hg_css: Vec<HgChangesetId>,
    chunk_size: usize,
) -> Result<(), Error> {
    let mut i = 0;
    for chunk in hg_css.chunks(chunk_size) {
        let mut futs = vec![];
        for hg_cs in chunk {
            futs.push(regenerate_single_manifest(
                ctx.clone(),
                repo.clone(),
                *hg_cs,
            ));
            i += 1;
        }
        let res: Result<Vec<_>, Error> = join_all(futs).await.into_iter().collect();
        res?;
        info!(ctx.logger(), "processed {}", i);
    }
    Ok(())
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let matches = args::MononokeAppBuilder::new(
        "Tool to regenerate filenodes").with_advanced_args_hidden().build()
        .args_from_usage(
            r#"
               --i-know-what-i-am-doing
               --file [FILE]                        'contains list of commit hashes for which we want to regenerate filenodes'
               --chunk-size [SIZE]                  'how many commits to process at once (default: 100)'
            "#,
        )
        .get_matches(fb)?;

    if matches.values_of("i-know-what-i-am-doing").is_none() {
        bail!("this is a dangerous tool. DO NOT RUN if unsure how it works");
    }

    let logger = matches.logger();
    let rt = matches.runtime();

    let repo_fut = args::open_repo(fb, logger, &matches);
    let repo = rt.block_on(repo_fut).unwrap();

    let ctx = CoreContext::test_mock(fb);

    let inputfile = matches.value_of("file").expect("input file is not set");
    let inputfile = File::open(inputfile).expect("cannot open input file");

    let chunk_size = args::get_usize(&matches, "chunk-size", 100);
    let file = BufReader::new(&inputfile);
    rt.block_on(
        regenerate_all_manifests(
            ctx,
            repo,
            file.lines()
                .map(|line| convert_to_cs(&line.unwrap()).unwrap())
                .collect(),
            chunk_size,
        )
        .boxed(),
    )
}
