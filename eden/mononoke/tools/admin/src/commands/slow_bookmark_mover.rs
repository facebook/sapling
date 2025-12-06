/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::VecDeque;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::format_err;
use blobstore::Loadable;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateLog;
use bookmarks::BookmarkUpdateReason;
use bookmarks::Bookmarks;
use bookmarks::BookmarksRef;
use borrowed::borrowed;
use clap::Parser;
use clap::ValueEnum;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphRef;
use commit_id::CommitIdNames;
use commit_id::NamedCommitIdsArgs;
use commit_id::resolve_commit_id;
use commit_id::resolve_optional_commit_id;
use context::CoreContext;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use maplit::btreeset;
use mercurial_mutation::HgMutationStore;
use mononoke_app::MononokeApp;
use mononoke_app::args::RepoArgs;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mutable_counters::MutableCounters;
use phases::Phases;
use phases::PhasesRef;
use pushrebase_mutation_mapping::PushrebaseMutationMapping;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use time_measuring::speed_sliding_window::AvgTimeSlidingWindows;
use time_measuring::speed_sliding_window::BasicSpeedTracker;
use topo_sort::sort_topological_starting_with_heads;
use tracing::info;

#[derive(Copy, Clone, Debug)]
struct SlowBookmarkMoverCommitArgs;

impl CommitIdNames for SlowBookmarkMoverCommitArgs {
    const NAMES: &'static [(&'static str, &'static str)] = &[
        (
            "move-to-cs-id",
            "Hg/bonsai changeset id or bookmark that the move will finish on",
        ),
        (
            "start-cs-id",
            "Hg/bonsai changeset id that bookmark points to or pointed in the past setting this makes it possible to safely resume slow bookmark mover even if the changes involve merge commit imports.",
        ),
    ];
}

/// Tool to slowly advance bookmark to avoid overwhelming jobs doing processing
/// on bookmark moves.
///
/// There are many things that happen when public bookmarks are advanced.  For
/// each new commit we need to:
///  * derive the data
///  * (for some branches) index it in segmented changelog
///  * phabricator has to index those commits
///
/// This binary slowly advances bookmarks in small batches of commits, waiting
/// for phases.
///
/// This tool behaviour is very simple to understand for repos with linear history.
/// For repos with merges in them the behaviour is a bit more complex:
///  * the bookmark may be moved between different branches before moving past merge commit
///  * if possible the tool sticks to doing imports on a single branch before moving the other one
///  * it is essential to provide --start-cs-id to indicate starting position when resuming the merge
///    this allows the tool to generate exactly the same visit order and no to skip any branches.
#[derive(Parser)]
#[clap(verbatim_doc_comment)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(flatten)]
    commit_ids_args: NamedCommitIdsArgs<SlowBookmarkMoverCommitArgs>,

    /// Bookmark that will be moved
    #[clap(long)]
    bookmark_to_move: String,
    /// How many commits to sync in one go
    #[clap(long, default_value_t = 100)]
    commits_limit: usize,
    /// How many bookmark moves can proceed before waiting for replication of the first one.
    /// WARNING! Doesn't work with moving across merges *and* waiting segmented changelog.
    #[clap(long, default_value_t = 1)]
    window_size: usize,
    /// Soft limit on the number of changed files that will be synced to hg
    /// servers in one go. It's guaranteed that it won't try sync more than
    /// (limit + num changed files in a single commit)
    #[clap(long, default_value_t = 10000)]
    files_limit: usize,
    /// What should we wait for before advancing to next batch of bookmarks (default: all)
    #[clap(long, value_enum)]
    wait_for: Option<Vec<WaitFor>>,
}

#[facet::container]
#[derive(Clone)]
pub struct Repo {
    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    repo_derived_data: RepoDerivedData,

    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    bonsai_svnrev_mapping: dyn BonsaiSvnrevMapping,

    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    commit_graph: CommitGraph,

    #[facet]
    bookmark_update_log: dyn BookmarkUpdateLog,

    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    phases: dyn Phases,

    #[facet]
    pushrebase_mutation_mapping: dyn PushrebaseMutationMapping,

    #[facet]
    mutable_counters: dyn MutableCounters,

    #[facet]
    hg_mutation_store: dyn HgMutationStore,
}

#[derive(ValueEnum, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
enum WaitFor {
    Phases,
}

