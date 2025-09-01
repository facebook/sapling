/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::env;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use blobstore::Loadable;
use blobstore::PutBehaviour;
use borrowed::borrowed;
use changeset_info::ChangesetInfo;
use cloned::cloned;
use commit_transformation::MultiMover;
use commit_transformation::get_renamed_implicit_deletes;
use commit_transformation::rewrite_commit_with_implicit_deletes;
use commit_transformation::upload_commits;
use derived_data_manager::BonsaiDerivable;
use fbinit::FacebookInit;
use fileblob::Fileblob;
use futures::StreamExt;
use futures::stream::TryStreamExt;
use futures::stream::{self};
use futures_stats::TimedTryFutureExt;
use git_types::MappedGitCommitId;
use git_types::RootGitDeltaManifestV2Id;
use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use maplit::hashmap;
use maplit::hashset;
use metaconfig_types::DerivedDataTypesConfig;
use metaconfig_types::GitDeltaManifestV2Config;
use metaconfig_types::GitDeltaManifestV3Config;
use mononoke_api::BookmarkKey;
use mononoke_api::ChangesetContext;
use mononoke_api::CoreContext;
use mononoke_api::MononokeError;
use mononoke_api::MononokeRepo;
use mononoke_api::Repo;
use mononoke_api::RepoContext;
use mononoke_macros::mononoke;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::NonRootMPath;
use rand::distributions::Alphanumeric;
use rand::distributions::DistString;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::RepoDerivedDataRef;
use scuba_ext::FutureStatsScubaExt;
use slog::Logger;
use slog::debug;
use slog::info;
use slog::trace;
use slog::warn;
use sql::rusqlite::Connection as SqliteConnection;
use test_repo_factory::TestRepoFactory;
use unodes::RootUnodeManifestId;
use warm_bookmarks_cache::NoopBookmarksCache;

pub use crate::git_repo::create_git_repo_on_disk;
use crate::partial_commit_graph::ChangesetParents;
pub use crate::partial_commit_graph::ExportPathInfo;
pub use crate::partial_commit_graph::GitExportGraphInfo;
pub use crate::partial_commit_graph::build_partial_commit_graph_for_export;

pub const MASTER_BOOKMARK: &str = "heads/master";

struct ChangesetRewriteInfo<R> {
    changeset_context: ChangesetContext<R>,
    export_paths: Vec<NonRootMPath>,
    implicit_deletes: Vec<Vec<NonRootMPath>>,
}

