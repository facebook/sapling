/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![type_length_limit = "4522397"]
use anyhow::{format_err, Error};
use blobrepo::{save_bonsai_changesets, BlobRepo};
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use bookmarks::{BookmarkName, BookmarkUpdateLog, BookmarkUpdateReason, Freshness};
use cmdlib::args;
use cmdlib::helpers::block_execute;
use context::CoreContext;
use cross_repo_sync::rewrite_commit;
use derived_data_utils::derived_data_utils;
use fbinit::FacebookInit;
use futures::{
    compat::{Future01CompatExt, Stream01CompatExt},
    future::TryFutureExt,
    stream::{self, StreamExt, TryStreamExt},
};
use import_tools::{GitimportPreferences, GitimportTarget};
use manifest::ManifestOps;
use maplit::hashset;
use mercurial_types::{HgChangesetId, MPath};
use metaconfig_types::RepoConfig;
use mononoke_types::{BonsaiChangeset, BonsaiChangesetMut, ChangesetId, DateTime};
use movers::DefaultAction;
use mutable_counters::{MutableCounters, SqlMutableCounters};
use pushrebase::do_pushrebase_bonsai;
use serde::{Deserialize, Serialize};
use serde_json;
use slog::info;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::convert::TryInto;
use std::num::NonZeroUsize;
use std::path::Path;
use tokio::{fs, io::AsyncWriteExt, process, time};
use topo_sort::sort_topological;
use unbundle::get_pushrebase_hooks;

mod cli;

use crate::cli::{
    setup_app, ARG_BACKUP_HASHES_FILE_PATH, ARG_BATCH_SIZE, ARG_BOOKMARK_SUFFIX, ARG_CALL_SIGN,
    ARG_COMMIT_AUTHOR, ARG_COMMIT_DATE_RFC3339, ARG_COMMIT_MESSAGE, ARG_DEST_BOOKMARK,
    ARG_DEST_PATH, ARG_GIT_REPOSITORY_PATH, ARG_HG_SYNC_CHECK_DISABLED, ARG_PHAB_CHECK_DISABLED,
    ARG_SLEEP_TIME, ARG_X_REPO_CHECK_DISABLED,
};

const LATEST_REPLAYED_REQUEST_KEY: &'static str = "latest-replayed-request";

#[derive(Deserialize, Clone, Debug)]
struct GraphqlQueryObj {
    differential_commit_query: Vec<GraphqlCommitQueryObj>,
}
#[derive(Deserialize, Clone, Debug)]
struct GraphqlCommitQueryObj {
    results: GraphqlResultsObj,
}
#[derive(Deserialize, Clone, Debug)]
struct GraphqlResultsObj {
    nodes: Vec<GraphqlImportedObj>,
}
#[derive(Deserialize, Clone, Debug)]
struct GraphqlImportedObj {
    imported: bool,
}
#[derive(Debug, Serialize)]
struct GraphqlInputVariables {
    commit: String,
}
#[derive(Debug)]
struct CheckerFlags<'a> {
    phab_check_disabled: bool,
    x_repo_check_disabled: bool,
    hg_sync_check_disabled: bool,
    call_sign: Option<&'a str>,
}
#[derive(Clone, Debug)]
struct ChangesetArgs {
    pub author: String,
    pub message: String,
    pub datetime: DateTime,
}

