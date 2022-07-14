/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use context::CoreContext;
use cross_repo_sync::types::Source;
use futures::future::try_join_all;
use mononoke_api_types::InnerRepo;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use movers::Mover;
use sorted_vector_map::SortedVectorMap;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

pub const EXTRA_PREFIX: &str = "source-cs-id-";

pub async fn find_source_repos<'a>(
    ctx: &CoreContext,
    hyper_repo: &BlobRepo,
    cs_id: ChangesetId,
    matches: &'a MononokeMatches<'_>,
) -> Result<Vec<InnerRepo>, Error> {
    let source_repos_and_latest_synced_cs_ids =
        find_source_repos_and_latest_synced_cs_ids(ctx, hyper_repo, cs_id, matches).await?;

    Ok(source_repos_and_latest_synced_cs_ids
        .into_iter()
        .map(|(source_repo, _)| source_repo)
        .collect())
}

pub async fn find_source_repos_and_latest_synced_cs_ids<'a>(
    ctx: &CoreContext,
    hyper_repo: &BlobRepo,
    cs_id: ChangesetId,
    matches: &'a MononokeMatches<'_>,
) -> Result<Vec<(InnerRepo, Source<ChangesetId>)>, Error> {
    let cs = cs_id.load(ctx, &hyper_repo.get_blobstore()).await?;

    let latest_synced_state = decode_latest_synced_state_extras(cs.extra())?;

    let source_repos: Vec<(InnerRepo, Source<ChangesetId>)> = try_join_all(
        latest_synced_state
            .into_iter()
            .map(|(name, cs_id)| async move {
                let repo =
                    args::open_repo_with_repo_name(ctx.fb, ctx.logger(), name.to_string(), matches)
                        .await?;
                anyhow::Ok((repo, Source(cs_id)))
            }),
    )
    .await?;

    Ok(source_repos)
}

pub fn encode_latest_synced_state_extras(
    latest_synced_state: &HashMap<String, ChangesetId>,
) -> SortedVectorMap<String, Vec<u8>> {
    latest_synced_state
        .iter()
        .map(|(name, cs_id)| {
            (
                format!("{}{}", EXTRA_PREFIX, name),
                Vec::from(cs_id.to_hex().as_bytes()),
            )
        })
        .collect()
}

pub fn decode_latest_synced_state_extras<'a>(
    extra: impl Iterator<Item = (&'a str, &'a [u8])>,
) -> Result<HashMap<String, ChangesetId>, Error> {
    extra
        .into_iter()
        .filter_map(|(name, value)| {
            name.strip_prefix(EXTRA_PREFIX)
                .map(|repo_name| (repo_name.to_string(), value))
        })
        .map(|(repo_name, value)| {
            let cs_id = ChangesetId::from_str(&String::from_utf8(value.to_vec())?)?;
            anyhow::Ok((repo_name, cs_id))
        })
        .collect::<Result<HashMap<_, _>, _>>()
        .context("failed to parsed latest synced state extras")
}

pub fn get_mover_and_reverse_mover(source_repo: &BlobRepo) -> Result<(Mover, Mover), Error> {
    let prefix = MPath::new(source_repo.name())?;
    let mover = Arc::new({
        let prefix = prefix.clone();
        move |path: &MPath| Ok(Some(prefix.join(path)))
    });

    let reverse_mover = Arc::new(move |path: &MPath| Ok(path.remove_prefix_component(&prefix)));

    Ok((mover, reverse_mover))
}
