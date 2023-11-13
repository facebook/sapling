/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![cfg_attr(not(fbcode_build), allow(unused_crate_dependencies))]

use std::collections::HashMap;
use std::fs::read;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::bail;
use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use ascii::AsciiString;
use blobimport_lib::BookmarkImportPolicy;
use blobrepo::BlobRepo;
use bonsai_globalrev_mapping::SqlBonsaiGlobalrevMappingBuilder;
use changeset_fetcher::ChangesetFetcherRef;
use clap::Parser;
use cmdlib::monitoring::AliveService;
use context::CoreContext;
use context::SessionContainer;
use derived_data_manager::BonsaiDerivable;
use failure_ext::SlogKVError;
use fbinit::FacebookInit;
use filenodes_derivation::FilenodesOnlyPublic;
use futures::future::try_join;
use futures::future::TryFutureExt;
#[cfg(fbcode_build)]
use mercurial_revlog::revlog::RevIdx;
use mercurial_types::HgChangesetId;
use mercurial_types::HgNodeHash;
use mononoke_app::args::AsRepoArg;
use mononoke_app::args::RepoArgs;
use mononoke_app::fb303::Fb303AppExtension;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;
use mononoke_types::ChangesetId;
use mutable_counters::MutableCountersRef;
use repo_identity::RepoIdentityRef;
use slog::error;
use slog::info;
use slog::warn;
use slog::Logger;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use synced_commit_mapping::SqlSyncedCommitMapping;
use wireproto_handler::BackupSourceRepo;

fn parse_fixed_parent_order<P: AsRef<Path>>(
    logger: &Logger,
    p: P,
) -> Result<HashMap<HgChangesetId, Vec<HgChangesetId>>> {
    let content = read(p)?;
    let mut res = HashMap::new();

    for line in String::from_utf8(content).map_err(Error::from)?.split('\n') {
        if line.is_empty() {
            continue;
        }
        let mut iter = line.split(' ').map(HgChangesetId::from_str).fuse();
        let maybe_hg_cs_id = iter.next();
        let hg_cs_id = match maybe_hg_cs_id {
            Some(hg_cs_id) => hg_cs_id?,
            None => {
                continue;
            }
        };

        let parents = match (iter.next(), iter.next()) {
            (Some(p1), Some(p2)) => vec![p1?, p2?],
            (Some(p), None) => {
                warn!(
                    logger,
                    "{}: parent order is fixed for a single parent, most likely won't have any effect",
                    hg_cs_id,
                );
                vec![p?]
            }
            (None, None) => {
                warn!(
                    logger,
                    "{}: parent order is fixed for a commit with no parents, most likely won't have any effect",
                    hg_cs_id,
                );
                vec![]
            }
            (None, Some(_)) => unreachable!(),
        };
        if iter.next().is_some() {
            bail!("got 3 parents, but mercurial supports at most 2!");
        }

        if res.insert(hg_cs_id, parents).is_some() {
            warn!(logger, "order is fixed twice for {}!", hg_cs_id);
        }
    }
    Ok(res)
}

#[cfg(fbcode_build)]
mod facebook {
    use manifold_client::cpp_client::ClientOptionsBuilder;
    use manifold_client::cpp_client::ManifoldCppClient;
    use manifold_client::write::WriteRequestOptionsBuilder;
    use manifold_client::ManifoldClient;

    use super::*;

    pub async fn update_manifold_key(
        fb: FacebookInit,
        latest_imported_rev: RevIdx,
        manifold_key: &str,
        manifold_bucket: &str,
    ) -> Result<()> {
        let opts = ClientOptionsBuilder::default()
            .build()
            .map_err(|e| format_err!("Cannot build Manifold options: {}", e))?;

        let client = ManifoldCppClient::from_options(fb, manifold_bucket, &opts)
            .context("Cannot build ManifoldCppClient")?;

        let opts = WriteRequestOptionsBuilder::default()
            .with_allow_overwrite_predicate(true)
            .build()
            .map_err(|e| format_err!("Cannot build Write options: {}", e))?;

        let mut req = client
            .create_write_request(&opts)
            .context("Cannot build write request")?;

        let next_revision_to_import = latest_imported_rev.as_u32() + 1;
        let payload = format!("{}", next_revision_to_import);
        req.write(manifold_key, payload.into()).await?;

        Ok(())
    }