async fn rewrite_file_paths(
    ctx: &CoreContext,
    repo: &BlobRepo,
    path: &Path,
    prefix: &str,
    backup_hashes_path: &str,
) -> Result<Vec<BonsaiChangeset>, Error> {
    let prefs = GitimportPreferences::default();
    let target = GitimportTarget::FullRepo;
    info!(ctx.logger(), "Started importing git commits to Mononoke");
    let import_map = import_tools::gitimport(ctx, repo, path, target, prefs).await?;
    info!(ctx.logger(), "Added commits to Mononoke");
    let mut remapped_parents: HashMap<ChangesetId, ChangesetId> = HashMap::new();
    let mover = movers::mover_factory(
        HashMap::new(),
        DefaultAction::PrependPrefix(MPath::new(prefix).unwrap()),
    )?;
    let mut bonsai_changesets = vec![];
    let mut index = 1;
    let map_size = import_map.len();
    // Save the hashes to a txt file as a backup. If we failed at deriving data types, we can
    // use the hashes to derive the commits manually.
    let mut file = fs::File::create(backup_hashes_path).await?;
    for (_id, (bcs_id, bcs)) in import_map {
        let bcs_mut = bcs.into_mut();
        let rewritten_bcs_opt = rewrite_commit(
            ctx.clone(),
            bcs_mut,
            &remapped_parents,
            mover.clone(),
            repo.clone(),
        )
        .await?;

        if let Some(rewritten_bcs_mut) = rewritten_bcs_opt {
            let rewritten_bcs = rewritten_bcs_mut.freeze()?;
            remapped_parents.insert(bcs_id, rewritten_bcs.get_changeset_id());
            info!(
                ctx.logger(),
                "Commit {}/{}: Remapped {:?} => {:?}",
                index,
                map_size,
                bcs_id,
                rewritten_bcs.get_changeset_id(),
            );
            let hash = format!("{}\n", rewritten_bcs.get_changeset_id());
            file.write_all(hash.as_bytes()).await?;
            bonsai_changesets.push(rewritten_bcs);
        }
        index += 1;
    }
    info!(ctx.logger(), "Saving bonsai changesets");
    save_bonsai_changesets(bonsai_changesets.clone(), ctx.clone(), repo.clone())
        .compat()
        .await?;
    info!(ctx.logger(), "Saved bonsai changesets");
    Ok(bonsai_changesets)
}

async fn derive_bonsais(
    ctx: &CoreContext,
    repo: &BlobRepo,
    shifted_bcs: &[BonsaiChangeset],
) -> Result<(), Error> {
    let derived_data_types = &repo.get_derived_data_config().derived_data_types;

    let len = derived_data_types.len();
    let mut derived_utils = vec![];
    for ty in derived_data_types {
        let utils = derived_data_utils(repo.clone(), ty)?;
        derived_utils.push(utils);
    }

    stream::iter(derived_utils)
        .map(Ok)
        .try_for_each_concurrent(len, |derived_util| async move {
            for bcs in shifted_bcs {
                let csid = bcs.get_changeset_id();
                derived_util
                    .derive(ctx.clone(), repo.clone(), csid)
                    .compat()
                    .map_ok(|_| ())
                    .await?;
            }
            Result::<(), Error>::Ok(())
        })
        .await
}

async fn move_bookmark(
    ctx: &CoreContext,
    repo: &BlobRepo,
    shifted_bcs: &[BonsaiChangeset],
    batch_size: usize,
    bookmark_suffix: &str,
    checker_flags: &CheckerFlags<'_>,
    sleep_time: u64,
    mutable_counters: &SqlMutableCounters,
) -> Result<(), Error> {
    info!(ctx.logger(), "Start moving the bookmark");
    if shifted_bcs.is_empty() {
        return Err(format_err!("There is no bonsai changeset present"));
    }

    let bookmark = BookmarkName::new(format!("repo_import_{}", bookmark_suffix))?;
    let first_bcs = match shifted_bcs.first() {
        Some(first) => first,
        None => {
            return Err(format_err!("There is no bonsai changeset present"));
        }
    };
    let mut old_csid = first_bcs.get_changeset_id();
    let mut transaction = repo.update_bookmark_transaction(ctx.clone());
    transaction.create(&bookmark, old_csid, BookmarkUpdateReason::ManualMove, None)?;
    if !transaction.commit().await? {
        return Err(format_err!("Logical failure while creating {:?}", bookmark));
    }
    info!(
        ctx.logger(),
        "Created bookmark {:?} pointing to {}", bookmark, old_csid
    );
    for chunk in shifted_bcs.chunks(batch_size) {
        transaction = repo.update_bookmark_transaction(ctx.clone());
        let curr_csid = match chunk.last() {
            Some(bcs) => bcs.get_changeset_id(),
            None => {
                return Err(format_err!("There is no bonsai changeset present"));
            }
        };
        transaction.update(
            &bookmark,
            curr_csid,
            old_csid,
            BookmarkUpdateReason::ManualMove,
            None,
        )?;

        if !transaction.commit().await? {
            return Err(format_err!("Logical failure while setting {:?}", bookmark));
        }
        info!(
            ctx.logger(),
            "Set bookmark {:?} to point to {:?}", bookmark, curr_csid
        );
        let hg_csid = repo
            .get_hg_from_bonsai_changeset(ctx.clone(), curr_csid)
            .compat()
            .await?;
        check_dependent_systems(
            &ctx,
            &repo,
            &checker_flags,
            hg_csid,
            sleep_time,
            &mutable_counters,
        )
        .await?;
        old_csid = curr_csid;
    }
    info!(ctx.logger(), "Finished moving the bookmark");
    Ok(())
}

