/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::error::SubcommandError;

use anyhow::format_err;
use anyhow::Error;
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use clap_old::App;
use clap_old::Arg;
use clap_old::ArgMatches;
use clap_old::SubCommand;
use cloned::cloned;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use cmdlib::helpers;
use context::CoreContext;
use deleted_manifest::DeletedManifestOps;
use deleted_manifest::RootDeletedManifestV2Id;
use derived_data::BonsaiDerived;
use fbinit::FacebookInit;
use futures::compat::Stream01CompatExt;
use futures::future;
use futures::StreamExt;
use futures::TryStreamExt;
use manifest::get_implicit_deletes;
use manifest::PathOrPrefix;
use mercurial_derived_data::DeriveHgChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::DeletedManifestV2Id;
use mononoke_types::MPath;
use revset::AncestorsNodeStream;
use slog::debug;
use slog::Logger;
use std::collections::BTreeSet;
use std::str::FromStr;

pub const DELETED_MANIFEST: &str = "deleted-manifest";
const COMMAND_MANIFEST: &str = "manifest";
const COMMAND_VERIFY: &str = "verify";
const COMMAND_FETCH: &str = "fetch";
const ARG_CSID: &str = "csid";
const ARG_ID: &str = "id";
const ARG_LIMIT: &str = "limit";
const ARG_PATH: &str = "path";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    let csid_arg = Arg::with_name(ARG_CSID)
        .help("{hg|bonsai} changeset id or bookmark name")
        .index(1)
        .required(true);

    let path_arg = Arg::with_name(ARG_PATH)
        .help("path")
        .index(2)
        .default_value("");

    SubCommand::with_name(DELETED_MANIFEST)
        .about("derive, inspect and verify deleted manifest")
        .subcommand(
            SubCommand::with_name(COMMAND_MANIFEST)
                .about("recursively list all deleted manifest entries under the given path")
                .arg(csid_arg.clone())
                .arg(path_arg.clone()),
        )
        .subcommand(
            SubCommand::with_name(COMMAND_VERIFY)
                .about("verify deleted manifest against actual paths deleted in commits")
                .arg(csid_arg.clone())
                .arg(
                    Arg::with_name(ARG_LIMIT)
                        .help("number of commits to be verified")
                        .takes_value(true)
                        .required(true),
                ),
        )
        .subcommand(
            SubCommand::with_name(COMMAND_FETCH)
                .about("fetch and print deleted manifest entry by id")
                .arg(
                    Arg::with_name(ARG_ID)
                        .help("deleted file manifest id to fetch")
                        .takes_value(true)
                        .required(true),
                ),
        )
}

pub async fn subcommand_deleted_manifest<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'_>,
    sub_matches: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    let repo: BlobRepo = args::open_repo(fb, &logger, matches).await?;
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    match sub_matches.subcommand() {
        (COMMAND_MANIFEST, Some(matches)) => {
            let hash_or_bookmark = String::from(matches.value_of(ARG_CSID).unwrap());
            let path = match matches.value_of(ARG_PATH).unwrap() {
                "" => None,
                p => MPath::new(p).map(Some)?,
            };
            let cs_id = helpers::csid_resolve(&ctx, repo.clone(), hash_or_bookmark).await?;
            subcommand_manifest(ctx, repo, cs_id, path).await?;
            Ok(())
        }
        (COMMAND_VERIFY, Some(matches)) => {
            let hash_or_bookmark = String::from(matches.value_of(ARG_CSID).unwrap());
            let limit = matches
                .value_of(ARG_LIMIT)
                .unwrap()
                .parse::<u64>()
                .expect("limit must be an integer");
            let cs_id = helpers::csid_resolve(&ctx, repo.clone(), hash_or_bookmark).await?;
            subcommand_verify(ctx, repo, cs_id, limit).await?;
            Ok(())
        }
        (COMMAND_FETCH, Some(matches)) => {
            let mf_id = DeletedManifestV2Id::from_str(
                matches
                    .value_of(ARG_ID)
                    .ok_or_else(|| format_err!("{} not set", ARG_ID))?,
            )?;
            let mf = mf_id
                .load(&ctx, repo.blobstore())
                .await
                .map_err(Error::from)?;
            println!("{:?}", mf);
            Ok(())
        }
        _ => Err(SubcommandError::InvalidArgs),
    }
}