/// Given a list of changesets, their parents and a list of paths, create
/// copies in a target mononoke repository containing only changes that
/// were made on the given paths.
pub async fn rewrite_partial_changesets<R: MononokeRepo>(
    fb: FacebookInit,
    source_repo_ctx: RepoContext<R>,
    graph_info: GitExportGraphInfo<R>,
    export_paths: Vec<ExportPathInfo<R>>,
    implicit_delete_prefetch_buffer_size: usize,
) -> Result<RepoContext<Repo>> {
    let source_repo_ctx = Arc::new(source_repo_ctx);
    let ctx = source_repo_ctx.ctx().clone();
    let changesets = graph_info.changesets;
    let changeset_parents = &graph_info.parents_map;
    let logger = ctx.logger();

    info!(logger, "Copying changesets to temporary repo...");

    debug!(logger, "export_paths: {:#?}", &export_paths);

    // Repo that will hold the partial changesets that will be exported to git
    let temp_repo_ctx = create_temp_repo(fb, &ctx).await?;

    let num_changesets = changesets.len().try_into().unwrap();

    // Commits are copied from oldest to newest. In the situation where an
    // export paths is created by copying a file from a non-export path, we
    // print a warning to the user informing the path being created, the commit
    // creating it and the commit from which the file is being copied.
    //
    // This set keeps track of the paths that haven't yet had a warning printed
    // for their creation during the rewrite of the commits.
    let all_export_paths: HashSet<NonRootMPath> =
        export_paths.iter().cloned().map(|p| p.0).collect();

    let mb_progress_bar = get_progress_bar(logger, num_changesets, "Copying changesets")?;

    let export_paths = Arc::new(export_paths);

    let (mb_head_cs_id, _, _) = stream::iter(changesets)
        .map(|cs| {
            cloned!(source_repo_ctx, export_paths);

            let blobstore = source_repo_ctx.repo().repo_blobstore_arc();
            async move {
                mononoke::spawn_task(async move {
                    let ctx = source_repo_ctx.ctx();
                    let export_paths = get_export_paths_for_changeset(&cs, &export_paths).await?;
                    let bcs = cs
                        .id()
                        .load(source_repo_ctx.ctx(), &blobstore)
                        .await
                        .map_err(MononokeError::from)?;

                    // Before getting implicit deletes, filter the file changes
                    // to remove irrelevant ones that slow down the process.
                    let file_changes: Vec<(&NonRootMPath, &FileChange)> = bcs
                        .file_changes()
                        .filter(|(source_path, _fc): &(&NonRootMPath, &FileChange)| {
                            export_paths.iter().any(|p|
                                // Inside export path, so should be fully analysed
                                p.is_prefix_of(*source_path) ||
                                // Not necessarily exported, but could implicitly delete
                                // an export path, so implicitly deletes should be collected
                                source_path.is_prefix_of(p))
                        })
                        .collect::<Vec<_>>();

                    let multi_mover = Arc::new(GitExportMultiMover::new(
                        source_repo_ctx.ctx().logger(),
                        export_paths.as_slice(),
                    ));

                    if file_changes.is_empty() {
                        return Err(anyhow!("No relevant file changes in changeset"));
                    };

                    let renamed_implicit_deletes = get_renamed_implicit_deletes(
                        source_repo_ctx.ctx(),
                        file_changes,
                        bcs.parents(),
                        multi_mover,
                        source_repo_ctx.repo(),
                    )
                    .try_timed()
                    .await?
                    .log_future_stats(
                        ctx.scuba().clone(),
                        "Getting renamed implicit deletes",
                        None,
                    );

                    Ok(ChangesetRewriteInfo {
                        changeset_context: cs,
                        export_paths,
                        implicit_deletes: renamed_implicit_deletes,
                    })
                })
                .await?
            }
        })
        .buffered(implicit_delete_prefetch_buffer_size)
        .try_fold(
            (None, HashMap::new(), all_export_paths),
            |(_head_cs_id, remapped_parents, export_paths_not_created), changeset_rewrite_info| {
                let changeset = changeset_rewrite_info.changeset_context;
                let export_paths = changeset_rewrite_info.export_paths;
                let implicit_deletes = changeset_rewrite_info.implicit_deletes;
                cloned!(source_repo_ctx);
                borrowed!(temp_repo_ctx, mb_progress_bar);

                async move {
                    let ctx: &CoreContext = source_repo_ctx.ctx();
                    let multi_mover =
                        Arc::new(GitExportMultiMover::new(ctx.logger(), &export_paths));
                    let (new_bcs, remapped_parents, export_paths_not_created) =
                        create_bonsai_for_new_repo(
                            &source_repo_ctx,
                            multi_mover,
                            changeset_parents,
                            remapped_parents,
                            changeset,
                            &export_paths,
                            export_paths_not_created,
                            implicit_deletes,
                        )
                        .try_timed()
                        .await?
                        .log_future_stats(
                            ctx.scuba().clone(),
                            "Creating bonsai for temp repo",
                            None,
                        );

                    let new_bcs_id = new_bcs.get_changeset_id();

                    upload_commits(
                        source_repo_ctx.ctx(),
                        vec![new_bcs],
                        source_repo_ctx.repo(),
                        temp_repo_ctx.repo(),
                        Vec::<(Arc<Repo>, HashSet<_>)>::new(),
                    )
                    .try_timed()
                    .await?
                    .log_future_stats(
                        ctx.scuba().clone(),
                        "Upload commits to temp repo",
                        None,
                    );

                    temp_repo_ctx
                        .repo()
                        .repo_derived_data()
                        .derive::<RootGitDeltaManifestV2Id>(ctx, new_bcs_id)
                        .try_timed()
                        .await
                        .with_context(|| {
                            format!(
                                "Error in deriving RootGitDeltaManifestV2Id for Bonsai commit {:?}",
                                new_bcs_id
                            )
                        })?
                        .log_future_stats(
                            ctx.scuba().clone(),
                            "Deriving RootGitDeltaManifestV2Id",
                            None,
                        );

                    if let Some(progress_bar) = mb_progress_bar {
                        progress_bar.inc(1);
                    }
                    Ok((Some(new_bcs_id), remapped_parents, export_paths_not_created))
                }
            },
        )
        .await?;

    if let Some(progress_bar) = mb_progress_bar {
        progress_bar.finish();
    }

    let head_cs_id = mb_head_cs_id.ok_or(Error::msg("No changesets were moved"))?;

    info!(logger, "Setting master bookmark to the latest changeset");

    // Set master bookmark to point to the latest changeset
    temp_repo_ctx
        .create_bookmark(&BookmarkKey::from_str(MASTER_BOOKMARK)?, head_cs_id, None)
        .await
        .with_context(|| "Failed to create master bookmark")?;

    info!(logger, "Finished copying all changesets!");
    Ok(temp_repo_ctx)
}

