/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::format_err;
use anyhow::Error;
use backsyncer::format_counter as format_backsyncer_counter;
use blobstore::Loadable;
use blobstore_factory::MetadataSqlFactory;
use blobstore_factory::ReadOnlyStorage;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateLog;
use bookmarks::BookmarkUpdateLogRef;
use bookmarks::BookmarkUpdateReason;
use bookmarks::Bookmarks;
use bookmarks::BookmarksRef;
use bookmarks::Freshness;
use cached_config::ConfigStore;
use changesets_creation::save_changesets;
use clap_old::App;
use clap_old::Arg;
use clap_old::ArgMatches;
use clap_old::SubCommand;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use context::CoreContext;
use cross_repo_sync::create_commit_syncers;
use cross_repo_sync::CommitSyncContext;
use cross_repo_sync::CommitSyncer;
use cross_repo_sync::Large;
use cross_repo_sync::Small;
use cross_repo_sync::SubmoduleDeps;
use cross_repo_sync::Syncers;
use cross_repo_sync::CHANGE_XREPO_MAPPING_EXTRA;
use fbinit::FacebookInit;
use filenodes::Filenodes;
use filestore::FilestoreConfig;
use filestore::FilestoreConfigRef;
use futures::stream;
use futures::try_join;
use futures::TryFutureExt;
use live_commit_sync_config::CfgrLiveCommitSyncConfig;
use live_commit_sync_config::LiveCommitSyncConfig;
use maplit::btreemap;
use maplit::hashmap;
use maplit::hashset;
use metaconfig_types::CommitSyncConfigVersion;
use metaconfig_types::DefaultSmallToLargeCommitSyncPathAction;
use metaconfig_types::RepoConfig;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::GitLfs;
use mononoke_types::NonRootMPath;
use mononoke_types::RepositoryId;
use mutable_counters::MutableCounters;
use mutable_counters::MutableCountersRef;
use phases::Phases;
use pushrebase::do_pushrebase_bonsai;
use pushrebase::FAIL_PUSHREBASE_EXTRA;
use pushrebase_mutation_mapping::PushrebaseMutationMapping;
use pushredirect::SqlPushRedirectionConfigBuilder;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_bookmark_attrs::RepoBookmarkAttrs;
use repo_cross_repo::RepoCrossRepo;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use repo_identity::RepoIdentityRef;
use slog::info;
use slog::Logger;
use sorted_vector_map::sorted_vector_map;
use sql_query_config::SqlQueryConfig;

use crate::common::get_source_target_repos_and_mapping;
use crate::error::SubcommandError;

pub const CROSSREPO: &str = "crossrepo";
const AUTHOR_ARG: &str = "author";
const DATE_ARG: &str = "date";
const ONCALL_ARG: &str = "oncall";
const DUMP_MAPPING_LARGE_REPO_PATH_ARG: &str = "dump-mapping-large-repo-path";
const PREPARE_ROLLOUT_SUBCOMMAND: &str = "prepare-rollout";
const PUSHREDIRECTION_SUBCOMMAND: &str = "pushredirection";
const LARGE_REPO_BOOKMARK_ARG: &str = "large-repo-bookmark";
const CHANGE_MAPPING_VERSION_SUBCOMMAND: &str = "change-mapping-version";
const VIA_EXTRAS_ARG: &str = "via-extra";

const ARG_VERSION_NAME: &str = "version-name";

#[facet::container]
#[derive(Clone)]
pub struct Repo {
    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    pushrebase_mutation_mapping: dyn PushrebaseMutationMapping,

    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    bookmark_update_log: dyn BookmarkUpdateLog,

    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    repo_derived_data: RepoDerivedData,

    #[facet]
    mutable_counters: dyn MutableCounters,

    #[facet]
    filestore_config: FilestoreConfig,

    #[facet]
    filenodes: dyn Filenodes,

    #[facet]
    commit_graph: CommitGraph,

    #[facet]
    commit_graph_writer: dyn CommitGraphWriter,

    #[facet]
    phases: dyn Phases,

    #[facet]
    repo_bookmark_attrs: RepoBookmarkAttrs,

    #[facet]
    repo_cross_repo: RepoCrossRepo,

    #[facet]
    repo_config: RepoConfig,

    #[facet]
    sql_query_config: SqlQueryConfig,
}

