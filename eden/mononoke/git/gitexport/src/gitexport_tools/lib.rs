/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod partial_commit_graph;
use std::collections::HashMap;
use std::env;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use blobstore::Loadable;
use blobstore::PutBehaviour;
use borrowed::borrowed;
use commit_transformation::rewrite_commit;
use commit_transformation::upload_commits;
use commit_transformation::MultiMover;
use fbinit::FacebookInit;
use fileblob::Fileblob;
use futures::stream::TryStreamExt;
use futures::stream::{self};
use futures::StreamExt;
use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use mononoke_api::BookmarkKey;
use mononoke_api::ChangesetContext;
use mononoke_api::CoreContext;
use mononoke_api::MononokeError;
use mononoke_api::RepoContext;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::NonRootMPath;
use rand::distributions::Alphanumeric;
use rand::distributions::DistString;
use repo_blobstore::RepoBlobstoreArc;
use slog::debug;
use slog::error;
use slog::info;
use slog::trace;
use slog::warn;
use slog::Drain;
use slog::Logger;
use sql::rusqlite::Connection as SqliteConnection;
use test_repo_factory::TestRepoFactory;

pub use crate::partial_commit_graph::build_partial_commit_graph_for_export;
use crate::partial_commit_graph::ChangesetParents;
use crate::partial_commit_graph::ExportPathInfo;
pub use crate::partial_commit_graph::GitExportGraphInfo;

pub const MASTER_BOOKMARK: &str = "master";

/// Given a list of changesets, their parents and a list of paths, create
/// copies in a target mononoke repository containing only changes that
/// were made on the given paths.
pub async fn rewrite_partial_changesets(
    fb: FacebookInit,
    source_repo_ctx: RepoContext,
    graph_info: GitExportGraphInfo,
    export_paths: Vec<ExportPathInfo>,
) -> Result<RepoContext> {
    let ctx: &CoreContext = source_repo_ctx.ctx();
    let changesets = graph_info.changesets;
    let changeset_parents = &graph_info.parents_map;
    let logger = ctx.logger();

    info!(logger, "Copying changesets to temporary repo...");

    debug!(logger, "export_paths: {:#?}", &export_paths);

    // Repo that will hold the partial changesets that will be exported to git
    let temp_repo_ctx = create_temp_repo(fb, ctx).await?;

    let num_changesets = changesets.len().try_into().unwrap();
    let cs_results: Vec<Result<ChangesetContext, MononokeError>> =
        changesets.into_iter().map(Ok).collect::<Vec<_>>();

    let mb_progress_bar = if logger.is_enabled(slog::Level::Info) {
        let progress_bar = ProgressBar::new(num_changesets)
        .with_message("Copying changesets")
        .with_style(
            ProgressStyle::with_template(
                "[{percent}%] {msg} [{bar:60.cyan}] (ETA: {eta}) ({human_pos}/{human_len}) ({per_sec}) ",
            )?
            .progress_chars("#>-"),
        );
        progress_bar.enable_steady_tick(std::time::Duration::from_secs(5));
        Some(progress_bar)
    } else {
        None
    };

    let (new_bonsai_changesets, _) = stream::iter(cs_results)
        .try_fold(
            (Vec::new(), HashMap::new()),
            |(mut new_bonsai_changesets, remapped_parents), changeset| {
                borrowed!(source_repo_ctx);
                borrowed!(mb_progress_bar);
                borrowed!(export_paths);
                borrowed!(logger);

                async move {
                    let export_paths =
                        get_export_paths_for_changeset(&changeset, export_paths).await?;

                    let multi_mover = build_multi_mover_for_changeset(logger, &export_paths)?;
                    let (new_bcs, remapped_parents) = create_bonsai_for_new_repo(
                        source_repo_ctx,
                        multi_mover,
                        changeset_parents,
                        remapped_parents,
                        changeset,
                        &export_paths,
                    )
                    .await?;
                    new_bonsai_changesets.push(new_bcs);
                    if let Some(progress_bar) = mb_progress_bar {
                        progress_bar.inc(1);
                    }
                    Ok((new_bonsai_changesets, remapped_parents))
                }
            },
        )
        .await?;

    trace!(
        logger,
        "new_bonsai_changesets: {:#?}",
        &new_bonsai_changesets
    );

    let head_cs_id = new_bonsai_changesets
        .last()
        .ok_or(Error::msg("No changesets were moved"))?
        .get_changeset_id();

    debug!(logger, "Uploading copied changesets...");
    upload_commits(
        source_repo_ctx.ctx(),
        new_bonsai_changesets,
        source_repo_ctx.repo(),
        temp_repo_ctx.repo(),
    )
    .await?;

    // Set master bookmark to point to the latest changeset
    if let Err(err) = temp_repo_ctx
        .create_bookmark(&BookmarkKey::from_str(MASTER_BOOKMARK)?, head_cs_id, None)
        .await
    {
        // TODO(T161902005): stop failing silently on bookmark creation
        error!(logger, "Failed to create master bookmark: {:?}", err);
    }

    info!(logger, "Finished copying all changesets!");
    Ok(temp_repo_ctx)
}