/// Given a changeset and a set of paths being exported, build the
/// BonsaiChangeset containing only the changes to those paths.
async fn create_bonsai_for_new_repo<'a, R: MononokeRepo>(
    source_repo_ctx: &RepoContext<R>,
    multi_mover: Arc<dyn MultiMover + 'a>,
    changeset_parents: &ChangesetParents,
    mut remapped_parents: HashMap<ChangesetId, ChangesetId>,
    changeset_ctx: ChangesetContext<R>,
    export_paths: &'a [NonRootMPath],
    mut export_paths_not_created: HashSet<NonRootMPath>,
    implicit_deletes: Vec<Vec<NonRootMPath>>,
) -> Result<
    (
        BonsaiChangeset,
        HashMap<ChangesetId, ChangesetId>,
        HashSet<NonRootMPath>,
    ),
    MononokeError,
> {
    let logger = changeset_ctx.ctx().logger();
    trace!(
        logger,
        "Rewriting changeset: {:#?} | {:#?}",
        &changeset_ctx.id(),
        &changeset_ctx.message().await?
    );

    let blobstore = source_repo_ctx.repo().repo_blobstore_arc();
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

    mut_bcs.file_changes.iter_mut().for_each(|(new_path, fc)| {
        if let FileChange::Change(tracked_fc) = fc {
            // If any FileChange copies a file from a previous revision (e.g. a parent),
            // set the `copy_from` field to point to its new parent.
            //
            // Since we're building a history using all changesets that
            // affect the exported directories, any file being copied
            // should always exist in the new parent.

            // Get all the export paths under which the modified file is.
            let matched_export_paths = export_paths
                .iter()
                .filter(|p| p.is_prefix_of(new_path))
                .collect::<HashSet<_>>();

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

                // This is copying from a path that's being exported, so we can
                // reference the new parent in the `copy_from` field.
                let is_old_path_exported = export_paths.iter().any(|p| p.is_prefix_of(old_path));

                let new_parent_cs_id = orig_parent_ids.first();

                if new_parent_cs_id.is_some() && is_old_path_exported {
                    *copy_from_cs_id = new_parent_cs_id.cloned().unwrap();
                } else {
                    // In this case, the `copy_from` reference will be dropped,
                    // because (a) the file being created is not being exported
                    // or (b) the file being copied is creating an export path,
                    // so we can't reference previous commits because the path
                    // won't be there.
                    //
                    // Scenario (a) is irrelevant, but in scenario (b), we print
                    // a warning to the user for every export path being created
                    // so that they can re-run gitexport passing extra paths
                    // if they want to follow the history.

                    // Check if the file being copied is creating any export
                    // path.
                    let exp_paths_not_created_refs = export_paths_not_created.iter().collect();
                    let created_export_paths = matched_export_paths
                        .intersection(&exp_paths_not_created_refs)
                        .collect::<Vec<_>>();

                    for exp_p in created_export_paths {
                        // By default, these renames won't be followed, but a warning
                        // will be printed so that the user can check the commit
                        // and decide if they want to re-run it passing the old
                        // name as an export path along with this commit as the
                        // head changeset.
                        warn!(
                            logger,
                            concat!(
                                "Changeset {} might have created the exported path {} by ",
                                "moving/copying files from a commit that might not be",
                                " exported (id {})."
                            ),
                            changeset_ctx.id(),
                            exp_p,
                            copy_from_cs_id
                        );
                        warn!(logger, "pre move/copy path: {}", old_path);
                    }

                    *tracked_fc = tracked_fc.with_new_copy_from(None);
                };
            };
            // If any of the matched export paths from the set of the
            // ones that haven't shown up yet (weren't created), to avoid
            // printing false positive warnings to the user about
            // `copy_from` references.
            matched_export_paths.into_iter().for_each(|p| {
                export_paths_not_created.take(p);
            });
        };
    });

    let rewritten_bcs_mut = rewrite_commit_with_implicit_deletes(
        logger,
        mut_bcs,
        &remapped_parents,
        multi_mover,
        vec![],
        None,
        implicit_deletes,
        Default::default(),
    )?
    // This shouldn't happen because every changeset provided is modifying
    // at least one of the exported files.
    .ok_or(Error::msg(
        "Commit wasn't rewritten because it had no significant changes",
    ))?;

    let rewritten_bcs = rewritten_bcs_mut.freeze()?;

    // Update the remapped_parents map so that children of this commit use
    // its new id when moved.
    remapped_parents.insert(changeset_ctx.id(), rewritten_bcs.get_changeset_id());

    Ok((rewritten_bcs, remapped_parents, export_paths_not_created))
}

