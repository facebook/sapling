/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use commit_transformation::rewrite_commit_with_file_changes_filter;
use commit_transformation::FileChangeFilter;
use commit_transformation::FileChangeFilterApplication;
use commit_transformation::FileChangeFilterFunc;
use commit_transformation::MultiMover;
use commit_transformation::RewriteOpts;
use context::CoreContext;
use metaconfig_types::GitSubmodulesChangesAction;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use movers::Mover;
use movers::Movers;

use crate::git_submodules::sync_commit_with_submodule_expansion;
use crate::git_submodules::SubmoduleExpansionData;
use crate::types::Repo;
use crate::types::SubmodulePath;

pub type SubmoduleExpansionContentIds = HashMap<SubmodulePath, HashSet<ContentId>>;

pub struct CommitRewriteResult {
    /// A version of the source repo's bonsai changeset with `Mover` applied to
    /// all changes and submodules processed according to the
    /// small repo sync config (e.g. expanded, stripped).
    ///
    /// - `None` if the rewrite decided that this commit should
    ///              not be present in the rewrite target
    /// - `Some(rewritten)` for a successful rewrite, which should be
    ///                         present in the rewrite target
    pub rewritten: Option<BonsaiChangesetMut>,
    /// Map from submodule dependency repo to all the file changes that have
    /// to be copied from its blobstore to the large repo's blobstore for the
    /// submodule expansion in the rewritten commit.
    pub submodule_expansion_content_ids: SubmoduleExpansionContentIds,
}

impl CommitRewriteResult {
    pub fn new(
        rewritten: Option<BonsaiChangesetMut>,
        submodule_expansion_content_ids: SubmoduleExpansionContentIds,
    ) -> Self {
        Self {
            rewritten,
            submodule_expansion_content_ids,
        }
    }
}

/// Create a version of `cs` with `Mover` applied to all changes
/// The return value can be:
/// - `Err` if the rewrite failed
/// - `Ok(None)` if the rewrite decided that this commit should
///              not be present in the rewrite target
/// - `Ok(Some(rewritten))` for a successful rewrite, which should be
///                         present in the rewrite target
///
/// The notion that the commit "should not be present in the rewrite
/// target" means that the commit is not a merge and all of its changes
/// were rewritten into nothingness by the `Mover`.
///
/// Precondition: this function expects all `cs` parents to be present
/// in `remapped_parents` as keys, and their remapped versions as values.
pub async fn rewrite_commit<'a, R: Repo>(
    ctx: &'a CoreContext,
    cs: BonsaiChangesetMut,
    remapped_parents: &'a HashMap<ChangesetId, ChangesetId>,
    movers: Movers,
    source_repo: &'a R,
    rewrite_opts: RewriteOpts,
    git_submodules_action: GitSubmodulesChangesAction,
    mb_submodule_expansion_data: Option<SubmoduleExpansionData<'a, R>>,
) -> Result<CommitRewriteResult> {
    // TODO(T169695293): add filter to only keep submodules for implicit deletes?
    let (file_changes_filters, cs): (Vec<FileChangeFilter<'a>>, BonsaiChangesetMut) =
        match git_submodules_action {
            GitSubmodulesChangesAction::Strip => {
                let filter_func: FileChangeFilterFunc<'a> = Arc::new(move |(_path, fc)| match fc {
                    FileChange::Change(tfc) => tfc.file_type() != FileType::GitSubmodule,
                    _ => true,
                });
                let filter: FileChangeFilter<'a> = FileChangeFilter {
                    func: filter_func,
                    application: FileChangeFilterApplication::MultiMover,
                };

                (vec![filter], cs)
            }
            // Keep submodules -> no filters and keep original bonsai
            GitSubmodulesChangesAction::Keep => (vec![], cs),
            // Expand submodules -> no filters, but modify the file change
            // file types in the bonsai
            GitSubmodulesChangesAction::Expand => {
                let submodule_expansion_data = mb_submodule_expansion_data.ok_or(
                  anyhow!("Submodule expansion data not provided when submodules is enabled for small repo")
              )?;

                return sync_commit_with_submodule_expansion(
                    ctx,
                    cs,
                    source_repo,
                    submodule_expansion_data,
                    movers.clone(),
                    remapped_parents,
                    rewrite_opts,
                )
                .await;
            }
        };

    let mb_rewritten = rewrite_commit_with_file_changes_filter(
        ctx,
        cs,
        remapped_parents,
        mover_to_multi_mover(movers.mover),
        source_repo,
        None,
        rewrite_opts,
        file_changes_filters,
    )
    .await?;

    Ok(CommitRewriteResult::new(mb_rewritten, HashMap::new()))
}

/// Adapter from Mover to MultiMover.
struct MoverMultiMover(Arc<dyn Mover>);

impl MultiMover for MoverMultiMover {
    fn multi_move_path(&self, path: &NonRootMPath) -> Result<Vec<NonRootMPath>> {
        Ok(self.0.move_path(path)?.into_iter().collect())
    }

    fn conflicts_with(&self, path: &NonRootMPath) -> Result<bool> {
        self.0.conflicts_with(path)
    }
}

/// Mover moves a path to at most a single path, while MultiMover can move a
/// path to multiple.
pub fn mover_to_multi_mover(mover: Arc<dyn Mover>) -> Arc<dyn MultiMover> {
    Arc::new(MoverMultiMover(mover))
}