#[derive(PartialEq, Eq, Hash)]
enum Trackers {
    Phases,
}

impl std::fmt::Display for Trackers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Phases => write!(f, "Phases"),
        }
    }
}

async fn load_commits_from_single_target<'a>(
    ctx: &'a CoreContext,
    repo: &'a Repo,
    cur_bookmark: ChangesetId,
    start_cs_id: Option<ChangesetId>,
    cs_id: ChangesetId,
) -> Result<impl Stream<Item = Result<BonsaiChangeset>> + 'a> {
    let start_cs_id = start_cs_id.unwrap_or(cur_bookmark);
    info!("Destination cs_id resolved to {}", cs_id);
    let heads = vec![start_cs_id, cs_id];

    let mut bcss = repo
        .commit_graph()
        .ancestors_difference_stream(ctx, vec![cs_id], vec![start_cs_id])
        .await?
        .map_ok({
            |cs_id| async move {
                let cs = cs_id.load(ctx, repo.repo_blobstore()).await?;
                Ok((cs_id, cs))
            }
        })
        .try_buffered(100)
        .try_collect::<BTreeMap<_, _>>()
        .await?;
    info!("Loaded {} commits", bcss.len());

    // We use sort_topological which provides stable, DFS-based sorting algo for commits
    // This is good for ensuring we cover merges in reasonable order.
    let graph: BTreeMap<_, _> = bcss
        .iter()
        .map(|(cs_id, cs)| (cs_id.clone(), cs.parents().collect::<Vec<_>>()))
        .collect();
    let ordered_changesets: Vec<_> = sort_topological_starting_with_heads(&graph, &heads)
        .expect("unexpected cycle in commit graph!")
        .into_iter()
        // This is essential for resuming updates involving merge commits: we ensure that
        // we skip over all previously visited changesets.
        .skip_while(|cs_id| *cs_id != cur_bookmark)
        .skip(1)
        // We need to filter because the topo sort order includes the parents of the commits we asked for too.
        .filter_map(|cs_id| bcss.remove(&cs_id))
        .collect();

    Ok(stream::iter(ordered_changesets).map(anyhow::Ok))
}

/// Yields commits that need to be moved, in order.
async fn load_commits<'a>(
    ctx: &'a CoreContext,
    repo: &'a Repo,
    args: &'a CommandArgs,
    cur_bookmark: ChangesetId,
) -> Result<impl Stream<Item = Result<BonsaiChangeset>> + 'a> {
    let move_to_cs_id = resolve_commit_id(
        ctx,
        &repo,
        args.commit_ids_args
            .named_commit_ids()
            .get("move-to-cs-id")
            .ok_or(anyhow!("move-to-cs-id is required"))?,
    )
    .await?;
    let start_cs_id = resolve_optional_commit_id(
        ctx,
        &repo,
        args.commit_ids_args.named_commit_ids().get("start-cs-id"),
    )
    .await?;
    load_commits_from_single_target(ctx, repo, cur_bookmark, start_cs_id, move_to_cs_id).await
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();
    let repo: Repo = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;
    borrowed!(ctx, repo);

    let wait_for: BTreeSet<WaitFor> = match &args.wait_for {
        Some(wait_for) => wait_for.iter().copied().collect(),
        None => btreeset! { WaitFor::Phases },
    };

    let bookmark = BookmarkKey::new(&args.bookmark_to_move)?;

    let maybe_bookmark_val = repo
        .bookmarks()
        .get(ctx.clone(), &bookmark, bookmarks::Freshness::MostRecent)
        .await?;
    let bookmark_val =
        maybe_bookmark_val.ok_or_else(|| format_err!("{} bookmark is not set", bookmark))?;
    info!(
        "Previous bookmark value {}. Waiting for it to catch up.",
        bookmark_val
    );

    wait_for_bookmark_to_catch_up(ctx, repo, bookmark_val, &wait_for, None).await?;

    info!("Loading ALL commits that need syncing");
    // Find all commits that need syncing
    let mut ordered_changesets = load_commits(ctx, repo, &args, bookmark_val).await?;

    info!("Starting to move bookmark");
    // Start syncing them
    let mut cur_num_files = 0;
    let mut cur_num_commits = 0;
    let mut old_bookmark_value = bookmark_val;
    let mut commit_tracker = BasicSpeedTracker::start();
    let mut file_count_tracker = BasicSpeedTracker::start();
    let catch_up_tracker = AvgTimeSlidingWindows::start(Duration::from_hours(1));
    let mut wait_window = VecDeque::new();
    let mut last_cs_id = None;
    while let Some(cs) = ordered_changesets.try_next().await? {
        last_cs_id = Some(cs.get_changeset_id());
        cur_num_files += cs.file_changes_map().len();
        cur_num_commits += 1;

        if cur_num_files >= args.files_limit || cur_num_commits >= args.commits_limit {
            let new_value = move_bookmark(
                ctx,
                repo,
                &bookmark,
                old_bookmark_value,
                cs.get_changeset_id(),
            )
            .await?;
            wait_window.push_back(new_value);

            if wait_window.len() >= args.window_size {
                let new_value = wait_window.pop_front().unwrap();
                wait_for_bookmark_to_catch_up(
                    ctx,
                    repo,
                    new_value,
                    &wait_for,
                    Some(&catch_up_tracker),
                )
                .await?;
            }
            commit_tracker.add_entries(cur_num_commits);
            file_count_tracker.add_entries(cur_num_files);
            info!(
                "Advanced {} commits. Speed sliding windows: {}",
                cur_num_commits,
                commit_tracker.human_readable()
            );
            info!(
                "Also advanced {} files. Speed sliding windows: {}",
                cur_num_files,
                file_count_tracker.human_readable()
            );
            info!("Other timings: {}", catch_up_tracker);
            cur_num_files = 0;
            cur_num_commits = 0;
            old_bookmark_value = cs.get_changeset_id();
        }
    }

    // Sync the final one
    if let Some(cs_id) = last_cs_id {
        if cs_id != old_bookmark_value {
            let new_value = move_bookmark(ctx, repo, &bookmark, old_bookmark_value, cs_id).await?;
            wait_window.push_back(new_value);
        }
    }
    while let Some(new_value) = wait_window.pop_front() {
        wait_for_bookmark_to_catch_up(ctx, repo, new_value, &wait_for, Some(&catch_up_tracker))
            .await?;
    }

    Ok(())
}