/// Given a changeset and a set of paths being exported, build the
/// BonsaiChangeset containing only the changes to those paths.
async fn create_bonsai_for_new_repo<'a>(
    source_repo_ctx: &RepoContext,
    multi_mover: MultiMover<'_>,
    changeset_parents: &ChangesetParents,
    mut remapped_parents: HashMap<ChangesetId, ChangesetId>,
    changeset_ctx: ChangesetContext,
    export_paths: &'a [&'a NonRootMPath],
) -> Result<(BonsaiChangeset, HashMap<ChangesetId, ChangesetId>), MononokeError> {
    let logger = changeset_ctx.repo().ctx().logger();
    trace!(
        logger,
        "Rewriting changeset: {:#?} | {:#?}",
        &changeset_ctx.id(),
        &changeset_ctx.message().await?
    );

    let blobstore = source_repo_ctx.blob_repo().repo_blobstore_arc();
    let bcs: BonsaiChangeset = changeset_ctx
        .id()
        .load(source_repo_ctx.ctx(), &blobstore)
        .await
        .map_err(MononokeError::from)?;

    // The BonsaiChangeset was built with the full graph, so the parents are
    // not necessarily part of the provided changesets. So we first need to
    // update the parents to reference the parents from the partial graph.
    //
    // These values will be used to read the `remapped_parents` map in
    // `rewrite_commit` and get the correct changeset id for the parents in
    // the new bonsai changeset.
    let orig_parent_ids: Vec<ChangesetId> = changeset_parents
        .get(&bcs.get_changeset_id())
        .cloned()
        .unwrap_or_default();
    let mut mut_bcs = bcs.into_mut();
    mut_bcs.parents = orig_parent_ids.clone();

    // If this changeset created an export directory by copying/moving files
    // from a directory that's not being exported, we only need to print a
    // warning once and not for every single file that was copied.
    let mut printed_warning_about_copy = false;

    // TODO(T161204758): iterate over all parents and select one that is the closest
    // ancestor of each commit in the `copy_from` field.
    mut_bcs.file_changes.iter_mut().for_each(|(_p, fc)| {
        if let FileChange::Change(tracked_fc) = fc {
            // If any FileChange copies a file from a previous revision (e.g. a parent),
            // set the `copy_from` field to point to its new parent.
            //
            // Since we're building a history using all changesets that
            // affect the exported directories, any file being copied
            // should always exist in the new parent.
            //
            // If this isn't done, it might not be possible to rewrite the
            // commit to the new repo, because the changeset referenced in
            // its `copy_from` field will not have been remapped.
            if let Some((old_path, copy_from_cs_id)) = tracked_fc.copy_from_mut() {
                // If this isn't the first changeset (i.e. that creates the oldest exported
                // directory), we need to make sure that file changes that copy from
                // previous commits only reference revisions that are also being exported.
                //
                // We should also make sure that we only reference files that are
                // are in the revision that we'll set as the parent.
                // If that's not the case, the `copy_from` information will be
                // dropped and a warning will be printed to the user so they're
                // aware that files in an export directory might have been
                // copied/moved from another one.
                let old_path = &*old_path; // We need an immutable reference for prefix checks
                let should_export = export_paths.iter().any(|p| p.is_prefix_of(old_path));
                let new_parent_cs_id = orig_parent_ids.first();
                if new_parent_cs_id.is_some() && should_export {
                    *copy_from_cs_id = new_parent_cs_id.cloned().unwrap();
                } else {
                    // First commit where the export paths were created.
                    // If the `copy_from` field is set, it means that some files
                    // were copied from other directories, which might not be
                    // included in the provided export paths.
                    //
                    // By default, these renames won't be followed, but a warning
                    // will be printed so that the user can check the commit
                    // and decide if they want to re-run it passing the old
                    // name as an export path along with this commit as the
                    // head changeset.
                    if !printed_warning_about_copy {
                        warn!(
                            logger,
                            "Changeset {0:?} might have created one of the exported paths by moving/copying files from a previous commit that will not be exported (id {1:?}).",
                            changeset_ctx.id(),
                            copy_from_cs_id
                        );
                        printed_warning_about_copy =  true;
                    };
                    *tracked_fc =  tracked_fc.with_new_copy_from(None);
                };
            };
        };
    });

    let rewritten_bcs_mut = rewrite_commit(
        source_repo_ctx.ctx(),
        mut_bcs,
        &remapped_parents,
        multi_mover,
        source_repo_ctx.repo(),
        None,
        Default::default(),
    )
    .await?
    // This shouldn't happen because every changeset provided is modifying
    // at least one of the exported files.
    .ok_or(Error::msg(
        "Commit wasn't rewritten because it had no signficant changes",
    ))?;

    let rewritten_bcs = rewritten_bcs_mut.freeze()?;

    // Update the remapped_parents map so that children of this commit use
    // its new id when moved.
    remapped_parents.insert(changeset_ctx.id(), rewritten_bcs.get_changeset_id());

    Ok((rewritten_bcs, remapped_parents))
}