    #[cfg(test)]
    mod test {
        use super::*;

        #[fbinit::test]
        async fn test_manifold_write(fb: FacebookInit) -> Result<(), Error> {
            update_manifold_key(
                fb,
                RevIdx::from(0u32),
                "flat/test_manifold_write",
                "mononoke_test",
            )
            .await?;

            update_manifold_key(
                fb,
                RevIdx::from(1u32),
                "flat/test_manifold_write",
                "mononoke_test",
            )
            .await?;

            Ok(())
        }
    }
}

async fn async_main(app: MononokeApp) -> Result<()> {
    let args: MononokeBlobImportArgs = app.args()?;
    let env = app.environment();
    let ctx = &SessionContainer::new_with_defaults(app.fb)
        .new_context(app.logger().clone(), env.scuba_sample_builder.clone());

    let changeset = args
        .changeset
        .map(|hash| HgNodeHash::from_str(&hash).unwrap());

    let manifold_key_bucket = match (args.manifold_next_rev_to_import, args.manifold_bucket) {
        (Some(key), Some(bucket)) => Some((key, bucket)),
        _ => None,
    };

    let bookmark_import_policy = if args.no_bookmark {
        BookmarkImportPolicy::Ignore
    } else {
        let prefix = match args.prefix_bookmark {
            Some(prefix) => AsciiString::from_ascii(prefix).unwrap(),
            None => AsciiString::new(),
        };
        BookmarkImportPolicy::Prefix(prefix)
    };

    let fixed_parent_order = if let Some(path) = args.fix_parent_order {
        parse_fixed_parent_order(ctx.logger(), path)
            .context("while parsing file with fixed parent order")?
    } else {
        HashMap::new()
    };

    let mut derived_data_types = args.derived_data_type;
    let excluded_derived_data_types = args.exclude_derived_data_type;

    for v in &excluded_derived_data_types {
        if derived_data_types.contains(v) {
            return Err(format_err!("Unexpected exclusion of requested {}", v));
        }
    }

    // Make sure filenodes derived unless specifically excluded since public hg changesets must have filenodes derived
    let filenodes_derived_name = FilenodesOnlyPublic::NAME.to_string();
    if !derived_data_types.contains(&filenodes_derived_name)
        && !excluded_derived_data_types.contains(&filenodes_derived_name)
    {
        derived_data_types.push(filenodes_derived_name);
    }

    let repo_arg = args.repo.as_repo_arg();
    let (_, repo_config) = app.repo_config(repo_arg)?;

    let globalrevs_store_builder = SqlBonsaiGlobalrevMappingBuilder::with_metadata_database_config(
        app.fb,
        &repo_config.storage_config.metadata,
        &env.mysql_options,
        env.readonly_storage.0,
    )
    .await?;
    let synced_commit_mapping = SqlSyncedCommitMapping::with_metadata_database_config(
        app.fb,
        &repo_config.storage_config.metadata,
        &env.mysql_options,
        env.readonly_storage.0,
    )
    .await?;

    let blobrepo: BlobRepo = if args.no_create {
        app.open_repo_unredacted(repo_arg).await?
    } else {
        app.create_repo_unredacted(repo_arg, None).await?
    };

    let small_repo_id = match (args.source_repo_id, args.source_repo_name) {
        (Some(id), _) => Some(RepoArgs::from_repo_id(id.parse()?)),
        (_, Some(name)) => Some(RepoArgs::from_repo_name(name)),
        _ => None,
    }
    .map(|source_repo_args| app.repo_id(source_repo_args.as_repo_arg()).unwrap());

    let backup_from_repo_args = match (args.backup_from_repo_id, args.backup_from_repo_name) {
        (Some(id), _) => Some(RepoArgs::from_repo_id(id.parse()?)),
        (_, Some(name)) => Some(RepoArgs::from_repo_name(name)),
        _ => None,
    };
    let origin_repo = match backup_from_repo_args {
        Some(backup_from_repo_args) => {
            Some(app.open_repo(backup_from_repo_args.as_repo_arg()).await?)
        }
        _ => None,
    };
    let globalrevs_store = Arc::new(globalrevs_store_builder.build(blobrepo.repo_identity().id()));
    let synced_commit_mapping = Arc::new(synced_commit_mapping);

    async move {
        let blobimport = blobimport_lib::Blobimport {
            ctx,
            blobrepo: blobrepo.clone(),
            revlogrepo_path: args.input,
            changeset,
            skip: args.skip,
            commits_limit: args.commits_limit,
            bookmark_import_policy,
            globalrevs_store,
            synced_commit_mapping,
            lfs_helper: args.lfs_helper,
            concurrent_changesets: args.concurrent_changesets,
            concurrent_blobs: args.concurrent_blobs,
            concurrent_lfs_imports: args.concurrent_lfs_imports,
            fixed_parent_order,
            has_globalrev: args.has_globalrev,
            populate_git_mapping: repo_config.pushrebase.populate_git_mapping,
            small_repo_id,
            derived_data_types,
            origin_repo: origin_repo.map(|repo| BackupSourceRepo::from_blob_repo(&repo)),
        };

        let maybe_latest_imported_rev = if args.find_already_imported_rev_only {
            blobimport.find_already_imported_revision().await?
        } else {
            blobimport.import().await?
        };

        match maybe_latest_imported_rev {
            Some((latest_imported_rev, latest_imported_cs_id)) => {
                info!(
                    ctx.logger(),
                    "latest imported revision {}",
                    latest_imported_rev.as_u32()
                );
                #[cfg(fbcode_build)]
                {
                    if let Some((manifold_key, bucket)) = manifold_key_bucket {
                        facebook::update_manifold_key(
                            app.fb,
                            latest_imported_rev,
                            &manifold_key,
                            &bucket,
                        )
                        .await?
                    }
                }
                #[cfg(not(fbcode_build))]
                {
                    assert!(
                        manifold_key_bucket.is_none(),
                        "Using Manifold is not supported in non fbcode builds"
                    );
                }

                maybe_update_highest_imported_generation_number(
                    ctx,
                    &blobrepo,
                    latest_imported_cs_id,
                )
                .await?;
            }
            None => info!(ctx.logger(), "didn't import any commits"),
        };
        Ok(())
    }
    .map_err({
        move |err| {
            // NOTE: We log the error immediatley, then provide another one for main's
            // Result (which will set our exit code).
            error!(ctx.logger(), "error while blobimporting"; SlogKVError(err));
            Error::msg("blobimport exited with a failure")
        }
    })
    .await
}