pub async fn subcommand_crossrepo<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'_>,
    sub_m: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    let config_store = matches.config_store();

    let ctx = CoreContext::new_with_logger_and_client_info(
        fb,
        logger.clone(),
        ClientInfo::default_with_entry_point(ClientEntryPoint::MononokeAdmin),
    );
    match sub_m.subcommand() {
        (PUSHREDIRECTION_SUBCOMMAND, Some(sub_sub_m)) => {
            let source_repo_id =
                args::not_shardmanager_compatible::get_source_repo_id(config_store, matches)?;
            let live_commit_sync_config =
                get_live_commit_sync_config(&ctx, fb, matches, source_repo_id).await?;
            run_pushredirection_subcommand(
                fb,
                ctx,
                matches,
                sub_sub_m,
                config_store,
                live_commit_sync_config,
            )
            .await
        }
        _ => Err(SubcommandError::InvalidArgs),
    }
}

async fn run_pushredirection_subcommand<'a>(
    fb: FacebookInit,
    ctx: CoreContext,
    matches: &'a MononokeMatches<'_>,
    config_subcommand_matches: &'a ArgMatches<'a>,
    config_store: &ConfigStore,
    live_commit_sync_config: CfgrLiveCommitSyncConfig,
) -> Result<(), SubcommandError> {
    let (source_repo, target_repo, _mapping) =
        get_source_target_repos_and_mapping(fb, ctx.logger().clone(), matches).await?;

    let live_commit_sync_config: Arc<dyn LiveCommitSyncConfig> = Arc::new(live_commit_sync_config);

    match config_subcommand_matches.subcommand() {
        (PREPARE_ROLLOUT_SUBCOMMAND, Some(_sub_m)) => {
            let commit_syncer = get_large_to_small_commit_syncer(
                &ctx,
                source_repo,
                target_repo,
                live_commit_sync_config.clone(),
            )
            .await?;

            if live_commit_sync_config
                .push_redirector_enabled_for_public(
                    &ctx,
                    commit_syncer.get_small_repo().repo_identity().id(),
                )
                .await?
            {
                return Err(format_err!(
                    "not allowed to run {} if pushredirection is enabled",
                    PREPARE_ROLLOUT_SUBCOMMAND
                )
                .into());
            }

            let small_repo = commit_syncer.get_small_repo();
            let large_repo = commit_syncer.get_large_repo();
            let largest_id = large_repo
                .bookmark_update_log()
                .get_largest_log_id(ctx.clone(), Freshness::MostRecent)
                .await?
                .ok_or_else(|| anyhow!("No bookmarks update log entries for large repo"))?;

            let counter = format_backsyncer_counter(&large_repo.repo_identity().id());
            info!(
                ctx.logger(),
                "setting value {} to counter {} for repo {}",
                largest_id,
                counter,
                small_repo.repo_identity().id()
            );
            let res = small_repo
                .mutable_counters()
                .set_counter(
                    &ctx,
                    &counter,
                    largest_id.try_into().unwrap(),
                    None, // prev_value
                )
                .await?;

            if !res {
                return Err(anyhow!("failed to set backsyncer counter").into());
            }
            info!(ctx.logger(), "successfully updated the counter");

            Ok(())
        }
        (CHANGE_MAPPING_VERSION_SUBCOMMAND, Some(sub_m)) => {
            let commit_syncer = get_large_to_small_commit_syncer(
                &ctx,
                source_repo,
                target_repo,
                live_commit_sync_config.clone(),
            )
            .await?;

            if sub_m.is_present(VIA_EXTRAS_ARG) {
                change_mapping_via_extras(
                    &ctx,
                    matches,
                    sub_m,
                    &commit_syncer,
                    config_store,
                    &live_commit_sync_config,
                )
                .await?;
                return Ok(());
            }

            if live_commit_sync_config
                .push_redirector_enabled_for_public(
                    &ctx,
                    commit_syncer.get_small_repo().repo_identity().id(),
                )
                .await?
            {
                return Err(format_err!(
                    "not allowed to run {} if pushredirection is enabled",
                    CHANGE_MAPPING_VERSION_SUBCOMMAND
                )
                .into());
            }

            let large_bookmark = Large(
                sub_m
                    .value_of(LARGE_REPO_BOOKMARK_ARG)
                    .map(BookmarkKey::new)
                    .transpose()?
                    .ok_or_else(|| format_err!("{} is not specified", LARGE_REPO_BOOKMARK_ARG))?,
            );
            let small_bookmark = Small(
                commit_syncer.get_bookmark_renamer().await?(&large_bookmark).ok_or_else(|| {
                    format_err!("{} bookmark doesn't remap to small repo", large_bookmark)
                })?,
            );

            let large_repo = Large(commit_syncer.get_large_repo());
            let small_repo = Small(commit_syncer.get_small_repo());
            let large_bookmark_value =
                Large(get_bookmark_value(&ctx, &large_repo, &large_bookmark).await?);
            let small_bookmark_value =
                Small(get_bookmark_value(&ctx, &small_repo, &small_bookmark).await?);

            let mapping_version = sub_m
                .value_of(ARG_VERSION_NAME)
                .ok_or_else(|| format_err!("{} is not specified", ARG_VERSION_NAME))?;
            let mapping_version = CommitSyncConfigVersion(mapping_version.to_string());
            if !commit_syncer.version_exists(&mapping_version).await? {
                return Err(format_err!("{} version does not exist", mapping_version).into());
            }

            let dump_mapping_file = sub_m
                .value_of(DUMP_MAPPING_LARGE_REPO_PATH_ARG)
                .map(NonRootMPath::new)
                .transpose()?;

            let large_cs_id = create_commit_for_mapping_change(
                &ctx,
                sub_m,
                &large_repo,
                &small_repo,
                &large_bookmark_value,
                &mapping_version,
                MappingCommitOptions {
                    add_mapping_change_extra: false,
                    dump_mapping_file,
                },
                &commit_syncer,
                &live_commit_sync_config,
            )
            .await?;

            let maybe_rewritten_small_cs_id = commit_syncer
                .unsafe_always_rewrite_sync_commit(
                    &ctx,
                    large_cs_id.0,
                    Some(hashmap! {
                      large_bookmark_value.0.clone() => small_bookmark_value.0.clone(),
                    }),
                    &mapping_version,
                    CommitSyncContext::AdminChangeMapping,
                )
                .await?;

            let rewritten_small_cs_id = Small(maybe_rewritten_small_cs_id.ok_or_else(|| {
                format_err!("{} was rewritten into non-existent commit", large_cs_id)
            })?);

            let f1 = move_bookmark(
                &ctx,
                &large_repo,
                &large_bookmark,
                *large_bookmark_value,
                *large_cs_id,
            );

            let f2 = move_bookmark(
                &ctx,
                &small_repo,
                &small_bookmark,
                *small_bookmark_value,
                *rewritten_small_cs_id,
            );

            try_join!(f1, f2)?;

            Ok(())
        }
        _ => Err(SubcommandError::InvalidArgs),
    }
}

