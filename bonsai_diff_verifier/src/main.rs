/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#[deny(warnings)]
use cloned::cloned;
use failure_ext::Error;
use futures::stream::Stream;
use futures_ext::{spawn_future, StreamExt};
use futures_util::{
    compat::Future01CompatExt,
    future::FutureExt as Futures03FutureExt,
    try_future::{try_join_all, TryFutureExt},
    try_join,
};
use std::collections::HashSet;
use std::sync::Arc;

use blobrepo::BlobRepo;
use blobstore::Blobstore;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_utils::{bonsai_diff as old_bonsai_diff, BonsaiDiffResult};
use clap::Arg;
use cmdlib::args;
use context::CoreContext;
use fbinit::FacebookInit;
use manifest::{bonsai_diff as new_bonsai_diff, BonsaiDiffFileChange, StoreLoadable};
use mercurial_types::{
    blobs::{HgBlobChangeset, HgBlobEntry},
    changeset::Changeset,
    HgEntry,
};
use mononoke_types::{ChangesetId, RepositoryId};
use revset::AncestorsNodeStream;
use tokio::runtime::Runtime;

const ARG_ROOT_COMMIT: &str = "root-bonsai-commit";
const ARG_LIMIT: &str = "limit";
const ARG_CONCURRENCY: &str = "concurrency";

enum LoadManifestError {
    BlobMissing,
    Error(Error),
}

impl From<Error> for LoadManifestError {
    fn from(e: Error) -> Self {
        Self::Error(e)
    }
}

async fn load_hg_changeset<M: BonsaiHgMapping>(
    ctx: &CoreContext,
    repo_id: RepositoryId,
    blobstore: Arc<dyn Blobstore>,
    mapping: &M,
    cs_id: ChangesetId,
) -> Result<HgBlobChangeset, LoadManifestError> {
    let hg_cs_id = mapping
        .get_hg_from_bonsai(ctx.clone(), repo_id, cs_id)
        .compat()
        .await?
        .ok_or(LoadManifestError::BlobMissing)?;

    let cs = HgBlobChangeset::load(ctx.clone(), &blobstore, hg_cs_id)
        .compat()
        .await?
        .ok_or(LoadManifestError::BlobMissing)?;

    Ok(cs)
}

async fn verify_bonsai_diff(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
) -> Result<Option<bool>, Error> {
    let repo_id = repo.get_repoid();
    let mapping = repo.get_bonsai_hg_mapping();
    let blobstore = repo.get_blobstore().boxed();

    let cs = cs_id.load(ctx.clone(), &blobstore).compat().await?;

    let hg_cs = load_hg_changeset(ctx, repo_id, blobstore.clone(), &mapping, cs_id);

    let hg_parents = cs
        .parents()
        .map(|cs_id| load_hg_changeset(ctx, repo_id, blobstore.clone(), &mapping, cs_id));

    let (hg_cs, hg_parents) = match try_join!(hg_cs, try_join_all(hg_parents)) {
        Ok((hg_cs, hg_parents)) => (hg_cs, hg_parents),
        Err(LoadManifestError::BlobMissing) => return Ok(Some(false)),
        Err(LoadManifestError::Error(e)) => return Err(e),
    };

    let new_diff = new_bonsai_diff(
        ctx.clone(),
        blobstore.clone(),
        hg_cs.manifestid(),
        hg_parents.iter().map(|p| p.manifestid()).collect(),
    )
    .collect()
    .compat();

    let mut parent_entries = hg_parents.iter().map(|p| {
        Box::new(HgBlobEntry::new_root(blobstore.clone(), p.manifestid()))
            as Box<dyn HgEntry + Sync>
    });

    let old_diff = old_bonsai_diff(
        ctx.clone(),
        Box::new(HgBlobEntry::new_root(blobstore.clone(), hg_cs.manifestid())),
        parent_entries.next(),
        parent_entries.next(),
    )
    .collect()
    .compat();

    // Only 2 parents in hg!
    assert!(parent_entries.next().is_none());

    let (new_diff, old_diff) = try_join!(new_diff, old_diff)?;

    let new_diff: HashSet<_> = new_diff.into_iter().collect();

    let old_diff: HashSet<_> = old_diff
        .into_iter()
        .map(|c| match c {
            BonsaiDiffResult::Changed(path, ft, id) => BonsaiDiffFileChange::Changed(path, ft, id),
            BonsaiDiffResult::ChangedReusedId(path, ft, id) => {
                BonsaiDiffFileChange::ChangedReusedId(path, ft, id)
            }
            BonsaiDiffResult::Deleted(path) => BonsaiDiffFileChange::Deleted(path),
        })
        .collect();

    let ok = new_diff == old_diff;
    println!("{}: {} ({} changes)", cs_id, ok, new_diff.len());

    if !ok {
        println!("hg_cs_id: {}", hg_cs.get_changeset_id());
        let mut old_diff: Vec<_> = old_diff.into_iter().collect();
        old_diff.sort();

        let mut new_diff: Vec<_> = new_diff.into_iter().collect();
        new_diff.sort();

        println!("Old: {:#?}", old_diff);
        println!("New: {:#?}", new_diff);
    }

    Ok(Some(ok))
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = args::MononokeApp::new("Mononoke Bonsai Diff Verifier")
        .with_advanced_args_hidden()
        .build()
        .arg(Arg::with_name(ARG_ROOT_COMMIT).help("Bonsai commit to start walking from"))
        .arg(
            Arg::with_name(ARG_LIMIT)
                .long(ARG_LIMIT)
                .takes_value(true)
                .help("Number of commits to check"),
        )
        .arg(
            Arg::with_name(ARG_CONCURRENCY)
                .long(ARG_CONCURRENCY)
                .takes_value(true)
                .help("Number of commits to process in parallel"),
        );

    let matches = app.get_matches();
    let mut runtime = Runtime::new()?;

    args::init_cachelib(fb, &matches);
    let logger = args::init_logging(fb, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let repo = runtime.block_on(args::open_repo(fb, &logger, &matches))?;

    let cs_id = ChangesetId::from_str(matches.value_of(ARG_ROOT_COMMIT).unwrap())?;
    let concurrency = matches.value_of(ARG_CONCURRENCY).unwrap_or("20").parse()?;

    let stream = AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), cs_id)
        .map(move |cs_id| {
            cloned!(ctx, repo);
            let fut = async move { verify_bonsai_diff(&ctx, &repo, cs_id).await }
                .boxed()
                .compat();
            spawn_future(fut)
        })
        .buffered(concurrency);

    let stream = match matches.value_of(ARG_LIMIT) {
        Some(limit) => stream.take(limit.parse()?).left_stream(),
        None => stream.right_stream(),
    };

    runtime.block_on(stream.for_each(|_| Ok(())))
}