/// Builds a vector of references to the paths that should be exported when
/// rewriting the provided changeset based on each export path's head commit.
async fn get_export_paths_for_changeset<'a>(
    processed_cs: &ChangesetContext,
    export_path_infos: &'a Vec<ExportPathInfo>,
) -> Result<Vec<&'a NonRootMPath>> {
    // Get the export paths for the changeset being processed considering its
    // head commit.
    let export_paths: Vec<&NonRootMPath> = stream::iter(export_path_infos)
        .then(|(exp_path, head_cs)| async move {
            // If the processed changeset is older than a path's head commit,
            // then this path should be exported when rewriting this changeset.
            let is_ancestor_of_head_cs = processed_cs.is_ancestor_of(head_cs.id()).await?;
            if is_ancestor_of_head_cs {
                return anyhow::Ok::<Option<&NonRootMPath>>(Some(exp_path));
            }
            // Otherwise the changeset is a descendant of this path's head, so
            // the path should NOT be exported.
            Ok(None)
        })
        .try_collect::<Vec<_>>()
        .await?
        .into_iter()
        .flatten()
        .collect::<Vec<&NonRootMPath>>();

    Ok(export_paths)
}

/// The MultiMover closure is called for every path affected by a commit being
/// copied and it should determine if the changes to the path should be
/// included or not in the new commit.
///
/// This function builds the MultiMover for a given changeset, considering what
/// paths should be exported based on their head.
/// This is needed to handle renames of export directories.
/// For details about head changeset, see the docs of `ExportPathInfo`.
///
/// **Example:**
/// `A -> B -> c -> D -> E -> f (master)`
/// A) CREATE: old/foo
/// B) MODIFY: old/foo
/// c) MODIFY: random/file.txt
/// D) RENAME: old/foo → new/foo
/// E) MODIFY: new/foo.txt
/// f) CREATE: old/foo.
///
/// Expected changesets: [A', B', D', E'], because the `old` directory created
/// in `f` should NOT be exported.
///
/// In this case, `export_path_infos` would be `[("new", "f", ("old", "D")]`.
///
fn build_multi_mover_for_changeset<'a>(
    logger: &'a Logger,
    export_paths: &'a [&'a NonRootMPath],
) -> Result<MultiMover<'a>> {
    let multi_mover: MultiMover = Arc::new(
        move |source_path: &NonRootMPath| -> Result<Vec<NonRootMPath>> {
            let should_export = export_paths.iter().any(|p| p.is_prefix_of(source_path));

            if !should_export {
                trace!(logger, "Path {:#?} will NOT be exported.", &source_path);
                return Ok(vec![]);
            }

            trace!(logger, "Path {:#?} will be exported.", &source_path);
            Ok(vec![source_path.clone()])
        },
    );

    Ok(multi_mover)
}

/// Create a temporary repository to store the changesets that affect the export
/// directories.
/// The temporary repo uses file-backed storage and does not perform any writes
/// to the Mononoke instance provided to the tool (e.g. production Mononoke).
async fn create_temp_repo(fb: FacebookInit, ctx: &CoreContext) -> Result<RepoContext, Error> {
    let logger = ctx.logger();
    let system_tmp = env::temp_dir();
    let temp_dir_suffix = Alphanumeric.sample_string(&mut rand::thread_rng(), 16);

    let mk_temp_path = |prefix| system_tmp.join(format!("{}_{}", prefix, temp_dir_suffix));

    let metadata_db_path = mk_temp_path("metadata_temp");
    let hg_mutation_db_path = mk_temp_path("hg_mut_temp");
    let blobstore_path = mk_temp_path("blobstore");

    debug!(logger, "metadata_db_path: {0:#?}", metadata_db_path);
    debug!(logger, "hg_mutation_db_path: {0:#?}", hg_mutation_db_path);
    debug!(logger, "blobstore_path: {0:#?}", blobstore_path);

    let temp_repo_name = format!("temp_repo_{}", temp_dir_suffix);
    debug!(logger, "Temporary repo name: {}", temp_repo_name);

    let metadata_conn = SqliteConnection::open(metadata_db_path)?;
    let hg_mutation_conn = SqliteConnection::open(hg_mutation_db_path)?;

    let put_behaviour = PutBehaviour::IfAbsent;
    let file_blobstore = Arc::new(Fileblob::create(blobstore_path, put_behaviour)?);

    let temp_repo = TestRepoFactory::with_sqlite_connection(fb, metadata_conn, hg_mutation_conn)?
        .with_blobstore(file_blobstore)
        .with_core_context_that_does_not_override_logger(ctx.clone())
        .with_name(temp_repo_name)
        .build()
        .await?;
    let temp_repo_ctx = RepoContext::new_test(ctx.clone(), temp_repo).await?;

    Ok(temp_repo_ctx)
}