async fn change_mapping_via_extras<'a>(
    ctx: &CoreContext,
    matches: &'a MononokeMatches<'a>,
    sub_m: &'a ArgMatches<'a>,
    commit_syncer: &'a CommitSyncer<Repo>,
    config_store: &ConfigStore,
    live_commit_sync_config: &Arc<dyn LiveCommitSyncConfig>,
) -> Result<(), Error> {
    // XXX(mitrandir): remove this check once this mode works regardless of sync direction
    if !live_commit_sync_config
        .push_redirector_enabled_for_public(
            ctx,
            commit_syncer.get_small_repo().repo_identity().id(),
        )
        .await?
        && std::env::var("MONONOKE_ADMIN_ALWAYS_ALLOW_MAPPING_CHANGE_VIA_EXTRA").is_err()
    {
        return Err(format_err!(
            "not allowed to run {} if pushredirection is not enabled",
            CHANGE_MAPPING_VERSION_SUBCOMMAND
        ));
    }

    let small_repo = commit_syncer.get_small_repo();
    let large_repo = commit_syncer.get_large_repo();

    let (_, repo_config) =
        args::get_config_by_repoid(config_store, matches, large_repo.repo_identity().id())?;

    let large_bookmark = Large(
        sub_m
            .value_of(LARGE_REPO_BOOKMARK_ARG)
            .map(BookmarkKey::new)
            .transpose()?
            .ok_or_else(|| format_err!("{} is not specified", LARGE_REPO_BOOKMARK_ARG))?,
    );
    let large_bookmark_value = Large(get_bookmark_value(ctx, large_repo, &large_bookmark).await?);

    let mapping_version = sub_m
        .value_of(ARG_VERSION_NAME)
        .ok_or_else(|| format_err!("{} is not specified", ARG_VERSION_NAME))?;
    let mapping_version = CommitSyncConfigVersion(mapping_version.to_string());
    if !commit_syncer.version_exists(&mapping_version).await? {
        return Err(format_err!("{} version does not exist", mapping_version));
    }

    let dump_mapping_file = sub_m
        .value_of(DUMP_MAPPING_LARGE_REPO_PATH_ARG)
        .map(NonRootMPath::new)
        .transpose()?;
    let large_cs_id = create_commit_for_mapping_change(
        ctx,
        sub_m,
        &Large(large_repo),
        &Small(small_repo),
        &large_bookmark_value,
        &mapping_version,
        MappingCommitOptions {
            add_mapping_change_extra: true,
            dump_mapping_file,
        },
        commit_syncer,
        live_commit_sync_config,
    )
    .await?;

    let pushrebase_flags = &repo_config.pushrebase.flags;
    let pushrebase_hooks = bookmarks_movement::get_pushrebase_hooks(
        ctx,
        large_repo,
        &large_bookmark,
        &repo_config.pushrebase,
        None,
    )
    .await?;

    let bcs = large_cs_id
        .load(ctx, &large_repo.repo_blobstore().clone())
        .map_err(Error::from)
        .await?;
    let pushrebase_res = do_pushrebase_bonsai(
        ctx,
        large_repo,
        pushrebase_flags,
        &large_bookmark,
        &hashset![bcs],
        &pushrebase_hooks,
    )
    .map_err(Error::from)
    .await?;

    println!("{}", pushrebase_res.head);

    Ok(())
}

