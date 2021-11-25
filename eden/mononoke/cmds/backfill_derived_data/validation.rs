/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Error};
use blobrepo::BlobRepo;
use blobrepo_override::DangerousOverride;
use blobstore::Blobstore;
use bonsai_hg_mapping::{ArcBonsaiHgMapping, MemWritesBonsaiHgMapping};
use borrowed::borrowed;
use cacheblob::MemWritesBlobstore;
use clap::ArgMatches;
use cmdlib::args::{self, MononokeMatches};
use context::CoreContext;
use derived_data_utils::derived_data_utils;
use futures::{future::try_join, stream, StreamExt, TryStreamExt};
use readonlyblob::ReadOnlyBlobstore;
use slog::info;
use std::sync::Arc;

use crate::commit_discovery::CommitDiscoveryOptions;
use crate::regenerate;
use crate::{ARG_DERIVED_DATA_TYPE, ARG_VALIDATE_CHUNK_SIZE};

pub async fn validate(
    ctx: &CoreContext,
    matches: &MononokeMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> Result<(), Error> {
    if !matches.environment().readonly_storage.0 {
        return Err(anyhow!(
            "validate subcommand should be run only on readonly storage!"
        ));
    }
    let repo: BlobRepo = args::open_repo_unredacted(ctx.fb, ctx.logger(), matches).await?;
    let csids = CommitDiscoveryOptions::from_matches(&ctx, &repo, sub_m)
        .await?
        .get_commits();

    let derived_data_type = sub_m
        .value_of(ARG_DERIVED_DATA_TYPE)
        .ok_or_else(|| anyhow!("{} is not set", ARG_DERIVED_DATA_TYPE))?;
    let opts = regenerate::DeriveOptions::from_matches(sub_m)?;

    let validate_chunk_size = args::get_usize(&sub_m, ARG_VALIDATE_CHUNK_SIZE, 10000);

    info!(ctx.logger(), "Started validation");
    for chunk in csids.chunks(validate_chunk_size) {
        let chunk = chunk.to_vec();
        info!(
            ctx.logger(),
            "Processing chunk starting from {:?}",
            chunk.get(0)
        );
        let orig_repo = repo.clone();
        let mut memblobstore = None;
        let repo = repo
            .dangerous_override(|blobstore| -> Arc<dyn Blobstore> {
                let blobstore = Arc::new(MemWritesBlobstore::new(blobstore));
                memblobstore = Some(blobstore.clone());
                blobstore
            })
            .dangerous_override(|bonsai_hg_mapping| -> ArcBonsaiHgMapping {
                Arc::new(MemWritesBonsaiHgMapping::new(bonsai_hg_mapping))
            });
        let memblobstore = memblobstore.unwrap();

        regenerate::regenerate_derived_data(
            &ctx,
            &repo,
            chunk.clone(),
            vec![derived_data_type.to_string()],
            &opts,
        )
        .await?;

        let cache = memblobstore.get_cache().lock().unwrap();
        info!(ctx.logger(), "created {} blobs", cache.len());
        let real_derived_utils = &derived_data_utils(ctx.fb, &orig_repo, derived_data_type)?;

        let repo = repo.dangerous_override(|blobstore| -> Arc<dyn Blobstore> {
            Arc::new(ReadOnlyBlobstore::new(blobstore))
        });
        let rederived_utils = &derived_data_utils(ctx.fb, &repo, derived_data_type)?;

        // Make sure that the generated data was saved in memory blobstore
        memblobstore.set_no_access_to_inner(true);

        borrowed!(ctx, orig_repo, repo);
        stream::iter(chunk)
            .map(Ok)
            .try_for_each_concurrent(100, |csid| async move {
                if !rederived_utils.is_derived(&ctx, csid).await? {
                    return Err(anyhow!("{} unexpectedly not derived", csid));
                }

                let f1 = real_derived_utils.derive(ctx.clone(), orig_repo.clone(), csid);
                let f2 = rederived_utils.derive(ctx.clone(), repo.clone(), csid);
                let (real, rederived) = try_join(f1, f2).await?;
                if real != rederived {
                    Err(anyhow!("mismatch in {}: {} vs {}", csid, real, rederived))
                } else {
                    Ok(())
                }
            })
            .await?;
        info!(ctx.logger(), "Validation successful!");
    }

    Ok(())
}