async fn merge_imported_commit(
    ctx: &CoreContext,
    repo: &BlobRepo,
    shifted_bcs: &[BonsaiChangeset],
    dest_bookmark: &BookmarkName,
    changeset_args: ChangesetArgs,
) -> Result<ChangesetId, Error> {
    info!(
        ctx.logger(),
        "Merging the imported commits into given bookmark, {}", dest_bookmark
    );
    let master_cs_id = match repo
        .get_bonsai_bookmark(ctx.clone(), dest_bookmark)
        .compat()
        .await?
    {
        Some(id) => id,
        None => {
            return Err(format_err!(
                "Couldn't extract changeset id from bookmark: {}",
                dest_bookmark
            ))
        }
    };
    let master_leaf_entries = get_leaf_entries(&ctx, &repo, master_cs_id).await?;

    let imported_cs_id = match shifted_bcs.last() {
        Some(bcs) => bcs.get_changeset_id(),
        None => return Err(format_err!("There is no bonsai changeset present")),
    };
    let imported_leaf_entries = get_leaf_entries(&ctx, &repo, imported_cs_id).await?;

    let intersection: Vec<MPath> = imported_leaf_entries
        .intersection(&master_leaf_entries)
        .cloned()
        .collect();

    if intersection.len() > 0 {
        return Err(format_err!(
            "There are paths present in both parents: {:?} ...",
            intersection
        ));
    }

    info!(ctx.logger(), "Done checking path conflicts");

    info!(
        ctx.logger(),
        "Creating a merge bonsai changeset with parents: {}, {}", master_cs_id, imported_cs_id
    );

    let ChangesetArgs {
        author,
        message,
        datetime,
    } = changeset_args;

    let merged_cs = BonsaiChangesetMut {
        parents: vec![master_cs_id, imported_cs_id],
        author: author.clone(),
        author_date: datetime,
        committer: Some(author.to_string()),
        committer_date: Some(datetime),
        message: message.to_string(),
        extra: BTreeMap::new(),
        file_changes: BTreeMap::new(),
    }
    .freeze()?;

    let merged_cs_id = merged_cs.get_changeset_id();
    info!(
        ctx.logger(),
        "Created merge bonsai: {} and changeset: {:?}", merged_cs_id, merged_cs
    );

    save_bonsai_changesets(vec![merged_cs], ctx.clone(), repo.clone())
        .compat()
        .await?;
    info!(ctx.logger(), "Finished merging");
    Ok(merged_cs_id)
}

async fn push_merge_commit(
    ctx: &CoreContext,
    repo: &BlobRepo,
    merged_cs_id: ChangesetId,
    bookmark_to_merge_into: &BookmarkName,
    repo_config: &RepoConfig,
) -> Result<ChangesetId, Error> {
    info!(ctx.logger(), "Running pushrebase");

    let merged_cs = merged_cs_id.load(ctx.clone(), repo.blobstore()).await?;
    let pushrebase_flags = repo_config.pushrebase.flags;
    let pushrebase_hooks = get_pushrebase_hooks(&repo, &repo_config.pushrebase);

    let pushrebase_res = do_pushrebase_bonsai(
        &ctx,
        &repo,
        &pushrebase_flags,
        bookmark_to_merge_into,
        &hashset![merged_cs],
        None,
        &pushrebase_hooks,
    )
    .await?;

    let pushrebase_cs_id = pushrebase_res.head;
    info!(
        ctx.logger(),
        "Finished pushrebasing to {}", pushrebase_cs_id
    );
    Ok(pushrebase_cs_id)
}