// Updating mutable_counters table to store the highest generation number that was imported.
// This in turn can be used to track which commits exist on both mercurial and Mononoke.
// For example, WarmBookmarkCache might consider a bookmark "warm" only if a commit is in both
// mercurial and Mononoke.
//
// Note that if a commit with a lower generation number was added (e.g. if this commit forked off from
// the main branch) then this hint will be misleading - i.e. the hint would store a higher generation
// number then the new commit which might not be processed by blobimport yet. In that case there are
// two options:
// 1) Use this hint only in single-branch repos
// 2) Accept that the hint might be incorrect sometimes.
async fn maybe_update_highest_imported_generation_number(
    ctx: &CoreContext,
    blobrepo: &BlobRepo,
    latest_imported_cs_id: ChangesetId,
) -> Result<(), Error> {
    let maybe_highest_imported_gen_num = blobrepo
        .mutable_counters()
        .get_counter(ctx, blobimport_lib::HIGHEST_IMPORTED_GEN_NUM);
    let new_gen_num = blobrepo
        .changeset_fetcher()
        .get_generation_number(ctx, latest_imported_cs_id);
    let (maybe_highest_imported_gen_num, new_gen_num) =
        try_join(maybe_highest_imported_gen_num, new_gen_num).await?;

    let new_gen_num = match maybe_highest_imported_gen_num {
        Some(highest_imported_gen_num) => {
            if new_gen_num.value() as i64 > highest_imported_gen_num {
                Some(new_gen_num)
            } else {
                None
            }
        }
        None => Some(new_gen_num),
    };

    if let Some(new_gen_num) = new_gen_num {
        blobrepo
            .mutable_counters()
            .set_counter(
                ctx,
                blobimport_lib::HIGHEST_IMPORTED_GEN_NUM,
                new_gen_num.value() as i64,
                maybe_highest_imported_gen_num,
            )
            .await?;
    }
    Ok(())
}