async fn wait_for_phases_to_catch_up(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
    sleep_duration: Duration,
) -> Result<()> {
    // Force first log
    let mut time_since_log = Duration::from_mins(1);

    loop {
        let public = repo
            .phases()
            .get_public(ctx, vec![cs_id], false /* ephemeral derive */)
            .await?;
        if !public.contains(&cs_id) {
            if time_since_log.as_secs() >= 60 {
                time_since_log = Duration::ZERO;
                info!(
                    "Waiting for {} to become public, sleeping for {:?}",
                    cs_id, sleep_duration,
                );
            }
            time_since_log += sleep_duration;
            tokio::time::sleep(sleep_duration).await;
        } else {
            break;
        }
    }

    Ok(())
}

async fn wait_for_bookmark_to_catch_up<'a>(
    ctx: &'a CoreContext,
    repo: &'a Repo,
    new_value: ChangesetId,
    wait_for: &'a BTreeSet<WaitFor>,
    catch_up_tracker: Option<&'a AvgTimeSlidingWindows<Trackers>>,
) -> Result<()> {
    let sleep_secs = Duration::from_secs(1);
    let now = Instant::now();
    if wait_for.contains(&WaitFor::Phases) {
        wait_for_phases_to_catch_up(ctx, repo, new_value, sleep_secs).await?;
        if let Some(tracker) = catch_up_tracker {
            tracker.add_entry(Trackers::Phases, now.elapsed());
        }
    }

    Ok(())
}

async fn move_bookmark<'a>(
    ctx: &'a CoreContext,
    repo: &'a Repo,
    bookmark: &'a BookmarkKey,
    old_value: ChangesetId,
    new_value: ChangesetId,
) -> Result<ChangesetId> {
    info!("moving {} to {} from {}", bookmark, new_value, old_value);
    let mut txn = repo.bookmarks().create_transaction(ctx.clone());
    txn.update(
        bookmark,
        new_value,
        old_value,
        BookmarkUpdateReason::ManualMove,
    )?;
    let res = txn.commit().await?.is_some();
    if !res {
        return Err(anyhow!("failed to move {} to {}", bookmark, new_value));
    }
    Ok(new_value)
}
