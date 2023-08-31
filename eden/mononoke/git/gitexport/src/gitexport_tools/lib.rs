/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod partial_commit_graph;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Error;
use blobstore::Loadable;
use borrowed::borrowed;
use cloned::cloned;
use commit_transformation::rewrite_commit;
use commit_transformation::upload_commits;
use commit_transformation::MultiMover;
use futures::stream::TryStreamExt;
use futures::stream::{self};
use mononoke_api::BookmarkKey;
use mononoke_api::ChangesetContext;
use mononoke_api::CoreContext;
use mononoke_api::MononokeError;
use mononoke_api::RepoContext;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use repo_blobstore::RepoBlobstoreArc;
use slog::debug;
use slog::error;
use slog::info;

pub use crate::partial_commit_graph::build_partial_commit_graph_for_export;
use crate::partial_commit_graph::ChangesetParents;

/// Given a list of changesets, their parents and a list of paths, create
/// copies in a target mononoke repository containing only changes that
/// were made on the given paths.
pub async fn rewrite_partial_changesets(
    source_repo_ctx: RepoContext,
    changesets: Vec<ChangesetContext>,
    changeset_parents: &ChangesetParents,
    // Repo that will hold the partial changesets that will be exported to git
    target_repo_ctx: &RepoContext,
    export_paths: Vec<MPath>,
) -> Result<(), MononokeError> {
    let ctx: &CoreContext = source_repo_ctx.ctx();

    let logger = ctx.logger();

    debug!(logger, "export_paths: {:#?}", &export_paths);
    debug!(logger, "changeset_parents: {:#?}", &changeset_parents);

    let logger_clone = logger.clone();

    let multi_mover: MultiMover = Arc::new(move |source_path: &MPath| {
        let should_export = export_paths.iter().any(|p| p.is_prefix_of(source_path));

        if !should_export {
            debug!(
                logger_clone,
                "Path {:#?} will NOT be exported.", &source_path
            );
            return Ok(vec![]);
        }

        debug!(logger_clone, "Path {:#?} will be exported.", &source_path);
        Ok(vec![source_path.clone()])
    });

    let cs_results: Vec<Result<ChangesetContext, MononokeError>> =
        changesets.into_iter().map(Ok).collect::<Vec<_>>();

    let (new_bonsai_changesets, _) = stream::iter(cs_results)
        .try_fold(
            (Vec::new(), HashMap::new()),
            |(mut new_bonsai_changesets, remapped_parents), changeset| {
                borrowed!(source_repo_ctx);
                cloned!(multi_mover);
                async move {
                    let (new_bcs, remapped_parents) = create_bonsai_for_new_repo(
                        source_repo_ctx,
                        multi_mover,
                        changeset_parents,
                        remapped_parents,
                        changeset,
                    )
                    .await?;
                    new_bonsai_changesets.push(new_bcs);
                    Ok((new_bonsai_changesets, remapped_parents))
                }
            },
        )
        .await?;

    debug!(
        logger,
        "new_bonsai_changesets: {:#?}", &new_bonsai_changesets
    );

    let head_cs_id = new_bonsai_changesets
        .last()
        .ok_or(Error::msg("No changesets were moved"))?
        .get_changeset_id();

    upload_commits(
        source_repo_ctx.ctx(),
        new_bonsai_changesets,
        source_repo_ctx.repo(),
        target_repo_ctx.repo(),
    )
    .await?;

    // Set master bookmark to point to the latest changeset
    if let Err(err) = target_repo_ctx
        .create_bookmark(&BookmarkKey::from_str("master")?, head_cs_id, None)
        .await
    {
        // TODO(T161902005): stop failing silently on bookmark creation
        error!(logger, "Failed to create master bookmark: {:?}", err);
    }

    info!(logger, "Finished copying all changesets!");
    Ok(())
}