/// Builds a vector of references to the paths that should be exported when
/// rewriting the provided changeset based on each export path's head commit.
async fn get_export_paths_for_changeset<'a, R: MononokeRepo>(
    processed_cs: &ChangesetContext<R>,
    export_path_infos: &'a Vec<ExportPathInfo<R>>,
) -> Result<Vec<NonRootMPath>> {
    // Get the export paths for the changeset being processed considering its
    // head commit.
    let export_paths: Vec<NonRootMPath> = stream::iter(export_path_infos)
        .then(|(exp_path, head_cs)| async move {
            // If the processed changeset is older than a path's head commit,
            // then this path should be exported when rewriting this changeset.
            let is_ancestor_of_head_cs = processed_cs.is_ancestor_of(head_cs.id()).await?;
            if is_ancestor_of_head_cs {
                return anyhow::Ok(Some(exp_path.clone()));
            }
            // Otherwise the changeset is a descendant of this path's head, so
            // the path should NOT be exported.
            Ok(None)
        })
        .try_collect::<Vec<_>>()
        .await?
        .into_iter()
        .flatten()
        .collect::<Vec<NonRootMPath>>();

    Ok(export_paths)
}

/// The MultiMover is called for every path affected by a commit being
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
struct GitExportMultiMover<'a> {
    logger: &'a Logger,
    export_paths: &'a [NonRootMPath],
}

impl<'a> GitExportMultiMover<'a> {
    fn new(logger: &'a Logger, export_paths: &'a [NonRootMPath]) -> Self {
        Self {
            logger,
            export_paths,
        }
    }
}