async fn get_leaf_entries(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
) -> Result<HashSet<MPath>, Error> {
    let hg_cs_id = repo
        .get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
        .compat()
        .await?;
    let hg_cs = hg_cs_id.load(ctx.clone(), &repo.get_blobstore()).await?;
    hg_cs
        .manifestid()
        .list_leaf_entries(ctx.clone(), repo.get_blobstore())
        .compat()
        .map_ok(|(path, (_file_type, _filenode_id))| path)
        .try_collect::<HashSet<_>>()
        .await
}

async fn check_dependent_systems(
    ctx: &CoreContext,
    repo: &BlobRepo,
    checker_flags: &CheckerFlags<'_>,
    hg_csid: HgChangesetId,
    sleep_time: u64,
    mutable_counters: &SqlMutableCounters,
) -> Result<(), Error> {
    // if a check is disabled, we have already passed the check
    let mut passed_phab_check = checker_flags.phab_check_disabled;
    let mut _passed_x_repo_check = checker_flags.x_repo_check_disabled;
    let mut passed_hg_sync_check = checker_flags.hg_sync_check_disabled;

    let repo_id = repo.get_repoid();
    while !passed_phab_check {
        let call_sign = checker_flags.call_sign.as_ref().unwrap();
        passed_phab_check = phabricator_commit_check(&call_sign, &hg_csid).await?;
        if !passed_phab_check {
            info!(
                ctx.logger(),
                "Phabricator hasn't parsed commit: {:?}", hg_csid
            );
            time::delay_for(time::Duration::from_secs(sleep_time)).await;
        }
    }

    let largest_id = match repo
        .attribute_expected::<dyn BookmarkUpdateLog>()
        .get_largest_log_id(ctx.clone(), Freshness::MostRecent)
        .await?
    {
        Some(id) => id,
        None => return Err(format_err!("Couldn't fetch id from bookmarks update log")),
    };

    /*
        In mutable counters table we store the latest bookmark id replayed by mercurial with
        LATEST_REPLAYED_REQUEST_KEY key. We use this key to extract the latest replayed id
        and compare it with the largest bookmark log id after we move the bookmark.
        If the replayed id is larger or equal to the bookmark id, we can try to move the bookmark
        to the next batch of commits
    */
    while !passed_hg_sync_check {
        let mut_counters_value = match mutable_counters
            .get_counter(ctx.clone(), repo_id, LATEST_REPLAYED_REQUEST_KEY)
            .compat()
            .await?
        {
            Some(value) => value,
            None => {
                return Err(format_err!(
                    "Couldn't fetch the counter value from mutable_counters for repo_id {:?}",
                    repo_id
                ))
            }
        };
        passed_hg_sync_check = largest_id <= mut_counters_value.try_into().unwrap();
        if !passed_hg_sync_check {
            info!(
                ctx.logger(),
                "Waiting for {} to be replayed to hg, the latest replayed is {}",
                largest_id,
                mut_counters_value
            );
            time::delay_for(time::Duration::from_secs(sleep_time)).await;
        }
    }
    Ok(())
}

async fn phabricator_commit_check(call_sign: &str, hg_csid: &HgChangesetId) -> Result<bool, Error> {
    let commit_id = format!("r{}{}", call_sign, hg_csid);
    let query = "query($commit: String!) {
                    differential_commit_query(query_params:{commits:[$commit]}) {
                        results {
                            nodes {
                                imported
                            }
                        }
                    }
                }";
    let variables = serde_json::to_string(&GraphqlInputVariables { commit: commit_id }).unwrap();
    let output = process::Command::new("jf")
        .arg("graphql")
        .arg("--query")
        .arg(query)
        .arg("--variables")
        .arg(variables)
        .output()
        .await?;
    if !output.status.success() {
        let e = format_err!(
            "Failed to fetch graphql commit: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return Err(e);
    }
    let query: GraphqlQueryObj = serde_json::from_str(&String::from_utf8_lossy(&output.stdout))?;
    let first_query = match query.differential_commit_query.first() {
        Some(first) => first,
        None => {
            return Err(format_err!(
                "No results were found when checking phabricator"
            ));
        }
    };
    let nodes = &first_query.results.nodes;
    let imported = match nodes.first() {
        Some(imp_obj) => imp_obj.imported,
        None => return Ok(false),
    };
    Ok(imported)
}