/// Given a changeset and a set of paths being exported, build the
/// BonsaiChangeset containing only the changes to those paths.
async fn create_bonsai_for_new_repo(
    source_repo_ctx: &RepoContext,
    multi_mover: MultiMover,
    changeset_parents: &ChangesetParents,
    mut remapped_parents: HashMap<ChangesetId, ChangesetId>,
    changeset_ctx: ChangesetContext,
) -> Result<(BonsaiChangeset, HashMap<ChangesetId, ChangesetId>), MononokeError> {
    let logger = changeset_ctx.repo().ctx().logger();
    debug!(
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

#[cfg(test)]
mod test {

    use std::collections::VecDeque;

    use anyhow::anyhow;
    use fbinit::FacebookInit;
    use futures::future::try_join_all;
    use mononoke_api::BookmarkFreshness;
    use test_repo_factory::TestRepoFactory;
    use test_utils::build_test_repo;
    use test_utils::get_relevant_changesets_from_ids;
    use test_utils::GitExportTestRepoOptions;
    use test_utils::EXPORT_DIR;
    use test_utils::EXPORT_FILE;
    use test_utils::FILE_IN_SECOND_EXPORT_DIR;
    use test_utils::SECOND_EXPORT_DIR;
    use test_utils::SECOND_EXPORT_FILE;

    use super::*;

    #[fbinit::test]
    async fn test_rewrite_partial_changesets(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

        let export_dir = MPath::new(EXPORT_DIR).unwrap();
        let second_export_dir = MPath::new(SECOND_EXPORT_DIR).unwrap();

        let (source_repo_ctx, changeset_ids) =
            build_test_repo(fb, &ctx, GitExportTestRepoOptions::default()).await?;

        let first = changeset_ids["first"];
        let third = changeset_ids["third"];
        let fifth = changeset_ids["fifth"];
        let sixth = changeset_ids["sixth"];
        let seventh = changeset_ids["seventh"];
        let ninth = changeset_ids["ninth"];
        let tenth = changeset_ids["tenth"];

        let target_repo = TestRepoFactory::new(fb)?.build().await?;
        let target_repo_ctx = RepoContext::new_test(ctx.clone(), target_repo).await?;

        // Test that changesets are rewritten when relevant changesets are given
        // topologically sorted
        let relevant_changeset_ids: Vec<ChangesetId> =
            vec![first, third, fifth, sixth, seventh, ninth, tenth];

        let relevant_changesets: Vec<ChangesetContext> =
            get_relevant_changesets_from_ids(&source_repo_ctx, relevant_changeset_ids).await?;

        let relevant_changeset_parents = HashMap::from([
            (first, vec![]),
            (third, vec![first]),
            (fifth, vec![third]),
            (sixth, vec![fifth]),
            (seventh, vec![sixth]),
            (ninth, vec![seventh]),
            (tenth, vec![ninth]),
        ]);

        rewrite_partial_changesets(
            source_repo_ctx.clone(),
            relevant_changesets.clone(),
            &relevant_changeset_parents,
            &target_repo_ctx,
            vec![export_dir.clone(), second_export_dir.clone()],
        )
        .await?;

        let master_cs = target_repo_ctx
            .resolve_bookmark(
                &BookmarkKey::from_str("master")?,
                BookmarkFreshness::MostRecent,
            )
            .await?
            .ok_or(anyhow!("Couldn't find master bookmark in target repo."))?;

        let mut parents_to_check: VecDeque<ChangesetId> = VecDeque::from([master_cs.id()]);
        let mut target_css = vec![];

        while let Some(changeset_id) = parents_to_check.pop_front() {
            let changeset = target_repo_ctx
                .changeset(changeset_id)
                .await?
                .ok_or(anyhow!("Changeset not found in target repo"))?;

            changeset
                .parents()
                .await?
                .into_iter()
                .for_each(|parent| parents_to_check.push_back(parent));

            target_css.push(changeset);
        }

        // Order the changesets topologically
        target_css.reverse();

        assert_eq!(
            try_join_all(target_css.iter().map(ChangesetContext::message)).await?,
            try_join_all(relevant_changesets.iter().map(ChangesetContext::message)).await?
        );

        async fn get_msg_and_files_changed(
            cs: &ChangesetContext,
            file_filter: Box<dyn Fn(&MPath) -> bool>,
        ) -> Result<(String, Vec<MPath>), MononokeError> {
            let msg = cs.message().await?;
            let fcs = cs.file_changes().await?;

            let files_changed: Vec<MPath> =
                fcs.into_keys().filter(file_filter).collect::<Vec<MPath>>();

            Ok((msg, files_changed))
        }

        let result = try_join_all(
            target_css
                .iter()
                .map(|cs| get_msg_and_files_changed(cs, Box::new(|_p| true))),
        )
        .await?;

        fn build_expected_tuple(msg: &str, fpaths: Vec<&str>) -> (String, Vec<MPath>) {
            (
                String::from(msg),
                fpaths
                    .iter()
                    .map(|p| MPath::new(p).unwrap())
                    .collect::<Vec<_>>(),
            )
        }

        assert_eq!(result.len(), 7);
        assert_eq!(result[0], build_expected_tuple("first", vec![EXPORT_FILE]));
        assert_eq!(result[1], build_expected_tuple("third", vec![EXPORT_FILE]));
        assert_eq!(result[2], build_expected_tuple("fifth", vec![EXPORT_FILE]));
        assert_eq!(
            result[3],
            build_expected_tuple("sixth", vec![FILE_IN_SECOND_EXPORT_DIR])
        );
        assert_eq!(
            result[4],
            build_expected_tuple("seventh", vec![EXPORT_FILE, FILE_IN_SECOND_EXPORT_DIR])
        );
        assert_eq!(
            result[5],
            build_expected_tuple("ninth", vec![SECOND_EXPORT_FILE])
        );
        assert_eq!(result[6], build_expected_tuple("tenth", vec![EXPORT_FILE]));

        Ok(())
    }

    #[fbinit::test]
    async fn test_rewriting_fails_with_irrelevant_changeset(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

        let export_dir = MPath::new(EXPORT_DIR).unwrap();

        let (source_repo_ctx, changeset_ids) =
            build_test_repo(fb, &ctx, GitExportTestRepoOptions::default()).await?;

        let first = changeset_ids["first"];
        let third = changeset_ids["third"];
        let fourth = changeset_ids["fourth"];
        let fifth = changeset_ids["fifth"];

        // Passing an irrelevant changeset in the list should result in an error
        let broken_changeset_list_ids: Vec<ChangesetId> = vec![first, third, fourth, fifth];

        let broken_changeset_list: Vec<ChangesetContext> =
            get_relevant_changesets_from_ids(&source_repo_ctx, broken_changeset_list_ids).await?;

        let broken_changeset_parents = HashMap::from([
            (first, vec![]),
            (third, vec![first]),
            (fourth, vec![third]),
            (fifth, vec![fourth]),
        ]);

        let target_repo = TestRepoFactory::new(fb)?.build().await?;
        let target_repo_ctx = RepoContext::new_test(ctx.clone(), target_repo).await?;

        let error = rewrite_partial_changesets(
            source_repo_ctx.clone(),
            broken_changeset_list.clone(),
            &broken_changeset_parents,
            &target_repo_ctx,
            vec![export_dir.clone()],
        )
        .await
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "internal error: Commit wasn't rewritten because it had no signficant changes"
        );

        Ok(())
    }
}