#[derive(Parser, Debug)]
#[clap(about = "Import a revlog-backed Mercurial repo into Mononoke blobstore.")]
struct MononokeBlobImportArgs {
    /// Input revlog repo
    input: PathBuf,

    /// If provided, the only changeset to be imported
    #[clap(long)]
    changeset: Option<String>,
    /// Skips commits from the beginning
    #[clap(long)]
    skip: Option<usize>,
    /// Import only LIMIT first commits from revlog repo
    #[clap(long)]
    commits_limit: Option<usize>,
    /// If provided then this manifold key will be updated with the next revision to import
    #[clap(long, requires = "manifold_bucket")]
    manifold_next_rev_to_import: Option<String>,
    /// Can only be used if --manifold-next-rev-to-import is set
    #[clap(long, requires = "manifold_next_rev_to_import")]
    manifold_bucket: Option<String>,
    /// If provided won't update bookmarks
    #[clap(long, conflicts_with = "prefix_bookmark")]
    no_bookmark: bool,
    /// If provided will update bookmarks, but prefix them with PREFIX
    #[clap(long)]
    prefix_bookmark: Option<String>,
    /// If provided, path to an executable that accepts OID SIZE and returns a LFS blob to stdout
    #[clap(long)]
    lfs_helper: Option<String>,
    /// If provided, max number of changesets to upload concurrently
    #[clap(long, default_value_t = 100)]
    concurrent_changesets: usize,
    /// If provided, max number of blobs to process concurrently
    #[clap(long, default_value_t = 100)]
    concurrent_blobs: usize,
    /// If provided, max number of LFS files to import concurrently
    #[clap(long, default_value_t = 10)]
    concurrent_lfs_imports: usize,
    /// File which fixes order or parents for commits in format 'HG_CS_ID P1_CS_ID [P2_CS_ID]'
    /// This is useful in case of merge commits - mercurial ignores order of parents of the merge commit
    /// while Mononoke doesn't ignore it. That might result in different bonsai hashes for the same
    /// Mercurial commit. Using --fix-parent-order allows to fix order of the parents.
    #[clap(long)]
    fix_parent_order: Option<PathBuf>,
    /// If provided will update globalrev
    #[clap(long)]
    has_globalrev: bool,
    /// If provided won't create a new repo (only meaningful for local)
    #[clap(long)]
    no_create: bool,
    /// Does not do any import. Just finds the rev that was already imported rev and
    /// updates manifold-next-rev-to-import if it's set. Note that we might have
    /// a situation where revision i is imported, i+1 is not and i+2 is imported.
    /// In that case this function would return i.
    #[clap(long)]
    find_already_imported_rev_only: bool,
    /// Derived data type to be backfilled. Note - 'filenodes' will always be derived unless excluded
    #[clap(long)]
    derived_data_type: Vec<String>,
    /// Exclude derived data types explicitly
    #[clap(long)]
    exclude_derived_data_type: Vec<String>,
    /// Numeric ID of backup source of truth mononoke repository (used only for backup jobs to sync bonsai changesets)
    #[clap(long, conflicts_with = "backup_from_repo_name")]
    backup_from_repo_id: Option<String>,
    /// Name of backup source of truth mononoke repository (used only for backup jobs to sync bonsai changesets)
    #[clap(long)]
    backup_from_repo_name: Option<String>,
    /// Numeric ID and Name of repository
    #[clap(flatten)]
    repo: RepoArgs,
    /// Numeric ID of source repository (used only for commands that operate on more than one repo)
    #[clap(long, conflicts_with = "source_repo_name")]
    source_repo_id: Option<String>,
    /// Name of source repository (used only for commands that operate on more than one repo)
    #[clap(long)]
    source_repo_name: Option<String>,
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let app = MononokeAppBuilder::new(fb)
        .with_app_extension(Fb303AppExtension {})
        .build::<MononokeBlobImportArgs>()?;
    app.run_with_monitoring_and_logging(async_main, "blobimport", AliveService)
}