fn is_valid_bookmark_suffix(bookmark_suffix: &str) -> bool {
    let spec_chars = "./-_";
    bookmark_suffix
        .chars()
        .all(|c| c.is_alphanumeric() || spec_chars.contains(c))
}

fn sort_bcs(shifted_bcs: &[BonsaiChangeset]) -> Result<Vec<BonsaiChangeset>, Error> {
    let mut bcs_parents = HashMap::new();
    let mut id_bcs = HashMap::new();
    for bcs in shifted_bcs {
        let parents: Vec<_> = bcs.parents().collect();
        let bcs_id = bcs.get_changeset_id();
        bcs_parents.insert(bcs_id, parents);
        id_bcs.insert(bcs_id, bcs);
    }

    let sorted_commits = sort_topological(&bcs_parents).expect("loop in commit chain!");
    let mut sorted_bcs: Vec<BonsaiChangeset> = vec![];
    for csid in sorted_commits {
        match id_bcs.get(&csid) {
            Some(&bcs) => sorted_bcs.push(bcs.clone()),
            _ => {
                return Err(format_err!(
                    "Could not find mapping for changeset id {}",
                    csid
                ))
            }
        }
    }
    Ok(sorted_bcs)
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = setup_app();
    let matches = app.get_matches();

    let path = Path::new(matches.value_of(ARG_GIT_REPOSITORY_PATH).unwrap());
    let prefix = matches.value_of(ARG_DEST_PATH).unwrap();
    let bookmark_suffix = matches.value_of(ARG_BOOKMARK_SUFFIX).unwrap();
    let batch_size = matches.value_of(ARG_BATCH_SIZE).unwrap();
    let batch_size = batch_size.parse::<NonZeroUsize>()?.get();
    if !is_valid_bookmark_suffix(&bookmark_suffix) {
        return Err(format_err!(
            "The bookmark suffix contains invalid character(s).
            You can only use alphanumeric and \"./-_\" characters"
        ));
    }

    let phab_check_disabled = matches.is_present(ARG_PHAB_CHECK_DISABLED);
    let x_repo_check_disabled = matches.is_present(ARG_X_REPO_CHECK_DISABLED);
    let hg_sync_check_disabled = matches.is_present(ARG_HG_SYNC_CHECK_DISABLED);
    let call_sign = matches.value_of(ARG_CALL_SIGN);
    if !phab_check_disabled && call_sign.is_none() {
        return Err(format_err!("Call sign was not specified"));
    }
    let checker_flags = CheckerFlags {
        phab_check_disabled,
        x_repo_check_disabled,
        hg_sync_check_disabled,
        call_sign,
    };
    let sleep_time = matches.value_of(ARG_SLEEP_TIME).unwrap();
    let sleep_time = sleep_time.parse::<u64>()?;
    let backup_hashes_path = matches.value_of(ARG_BACKUP_HASHES_FILE_PATH).unwrap();
    let dest_bookmark_name = matches.value_of(ARG_DEST_BOOKMARK).unwrap();
    let commit_author = matches.value_of(ARG_COMMIT_AUTHOR).unwrap();
    let commit_message = matches.value_of(ARG_COMMIT_MESSAGE).unwrap();
    let datetime = match matches.value_of(ARG_COMMIT_DATE_RFC3339) {
        Some(date) => DateTime::from_rfc3339(date)?,
        None => DateTime::now(),
    };
    let changeset_args = ChangesetArgs {
        author: commit_author.to_string(),
        message: commit_message.to_string(),
        datetime,
    };
    args::init_cachelib(fb, &matches, None);

    let logger = args::init_logging(fb, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let repo = args::create_repo(fb, &logger, &matches);
    block_execute(
        async {
            let repo = repo.compat().await?;
            let mutable_counters = args::open_sql::<SqlMutableCounters>(ctx.fb, &matches)
                .compat()
                .await?;
            let mut shifted_bcs =
                rewrite_file_paths(&ctx, &repo, &path, &prefix, &backup_hashes_path).await?;
            shifted_bcs = sort_bcs(&shifted_bcs)?;
            info!(ctx.logger(), "Start deriving data types");
            derive_bonsais(&ctx, &repo, &shifted_bcs).await?;
            info!(ctx.logger(), "Finished deriving data types");
            move_bookmark(
                &ctx,
                &repo,
                &shifted_bcs,
                batch_size,
                &bookmark_suffix,
                &checker_flags,
                sleep_time,
                &mutable_counters,
            )
            .await?;
            let dest_bookmark = BookmarkName::new(dest_bookmark_name)?;
            let merged_cs_id =
                merge_imported_commit(&ctx, &repo, &shifted_bcs, &dest_bookmark, changeset_args)
                    .await?;
            let (_, repo_config) = args::get_config_by_repoid(ctx.fb, &matches, repo.get_repoid())?;
            push_merge_commit(&ctx, &repo, merged_cs_id, &dest_bookmark, &repo_config).await?;
            Ok(())
        },
        fb,
        "repo_import",
        &logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}

#[cfg(test)]
mod tests {
    use crate::{
        check_dependent_systems, merge_imported_commit, move_bookmark, push_merge_commit, sort_bcs,
        ChangesetArgs, CheckerFlags, LATEST_REPLAYED_REQUEST_KEY,
    };

    use anyhow::Result;
    use blobstore::Loadable;
    use bookmarks::{BookmarkName, BookmarkUpdateLog, BookmarkUpdateReason, Freshness};
    use context::CoreContext;
    use fbinit::FacebookInit;
    use futures::{compat::Future01CompatExt, stream::TryStreamExt};
    use mercurial_types_mocks::nodehash::ONES_CSID as HG_CSID;
    use metaconfig_types::{PushrebaseParams, RepoConfig};
    use mononoke_types::{
        globalrev::{Globalrev, START_COMMIT_GLOBALREV},
        DateTime, RepositoryId,
    };
    use mononoke_types_mocks::changesetid::{ONES_CSID as MON_CSID, TWOS_CSID};
    use mutable_counters::{MutableCounters, SqlMutableCounters};
    use sql::rusqlite::Connection as SqliteConnection;
    use sql_construct::SqlConstruct;
    use std::time::Duration;
    use tests_utils::{bookmark, drawdag::create_from_dag, CreateCommitContext};
    use tokio::time;

    fn create_bookmark_name(book: &str) -> BookmarkName {
        BookmarkName::new(book.to_string()).unwrap()
    }

    #[fbinit::compat_test]
    async fn move_bookmark_test(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let blob_repo = blobrepo_factory::new_memblob_empty(None)?;
        let batch_size: usize = 2;
        let call_sign = Some("FBS");
        let checker_flags = CheckerFlags {
            phab_check_disabled: true,
            x_repo_check_disabled: true,
            hg_sync_check_disabled: true,
            call_sign,
        };
        let sleep_time = 1;
        let mutable_counters = SqlMutableCounters::with_sqlite_in_memory().unwrap();
        let changesets = create_from_dag(
            &ctx,
            &blob_repo,
            r##"
                A-B-C-D-E-F-G
            "##,
        )
        .await?;
        let mut bonsais = vec![];
        for (_, csid) in &changesets {
            bonsais.push(csid.load(ctx.clone(), &blob_repo.get_blobstore()).await?);
        }
        bonsais = sort_bcs(&bonsais)?;
        move_bookmark(
            &ctx,
            &blob_repo,
            &bonsais,
            batch_size,
            "test_repo",
            &checker_flags,
            sleep_time,
            &mutable_counters,
        )
        .await?;
        // Check the bookmark moves created BookmarkLogUpdate entries
        let entries = blob_repo
            .attribute_expected::<dyn BookmarkUpdateLog>()
            .list_bookmark_log_entries(
                ctx.clone(),
                BookmarkName::new("repo_import_test_repo")?,
                5,
                None,
                Freshness::MostRecent,
            )
            .map_ok(|(cs, rs, _ts)| (cs, rs)) // dropping timestamps
            .try_collect::<Vec<_>>()
            .await?;

        assert_eq!(
            entries,
            vec![
                (Some(changesets["G"]), BookmarkUpdateReason::ManualMove),
                (Some(changesets["F"]), BookmarkUpdateReason::ManualMove),
                (Some(changesets["D"]), BookmarkUpdateReason::ManualMove),
                (Some(changesets["B"]), BookmarkUpdateReason::ManualMove),
                (Some(changesets["A"]), BookmarkUpdateReason::ManualMove),
            ]
        );
        Ok(())
    }

    /*
                        largest_id      mutable_counters value   assert
        No action       None            None                     Error
        Move bookmark   1               None                     Error
        Set counter     1               1                        Ok(())
        Move bookmark   2               1                        inifite loop -> timeout
        Set counter     2               2                        Ok(())
    */
    #[fbinit::compat_test]
    async fn hg_sync_check_test(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo = blobrepo_factory::new_memblob_empty(None)?;
        let checker_flags = CheckerFlags {
            phab_check_disabled: true,
            x_repo_check_disabled: true,
            hg_sync_check_disabled: false,
            call_sign: None,
        };
        let sleep_time = 1;
        let mutable_counters = SqlMutableCounters::with_sqlite_in_memory().unwrap();
        let repo_id = repo.get_repoid();
        let bookmark = create_bookmark_name("book");

        assert!(check_dependent_systems(
            &ctx,
            &repo,
            &checker_flags,
            HG_CSID,
            sleep_time,
            &mutable_counters
        )
        .await
        .is_err());

        let mut txn = repo.update_bookmark_transaction(ctx.clone());
        txn.create(&bookmark, MON_CSID, BookmarkUpdateReason::TestMove, None)?;
        txn.commit().await.unwrap();
        assert!(check_dependent_systems(
            &ctx,
            &repo,
            &checker_flags,
            HG_CSID,
            sleep_time,
            &mutable_counters
        )
        .await
        .is_err());

        mutable_counters
            .set_counter(ctx.clone(), repo_id, LATEST_REPLAYED_REQUEST_KEY, 1, None)
            .compat()
            .await?;

        check_dependent_systems(
            &ctx,
            &repo,
            &checker_flags,
            HG_CSID,
            sleep_time,
            &mutable_counters,
        )
        .await?;

        let mut txn = repo.update_bookmark_transaction(ctx.clone());
        txn.update(
            &bookmark,
            TWOS_CSID,
            MON_CSID,
            BookmarkUpdateReason::TestMove,
            None,
        )?;
        txn.commit().await.unwrap();

        let timed_out = time::timeout(
            Duration::from_millis(2000),
            check_dependent_systems(
                &ctx,
                &repo,
                &checker_flags,
                HG_CSID,
                sleep_time,
                &mutable_counters,
            ),
        )
        .await
        .is_err();
        assert!(timed_out);

        mutable_counters
            .set_counter(ctx.clone(), repo_id, LATEST_REPLAYED_REQUEST_KEY, 2, None)
            .compat()
            .await?;

        check_dependent_systems(
            &ctx,
            &repo,
            &checker_flags,
            HG_CSID,
            sleep_time,
            &mutable_counters,
        )
        .await?;
        Ok(())
    }

    #[fbinit::compat_test]
    async fn merge_push_commit_test(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let (repo, _con) = blobrepo_factory::new_memblob_with_sqlite_connection_with_id(
            SqliteConnection::open_in_memory()?,
            RepositoryId::new(1),
        )?;

        let master_cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("a", "a")
            .commit()
            .await?;
        let imported_cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("b", "b")
            .commit()
            .await?;
        let imported_cs = imported_cs_id.load(ctx.clone(), repo.blobstore()).await?;

        let dest_bookmark = bookmark(&ctx, &repo, "master").set_to(master_cs_id).await?;

        let changeset_args = ChangesetArgs {
            author: "user".to_string(),
            message: "merging".to_string(),
            datetime: DateTime::now(),
        };

        let merged_cs_id = merge_imported_commit(
            &ctx,
            &repo,
            &[imported_cs.clone()],
            &dest_bookmark,
            changeset_args,
        )
        .await?;

        let mut repo_config = RepoConfig::default();
        repo_config.pushrebase = PushrebaseParams {
            assign_globalrevs: true,
            ..Default::default()
        };

        let pushed_cs_id =
            push_merge_commit(&ctx, &repo, merged_cs_id, &dest_bookmark, &repo_config).await?;
        let pushed_cs = pushed_cs_id.load(ctx.clone(), repo.blobstore()).await?;

        assert_eq!(
            Globalrev::new(START_COMMIT_GLOBALREV),
            Globalrev::from_bcs(&pushed_cs)?
        );
        Ok(())
    }
}