impl<'a> MultiMover for GitExportMultiMover<'a> {
    fn multi_move_path(&self, source_path: &NonRootMPath) -> Result<Vec<NonRootMPath>> {
        let should_export = self
            .export_paths
            .iter()
            .any(|p| p.is_prefix_of(source_path));

        if !should_export {
            trace!(
                self.logger,
                "Path {:#?} will NOT be exported.", &source_path
            );
            return Ok(vec![]);
        }

        trace!(self.logger, "Path {:#?} will be exported.", &source_path);
        Ok(vec![source_path.clone()])
    }

    fn conflicts_with(&self, path: &NonRootMPath) -> Result<bool> {
        Ok(self.export_paths.iter().any(|p| p.is_related_to(path)))
    }
}

/// Create a temporary repository to store the changesets that affect the export
/// directories.
/// The temporary repo uses file-backed storage and does not perform any writes
/// to the Mononoke instance provided to the tool (e.g. production Mononoke).
async fn create_temp_repo(fb: FacebookInit, ctx: &CoreContext) -> Result<RepoContext<Repo>, Error> {
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

    let derived_data_types_config = DerivedDataTypesConfig {
        types: hashset! {
            ChangesetInfo::VARIANT,
            MappedGitCommitId::VARIANT,
            RootGitDeltaManifestV2Id::VARIANT,
            RootUnodeManifestId::VARIANT,
        },
        git_delta_manifest_v2_config: Some(GitDeltaManifestV2Config {
            max_inlined_object_size: 2_000,
            max_inlined_delta_size: 2_000,
            delta_chunk_size: 1_000_000,
        }),
        git_delta_manifest_v3_config: Some(GitDeltaManifestV3Config {
            max_inlined_object_size: 100_000,
            max_inlined_delta_size: 100_000,
            delta_chunk_size: 1_000_000,
            entry_chunk_size: 100_000,
        }),
        ..Default::default()
    };
    let available_configs = hashmap! {
        "default".to_string() => derived_data_types_config.clone(),
    };
    let mut factory = TestRepoFactory::with_sqlite_connection(fb, metadata_conn, hg_mutation_conn)?;
    factory
        .with_blobstore(file_blobstore)
        .with_cacheless_git_symbolic_refs()
        .with_core_context_that_does_not_override_logger(ctx.clone())
        .with_name(temp_repo_name)
        .with_config_override(|cfg| {
            cfg.derived_data_config.available_configs = available_configs;

            // If this isn't disabled the master bookmark creation will fail
            // because skeleton manifests derivation is disabled.
            cfg.pushrebase.flags.casefolding_check = false;
        });
    let bookmarks =
        factory.bookmarks(&factory.sql_bookmarks(&factory.repo_identity(&factory.repo_config()))?);
    factory.with_bookmarks_cache(Arc::new(NoopBookmarksCache::new(bookmarks)));

    let temp_repo = factory.build().await?;
    let temp_repo_ctx = RepoContext::new_test(ctx.clone(), temp_repo).await?;

    Ok(temp_repo_ctx)
}

fn get_progress_bar(
    logger: &Logger,
    num_changesets: u64,
    message: &'static str,
) -> Result<Option<ProgressBar>> {
    if logger.is_enabled(slog::Level::Info) {
        let progress_bar = ProgressBar::new(num_changesets)
  .with_message(message)
  .with_style(
      ProgressStyle::with_template(
          "[{percent}%][elapsed: {elapsed}] {msg} [{bar:60.cyan}] (ETA: {eta}) ({pos}/{len}) ({per_sec}) ",
      )?
      .progress_chars("#>-"),
  );
        progress_bar.enable_steady_tick(std::time::Duration::from_secs(3));
        Ok(Some(progress_bar))
    } else {
        Ok(None)
    }
}