struct MappingCommitOptions {
    add_mapping_change_extra: bool,
    // Fine to have Option<NonRootMPath> in this case since this represents an Optional
    // path that may or may not be provided, i.e. None != Root path in this case
    dump_mapping_file: Option<NonRootMPath>,
}

async fn create_commit_for_mapping_change(
    ctx: &CoreContext,
    sub_m: &ArgMatches<'_>,
    large_repo: &Large<&Repo>,
    small_repo: &Small<&Repo>,
    parent: &Large<ChangesetId>,
    mapping_version: &CommitSyncConfigVersion,
    options: MappingCommitOptions,
    commit_syncer: &CommitSyncer<Repo>,
    live_commit_sync_config: &Arc<dyn LiveCommitSyncConfig>,
) -> Result<Large<ChangesetId>, Error> {
    let author = sub_m
        .value_of(AUTHOR_ARG)
        .ok_or_else(|| format_err!("{} is not specified", AUTHOR_ARG))?;

    let author_date = sub_m
        .value_of(DATE_ARG)
        .map_or_else(|| Ok(DateTime::now()), DateTime::from_rfc3339)?;

    let oncall = sub_m.value_of(ONCALL_ARG);
    let oncall_msg_part = oncall.map(|o| format!("\n\nOncall Short Name: {}\n", o));

    let commit_msg = format!(
        "Changing synced mapping version to {} for {}->{} sync{}",
        mapping_version,
        large_repo.repo_identity().name(),
        small_repo.repo_identity().name(),
        oncall_msg_part.as_deref().unwrap_or("")
    );

    let mut extras = sorted_vector_map! {
        FAIL_PUSHREBASE_EXTRA.to_string() => b"1".to_vec(),
    };
    if options.add_mapping_change_extra {
        extras.insert(
            CHANGE_XREPO_MAPPING_EXTRA.to_string(),
            mapping_version.0.clone().into_bytes(),
        );
    }

    let file_changes = create_file_changes(
        ctx,
        small_repo,
        large_repo,
        mapping_version,
        options,
        commit_syncer,
        live_commit_sync_config,
    )
    .await?;

    // Create an empty commit on top of large bookmark
    let bcs = BonsaiChangesetMut {
        parents: vec![parent.0.clone()],
        author: author.to_string(),
        author_date,
        message: commit_msg,
        hg_extra: extras,
        file_changes: file_changes.into(),
        ..Default::default()
    }
    .freeze()?;

    let large_cs_id = bcs.get_changeset_id();
    save_changesets(ctx, &large_repo.0, vec![bcs]).await?;

    Ok(Large(large_cs_id))
}