async fn subcommand_manifest(
    ctx: CoreContext,
    repo: BlobRepo,
    cs_id: ChangesetId,
    prefix: Option<MPath>,
) -> Result<(), Error> {
    let root_manifest = RootDeletedManifestV2Id::derive(&ctx, &repo, cs_id).await?;
    debug!(ctx.logger(), "ROOT Deleted Manifest V2 {:?}", root_manifest,);
    let mut entries: Vec<_> = root_manifest
        .find_entries(&ctx, repo.blobstore(), Some(PathOrPrefix::Prefix(prefix)))
        .try_collect()
        .await?;
    entries.sort_by_key(|(path, _)| path.clone());
    for (path, mf_id) in entries {
        println!("{}/ {:?}", MPath::display_opt(path.as_ref()), mf_id);
    }
    Ok(())
}

async fn subcommand_verify(
    ctx: CoreContext,
    repo: BlobRepo,
    cs_id: ChangesetId,
    limit: u64,
) -> Result<(), Error> {
    let mut csids = AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), cs_id)
        .compat()
        .take(limit as usize);
    while let Some(cs_id) = csids.try_next().await? {
        verify_single_commit(ctx.clone(), repo.clone(), cs_id).await?
    }
    Ok(())
}

async fn get_file_changes(
    ctx: CoreContext,
    repo: BlobRepo,
    cs_id: ChangesetId,
) -> Result<(Vec<MPath>, Vec<MPath>), Error> {
    let paths_added_fut = async {
        let bonsai = cs_id.load(&ctx, repo.blobstore()).await?;
        let paths = bonsai
            .into_mut()
            .file_changes
            .into_iter()
            .filter_map(|(path, change)| change.is_changed().then(|| path))
            .collect::<Vec<_>>();
        Ok::<_, Error>(paths)
    };

    let parent_manifests_fut = async {
        let hg_cs_id = repo.derive_hg_changeset(&ctx, cs_id).await?;
        let parents = repo.get_changeset_parents(ctx.clone(), hg_cs_id).await?;
        let parents_futs = parents.into_iter().map(|csid| {
            cloned!(ctx, repo);
            async move {
                let blob = csid.load(&ctx, repo.blobstore()).await?;
                Ok(blob.manifestid())
            }
        });
        future::try_join_all(parents_futs).await
    };

    let (paths_added, parent_manifests) =
        futures::try_join!(paths_added_fut, parent_manifests_fut)?;
    let paths_deleted = get_implicit_deletes(
        &ctx,
        repo.get_blobstore(),
        paths_added.clone(),
        parent_manifests,
    )
    .try_collect()
    .await?;
    Ok((paths_added, paths_deleted))
}

async fn verify_single_commit(
    ctx: CoreContext,
    repo: BlobRepo,
    cs_id: ChangesetId,
) -> Result<(), Error> {
    let file_changes = get_file_changes(ctx.clone(), repo.clone(), cs_id.clone());
    let deleted_manifest_paths = async move {
        let root_manifest = RootDeletedManifestV2Id::derive(&ctx, &repo, cs_id).await?;
        let entries: BTreeSet<_> = root_manifest
            .list_all_entries(&ctx, repo.blobstore())
            .try_filter_map(|(path_opt, ..)| async move { Ok(path_opt) })
            .try_collect()
            .await?;
        Ok(entries)
    };
    let ((paths_added, paths_deleted), deleted_manifest_paths) =
        futures::try_join!(file_changes, deleted_manifest_paths)?;
    for path in paths_added {
        // check that changed files are alive
        if deleted_manifest_paths.contains(&path) {
            println!("Path {} is alive in changeset {:?}", path, cs_id);
            return Err(format_err!("Path {} is alive", path));
        }
    }
    for path in paths_deleted {
        // check that deleted files are in the manifest
        if !deleted_manifest_paths.contains(&path) {
            println!("Path {} was deleted in changeset {:?}", path, cs_id);
            return Err(format_err!("Path {} is deleted", path));
        }
    }
    Ok(())
}