async fn create_file_changes(
    ctx: &CoreContext,
    small_repo: &Small<&Repo>,
    large_repo: &Large<&Repo>,
    mapping_version: &CommitSyncConfigVersion,
    options: MappingCommitOptions,
    commit_syncer: &CommitSyncer<Repo>,
    live_commit_sync_config: &Arc<dyn LiveCommitSyncConfig>,
) -> Result<BTreeMap<NonRootMPath, FileChange>, Error> {
    let mut file_changes = btreemap! {};
    if let Some(path) = options.dump_mapping_file {
        // This "dump-mapping-file" is going to be created in the large repo,
        // but this file needs to rewrite to a small repo as well. If it doesn't
        // rewrite to a small repo, then the whole mapping change commit isn't
        // going to exist in the small repo.

        let movers = commit_syncer.get_movers_by_version(mapping_version).await?;

        let mover = if commit_syncer.get_source_repo().repo_identity().id()
            == large_repo.repo_identity().id()
        {
            movers.mover
        } else {
            movers.reverse_mover
        };

        if mover(&path)?.is_none() {
            return Err(anyhow!(
                "cannot dump mapping to a file because path doesn't rewrite to a small repo"
            ));
        }

        // Now get the mapping and create json with it
        let commit_sync_config = live_commit_sync_config
            .get_commit_sync_config_by_version(large_repo.repo_identity().id(), mapping_version)
            .await?;

        let small_repo_sync_config = commit_sync_config
            .small_repos
            .get(&small_repo.repo_identity().id())
            .ok_or_else(|| {
                format_err!(
                    "small repo {} not found in {} mapping",
                    small_repo.repo_identity().id(),
                    mapping_version
                )
            })?;

        let default_prefix = match &small_repo_sync_config.default_action {
            DefaultSmallToLargeCommitSyncPathAction::Preserve => String::new(),
            DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(prefix) => prefix.to_string(),
        };

        let mut map = serde_json::Map::new();
        map.insert("default_prefix".to_string(), default_prefix.into());
        let mut map_overrides = serde_json::Map::new();
        for (key, value) in &small_repo_sync_config.map {
            map_overrides.insert(key.to_string(), value.to_string().into());
        }
        map.insert("overrides".to_string(), map_overrides.into());

        let content = (get_generated_string() + &serde_json::to_string_pretty(&map)?).into_bytes();
        let content = bytes::Bytes::from(content);
        let size = content.len() as u64;
        let content_metadata = filestore::store(
            large_repo.repo_blobstore(),
            *large_repo.filestore_config(),
            ctx,
            &filestore::StoreRequest::new(size),
            stream::once(async move { Ok(content) }),
        )
        .await?;

        let file_change = FileChange::tracked(
            content_metadata.content_id,
            FileType::Regular,
            size,
            None,
            GitLfs::FullContent,
        );

        file_changes.insert(path, file_change);
    }

    Ok(file_changes)
}

// Mark content as (at)generated to discourage people from modifying it
// manually.
// However split this so that this source file is not marked as generated
fn get_generated_string() -> String {
    "\x40generated by the megarepo bind, reach out to Source Control @ FB with any questions\n"
        .to_owned()
}

async fn get_bookmark_value(
    ctx: &CoreContext,
    repo: &Repo,
    bookmark: &BookmarkKey,
) -> Result<ChangesetId, Error> {
    let maybe_bookmark_value = repo.bookmarks().get(ctx.clone(), bookmark).await?;

    maybe_bookmark_value.ok_or_else(|| {
        format_err!(
            "{} is not found in {}",
            bookmark,
            repo.repo_identity().name()
        )
    })
}

async fn move_bookmark(
    ctx: &CoreContext,
    repo: &Repo,
    bookmark: &BookmarkKey,
    prev_value: ChangesetId,
    new_value: ChangesetId,
) -> Result<(), Error> {
    let mut book_txn = repo.bookmarks().create_transaction(ctx.clone());

    info!(
        ctx.logger(),
        "moving {} to {} in {}",
        bookmark,
        new_value,
        repo.repo_identity().name()
    );
    book_txn.update(
        bookmark,
        new_value,
        prev_value,
        BookmarkUpdateReason::ManualMove,
    )?;

    let res = book_txn.commit().await?.is_some();

    if res {
        Ok(())
    } else {
        Err(format_err!(
            "failed to move bookmark {} in {}",
            bookmark,
            repo.repo_identity().name()
        ))
    }
}

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    let prepare_rollout_subcommand = SubCommand::with_name(PREPARE_ROLLOUT_SUBCOMMAND)
        .about("command to prepare rollout of pushredirection");

    let change_mapping_version = SubCommand::with_name(CHANGE_MAPPING_VERSION_SUBCOMMAND)
        .about(
            "a command to change mapping version for a given bookmark. \
        Note that this command doesn't check that the working copies of source and target repo \
        are equivalent according to the new mapping. This needs to ensured before calling this command",
        )
        .arg(
            Arg::with_name(AUTHOR_ARG)
                .long(AUTHOR_ARG)
                .required(true)
                .takes_value(true)
                .help("Author of the commit that will change the mapping"),
        )
        .arg(
            Arg::with_name(DATE_ARG)
                .long(DATE_ARG)
                .required(false)
                .takes_value(true)
                .help("Date for the commit that will change the mapping (in RFC-3339 format)"),
        )
        .arg(
            Arg::with_name(ONCALL_ARG)
                .long(ONCALL_ARG)
                .required(false)
                .takes_value(true)
                .help("Oncall for the commit that will change the mapping"),
        )
        .arg(
            Arg::with_name(LARGE_REPO_BOOKMARK_ARG)
                .long(LARGE_REPO_BOOKMARK_ARG)
                .required(true)
                .takes_value(true)
                .help("bookmark in the large repo"),
        )
        .arg(
            Arg::with_name(ARG_VERSION_NAME)
                .long(ARG_VERSION_NAME)
                .required(true)
                .takes_value(true)
                .help("mapping version to change to"),
        )
        .arg(
            Arg::with_name(VIA_EXTRAS_ARG)
                .long(VIA_EXTRAS_ARG)
                .required(false)
                .takes_value(false)
                .help("change mapping via pushing a commit with a special extra set. \
                This should become a default method, but for now let's hide behind this arg")
        )
        .arg(
            Arg::with_name(DUMP_MAPPING_LARGE_REPO_PATH_ARG)
                .long(DUMP_MAPPING_LARGE_REPO_PATH_ARG)
                .required(false)
                .takes_value(true)
                .help("Path in the repo where new mapping version will be dumped.")
        );

    let pushredirection_subcommand = SubCommand::with_name(PUSHREDIRECTION_SUBCOMMAND)
        .about("helper commands to enable/disable pushredirection")
        .subcommand(prepare_rollout_subcommand)
        .subcommand(change_mapping_version);

    SubCommand::with_name(CROSSREPO).subcommand(pushredirection_subcommand)
}

async fn get_syncers<'a>(
    ctx: &'a CoreContext,
    source_repo: Repo,
    target_repo: Repo,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
) -> Result<Syncers<Repo>, Error> {
    let common_sync_config =
        live_commit_sync_config.get_common_config(source_repo.repo_identity().id())?;

    let (large_repo, small_repo) = if common_sync_config.large_repo_id
        == source_repo.repo_identity().id()
        && common_sync_config
            .small_repos
            .contains_key(&target_repo.repo_identity().id())
    {
        (source_repo, target_repo)
    } else if common_sync_config.large_repo_id == target_repo.repo_identity().id()
        && common_sync_config
            .small_repos
            .contains_key(&source_repo.repo_identity().id())
    {
        (target_repo, source_repo)
    } else {
        return Err(format_err!(
            "CommitSyncMapping incompatible with source repo {:?} and target repo {:?}",
            source_repo.repo_identity().id(),
            target_repo.repo_identity().id()
        ));
    };

    let submodule_deps = SubmoduleDeps::NotNeeded;

    create_commit_syncers(
        ctx,
        small_repo,
        large_repo,
        submodule_deps,
        live_commit_sync_config,
    )
}

async fn get_large_to_small_commit_syncer<'a>(
    ctx: &'a CoreContext,
    source_repo: Repo,
    target_repo: Repo,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
) -> Result<CommitSyncer<Repo>, Error> {
    Ok(
        get_syncers(ctx, source_repo, target_repo, live_commit_sync_config)
            .await?
            .large_to_small,
    )
}

async fn get_live_commit_sync_config<'a>(
    _ctx: &'a CoreContext,
    fb: FacebookInit,
    matches: &'a MononokeMatches<'_>,
    repo_id: RepositoryId,
) -> Result<CfgrLiveCommitSyncConfig, Error> {
    let config_store = matches.config_store();
    let mysql_options = matches.mysql_options();
    let (_, config) = args::get_config_by_repoid(config_store, matches, repo_id)?;
    let readonly_storage = ReadOnlyStorage(false);
    let sql_factory: MetadataSqlFactory = MetadataSqlFactory::new(
        fb,
        config.storage_config.metadata,
        mysql_options.clone(),
        readonly_storage,
    )
    .await?;
    let builder = sql_factory
        .open::<SqlPushRedirectionConfigBuilder>()
        .await?;
    let push_redirection_config = builder.build(Arc::new(SqlQueryConfig { caching: None }));
    CfgrLiveCommitSyncConfig::new(config_store, Arc::new(push_redirection_config))
}
