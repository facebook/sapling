/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use blobrepo::save_bonsai_changesets;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use changeset_fetcher::ChangesetFetcherRef;
use changesets::ChangesetsRef;
use commit_transformation::create_directory_source_to_target_multi_mover;
use commit_transformation::create_source_to_target_multi_mover;
use commit_transformation::rewrite_as_squashed_commit;
use commit_transformation::rewrite_commit;
use commit_transformation::upload_commits;
use commit_transformation::CommitRewrittenToEmpty;
use context::CoreContext;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use megarepo_config::MononokeMegarepoConfigs;
use megarepo_config::Source;
use megarepo_config::SourceMappingRules;
use megarepo_config::SourceRevision;
use megarepo_config::Target;
use megarepo_error::MegarepoError;
use megarepo_mapping::CommitRemappingState;
use megarepo_mapping::MegarepoMapping;
use megarepo_mapping::SourceName;
use mononoke_api::ChangesetContext;
use mononoke_api::Mononoke;
use mononoke_api::RepoContext;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mutable_renames::MutableRenames;

use crate::common::find_source_config;
use crate::common::find_target_bookmark_and_value;
use crate::common::find_target_sync_config;
use crate::common::MegarepoOp;
use crate::common::SourceAndMovedChangesets;

pub(crate) struct SyncChangeset<'a> {
    megarepo_configs: &'a Arc<dyn MononokeMegarepoConfigs>,
    mononoke: &'a Arc<Mononoke>,
    target_megarepo_mapping: &'a Arc<MegarepoMapping>,
    mutable_renames: &'a Arc<MutableRenames>,
}

#[async_trait]
impl<'a> MegarepoOp for SyncChangeset<'a> {
    fn mononoke(&self) -> &Arc<Mononoke> {
        self.mononoke
    }
}

pub enum MergeMode {
    Squashed {
        side_commits: Vec<ChangesetContext>,
    },
    ExtraMoveCommits {
        side_parents_move_commits: Vec<SourceAndMovedChangesets>,
    },
}

fn get_squashing_overrides(repo_name: &str, target_bookmark: &str) -> (Option<i64>, Option<bool>) {
    let targets = tunables::tunables()
        .get_by_repo_megarepo_squashing_config_override_targets(repo_name)
        .unwrap_or_default();
    if targets
        .iter()
        .any(|target| target.as_str() == target_bookmark)
    {
        (
            tunables::tunables().get_by_repo_megarepo_override_squashing_limit(repo_name),
            tunables::tunables().get_by_repo_megarepo_override_author_check(repo_name),
        )
    } else {
        (None, None)
    }
}

pub struct SquashingConfig {
    squash_limit: usize,
    check_author: bool,
}

const MERGE_COMMIT_MOVES_CONCURRENCY: usize = 10;

impl<'a> SyncChangeset<'a> {
    pub(crate) fn new(
        megarepo_configs: &'a Arc<dyn MononokeMegarepoConfigs>,
        mononoke: &'a Arc<Mononoke>,
        target_megarepo_mapping: &'a Arc<MegarepoMapping>,
        mutable_renames: &'a Arc<MutableRenames>,
    ) -> Self {
        Self {
            megarepo_configs,
            mononoke,
            target_megarepo_mapping,
            mutable_renames,
        }
    }

    pub(crate) async fn sync(
        &self,
        ctx: &CoreContext,
        source_cs_id: ChangesetId,
        source_name: &SourceName,
        target: &Target,
        target_location: ChangesetId,
    ) -> Result<ChangesetId, MegarepoError> {
        let target_repo = self.find_repo_by_id(ctx, target.repo_id).await?;

        // Now we need to find the target config version that was used to create the latest
        // target commit. This config version will be used to sync the new changeset
        let (_, actual_target_location) =
            find_target_bookmark_and_value(ctx, &target_repo, target).await?;

        if target_location != actual_target_location {
            // Check if previous call was successful and return result if so
            return self
                .check_if_this_method_has_already_succeeded(
                    ctx,
                    source_cs_id,
                    source_name,
                    (target_location, actual_target_location),
                    &target_repo,
                )
                .await;
        }

        let (commit_remapping_state, target_config) = find_target_sync_config(
            ctx,
            target_repo.blob_repo(),
            target_location,
            target,
            self.megarepo_configs,
        )
        .await?;

        // Given the SyncTargetConfig, let's find config for the source
        // we are going to sync from
        let source_config = find_source_config(source_name, &target_config)?;

        // Find source repo and changeset that we need to sync
        let source_repo = self.find_repo_by_id(ctx, source_config.repo_id).await?;
        let source_cs = source_cs_id
            .load(ctx, source_repo.blob_repo().blobstore())
            .await?;

        validate_can_sync_changeset(
            ctx,
            target,
            &source_cs,
            &commit_remapping_state,
            &source_repo,
            source_config,
        )
        .await?;

        // In case of merge commits we need to add move commits on top of the
        // merged-in commits or squash side-branch.
        let maybe_squashing_config = match &source_config.merge_mode {
            Some(megarepo_config::MergeMode::squashed(sq)) => {
                let (maybe_squash_limit, maybe_check_author) =
                    get_squashing_overrides(target_repo.name(), &target.bookmark);
                Some(SquashingConfig {
                    squash_limit: maybe_squash_limit
                        .unwrap_or(sq.squash_limit)
                        .try_into()
                        .context("couldn't convert squash commits limit")?,
                    check_author: maybe_check_author.unwrap_or(true),
                })
            }
            None | Some(megarepo_config::MergeMode::with_move_commit(_)) => None,
            Some(megarepo_config::MergeMode::UnknownField(_)) => {
                return Err(anyhow!("Unknown MergeMode").into());
            }
        };

        let (is_squashable, side_commits) = if let Some(sq) = maybe_squashing_config {
            self.is_commit_squashable(
                target,
                source_cs_id,
                &commit_remapping_state,
                source_name,
                &source_repo,
                &sq,
            )
            .await?
        } else {
            (false, vec![])
        };

        let merge_mode = if is_squashable {
            MergeMode::Squashed { side_commits }
        } else {
            MergeMode::ExtraMoveCommits {
                side_parents_move_commits: self
                    .create_move_commits(
                        ctx,
                        target,
                        &source_cs,
                        &commit_remapping_state,
                        &source_repo,
                        source_name,
                        source_config,
                    )
                    .await?,
            }
        };

        // Finally create a commit in the target and update the mapping.
        let source_cs_id = source_cs.get_changeset_id();
        let new_target_cs_id = sync_changeset_to_target(
            ctx,
            &source_config.mapping,
            source_name,
            source_repo.blob_repo(),
            source_cs,
            target_repo.blob_repo(),
            target_location,
            target,
            commit_remapping_state,
            merge_mode,
        )
        .await?;

        self.target_megarepo_mapping
            .insert_source_target_cs_mapping(
                ctx,
                source_name,
                target,
                source_cs_id,
                new_target_cs_id,
                &target_config.version,
            )
            .await?;

        // Move the bookmark and record latest synced source changeset
        self.move_bookmark_conditionally(
            ctx,
            target_repo.blob_repo(),
            target.bookmark.clone(),
            (target_location, new_target_cs_id),
        )
        .await?;

        Ok(new_target_cs_id)
    }

    async fn is_commit_squashable(
        &self,
        target: &Target,
        source_cs_id: ChangesetId,
        commit_remapping_state: &CommitRemappingState,
        source_name: &SourceName,
        source_repo: &RepoContext,
        squashing_config: &SquashingConfig,
    ) -> Result<(bool, Vec<ChangesetContext>)> {
        if squashing_config.squash_limit == 0 {
            return Ok((false, vec![]));
        }

        let latest_synced_cs_id =
            find_latest_synced_cs_id(commit_remapping_state, source_name, target)?;

        // Take all ancestors of source_cs_id which are not ancestors of
        // latest_synced_cs_id up to limit+1 commits.
        let side_commits: Vec<_> = source_repo
            .difference_of_unions_of_ancestors(vec![source_cs_id], vec![latest_synced_cs_id])
            .skip(1) // The source_cs_id is always the first returned commit.
            .take(squashing_config.squash_limit + 1) // Get one above limit so we know if there's more than limit.
            .try_collect()
            .await?;

        // Bail if we got nothing to do
        if side_commits.is_empty() {
            return Ok((false, vec![]));
        }

        // Bail if we got over limit of commits
        if side_commits.len() > squashing_config.squash_limit {
            return Ok((false, vec![]));
        }

        // The author we'll be comparing to ensure all commits are the same author.
        if squashing_config.check_author {
            let author = match side_commits.first() {
                Some(p) => p.author().await?,
                None => {
                    return Ok((false, vec![]));
                }
            };

            for side_commit in side_commits.iter() {
                // Author is different, bail
                if author != side_commit.author().await? {
                    return Ok((false, vec![]));
                }
            }
        }

        // All checks passed, squashing is possible
        Ok((true, side_commits))
    }

    // Creates move commits on top of the merge parents in the source that
    // hasn't already been synced to targets (all but one). These move commits
    // put all source files into a correct places in a target so the file
    // history is correct.
    async fn create_move_commits(
        &self,
        ctx: &CoreContext,
        target: &Target,
        source_cs: &BonsaiChangeset,
        commit_remapping_state: &CommitRemappingState,
        target_repo: &RepoContext,
        source_name: &SourceName,
        source: &Source,
    ) -> Result<Vec<SourceAndMovedChangesets>, MegarepoError> {
        let latest_synced_cs_id =
            find_latest_synced_cs_id(commit_remapping_state, source_name, target)?;

        // All parents except the one that's already synced to the target
        let side_parents = source_cs.parents().filter(|p| *p != latest_synced_cs_id);
        let mover = create_source_to_target_multi_mover(source.mapping.clone())
            .map_err(MegarepoError::request)?;
        let directory_mover = create_directory_source_to_target_multi_mover(source.mapping.clone())
            .map_err(MegarepoError::request)?;
        let moved_commits = stream::iter(side_parents)
            .map(|parent| {
                self.create_single_move_commit(
                    ctx,
                    target_repo.blob_repo(),
                    parent.clone(),
                    &mover,
                    &directory_mover,
                    Default::default(),
                    source_name,
                )
            })
            .buffer_unordered(MERGE_COMMIT_MOVES_CONCURRENCY)
            .try_collect::<Vec<_>>()
            .await?;

        save_bonsai_changesets(
            moved_commits.iter().map(|css| css.moved.clone()).collect(),
            ctx.clone(),
            target_repo.blob_repo(),
        )
        .await?;

        let mutable_renames_count: usize = moved_commits
            .iter()
            .map(|css| css.mutable_renames.len())
            .sum();
        let mut scuba = ctx.scuba().clone();
        scuba.add("mutable_renames_count", mutable_renames_count);
        scuba.log_with_msg("Started saving mutable renames", None);
        self.save_mutable_renames(
            ctx,
            target_repo.inner_repo().changesets(),
            self.mutable_renames,
            moved_commits.iter().map(|css| &css.mutable_renames),
        )
        .await?;
        scuba.log_with_msg("Saved mutable renames", None);

        Ok(moved_commits)
    }

    // If that sync_changeset() call was successful, but failed to send
    // successful result to the client (e.g. network issues) then
    // client will retry a request. We need to detect this situation and
    // send a successful response to the client.
    async fn check_if_this_method_has_already_succeeded(
        &self,
        ctx: &CoreContext,
        source_cs_id: ChangesetId,
        source_name: &SourceName,
        (expected_target_location, actual_target_location): (ChangesetId, ChangesetId),
        repo: &RepoContext,
    ) -> Result<ChangesetId, MegarepoError> {
        // Bookmark points a non-expected commit - let's see if changeset it points to was created
        // by a previous sync_changeset call

        // Check that first parent is a target location
        let parents = repo
            .blob_repo()
            .changeset_fetcher()
            .get_parents(ctx.clone(), actual_target_location)
            .await?;
        if parents.get(0) != Some(&expected_target_location) {
            return Err(MegarepoError::request(anyhow!(
                "Neither {} nor its first parent {:?} point to a target location {}",
                actual_target_location,
                parents.get(0),
                expected_target_location,
            )));
        }

        let state = self
            .read_remapping_state_file(ctx, repo, actual_target_location)
            .await?;

        let latest_synced = state.latest_synced_changesets.get(source_name);
        if Some(&source_cs_id) != latest_synced {
            return Err(MegarepoError::request(anyhow!(
                "In target commit {} latest synced source commit is {:?}, but expected {}",
                actual_target_location,
                latest_synced,
                source_cs_id,
            )));
        }

        Ok(actual_target_location)
    }
}

// We allow syncing changeset from a source if one of its parents was the latest synced changeset
// from this source into this target.
async fn validate_can_sync_changeset(
    _ctx: &CoreContext,
    target: &Target,
    source_cs: &BonsaiChangeset,
    commit_remapping_state: &CommitRemappingState,
    _source_repo: &RepoContext,
    source: &Source,
) -> Result<(), MegarepoError> {
    match &source.revision {
        SourceRevision::hash(_) => {
            /* If the revision is hardcoded hash it should be changed using remerge_source */
            return Err(MegarepoError::request(anyhow!(
                "can't sync changeset from source {} because this source points to a changeset",
                source.source_name,
            )));
        }
        SourceRevision::bookmark(_bookmark) => {
            /* If the source is following a git repo branch we can't verify much as the bookmark
            doesn't have to exist in the megarepo */
        }
        SourceRevision::UnknownField(_) => {
            return Err(MegarepoError::internal(anyhow!(
                "unexpected source revision!"
            )));
        }
    };

    let latest_synced_cs_id = find_latest_synced_cs_id(
        commit_remapping_state,
        &SourceName::new(&source.source_name),
        target,
    )?;

    let found = source_cs.parents().find(|p| *p == latest_synced_cs_id);
    if found.is_none() {
        return Err(MegarepoError::request(anyhow!(
            "Can't sync {}, because latest synced commit is not a parent of this commit. \
                    Latest synced source changeset is {}",
            source_cs.get_changeset_id(),
            latest_synced_cs_id,
        )));
    }
    Ok(())
}

async fn sync_changeset_to_target(
    ctx: &CoreContext,
    mapping: &SourceMappingRules,
    source: &SourceName,
    source_repo: &BlobRepo,
    source_cs: BonsaiChangeset,
    target_repo: &BlobRepo,
    target_cs_id: ChangesetId,
    target: &Target,
    mut state: CommitRemappingState,
    merge_mode: MergeMode,
) -> Result<ChangesetId, MegarepoError> {
    let mover =
        create_source_to_target_multi_mover(mapping.clone()).map_err(MegarepoError::internal)?;

    let source_cs_id = source_cs.get_changeset_id();
    // Create a new commit using a mover
    let source_cs_mut = source_cs.into_mut();
    let latest_synced_cs_id = find_latest_synced_cs_id(&state, source, target)?;

    let mut rewritten_commit = match merge_mode {
        MergeMode::ExtraMoveCommits {
            side_parents_move_commits,
        } => {
            let mut remapped_parents = HashMap::new();

            remapped_parents.insert(latest_synced_cs_id, target_cs_id);
            for css in side_parents_move_commits.iter() {
                remapped_parents.insert(css.source, css.moved.get_changeset_id());
            }
            rewrite_commit(
                ctx, // this is already a reference
                source_cs_mut,
                &remapped_parents,
                mover,
                source_repo,
                // In case of octopus merges only first two parent get preserved during
                // hg derivation. This ensures that mainline is within those two so is
                // represented in the commit graph and the sync is a fast-forward move.
                Some(target_cs_id),
                CommitRewrittenToEmpty::Discard,
            )
            .await
            .map_err(MegarepoError::internal)?
            .ok_or_else(|| {
                MegarepoError::internal(anyhow!(
                    "failed to rewrite commit {}, target: {:?}",
                    source_cs_id,
                    target
                ))
            })
        }
        MergeMode::Squashed { side_commits } => {
            let side_commits_info = stream::iter(side_commits.into_iter())
                .map(|cs_ctx| async move {
                    let hash = match cs_ctx.git_sha1().await? {
                        None => format!("HG hash: {}", cs_ctx.id()),
                        Some(hash) => format!("Git hash: {}", hash),
                    };
                    let author_date = cs_ctx.author_date().await?;
                    let message = cs_ctx.message().await?;
                    let title = message.trim_start().lines().next().unwrap_or("");
                    Ok::<_, MegarepoError>(format!(" * {}\t{}\t{}", hash, author_date, title))
                })
                .buffer_unordered(100)
                .try_collect()
                .await?;
            rewrite_as_squashed_commit(
                ctx,
                source_repo,
                source_cs_id,
                (latest_synced_cs_id, target_cs_id),
                source_cs_mut,
                mover,
                side_commits_info,
            )
            .await
            .map_err(MegarepoError::internal)?
            .ok_or_else(|| {
                MegarepoError::internal(anyhow!(
                    "failed to rewrite as squashed commit {}, target: {:?}",
                    source_cs_id,
                    target
                ))
            })
        }
    }?;

    state.set_source_changeset(source.clone(), source_cs_id);
    state
        .save_in_changeset(ctx, target_repo, &mut rewritten_commit)
        .await?;

    let rewritten_commit = rewritten_commit.freeze().map_err(MegarepoError::internal)?;
    let target_cs_id = rewritten_commit.get_changeset_id();
    upload_commits(ctx, vec![rewritten_commit], source_repo, target_repo)
        .await
        .map_err(MegarepoError::internal)?;

    Ok(target_cs_id)
}

fn find_latest_synced_cs_id(
    commit_remapping_state: &CommitRemappingState,
    source_name: &SourceName,
    target: &Target,
) -> Result<ChangesetId, MegarepoError> {
    let maybe_latest_synced_cs_id = commit_remapping_state.get_latest_synced_changeset(source_name);
    if let Some(latest_synced_cs_id) = maybe_latest_synced_cs_id {
        Ok(latest_synced_cs_id.clone())
    } else {
        Err(MegarepoError::internal(anyhow!(
            "Source {:?} was not synced into target {:?}",
            source_name,
            target
        )))
    }
}

#[cfg(test)]
mod test {
    use anyhow::Error;
    use fbinit::FacebookInit;
    use maplit::hashmap;
    use megarepo_mapping::REMAPPING_STATE_FILE;
    use mononoke_types::FileChange;
    use mononoke_types::MPath;
    use tests_utils::bookmark;
    use tests_utils::list_working_copy_utf8;
    use tests_utils::resolve_cs_id;
    use tests_utils::CreateCommitContext;

    use super::*;
    use crate::megarepo_test_utils::MegarepoTest;
    use crate::megarepo_test_utils::SyncTargetConfigBuilder;

    #[fbinit::test]
    async fn test_sync_changeset_simple(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut test = MegarepoTest::new(&ctx).await?;
        let target: Target = test.target("target".to_string());

        let source_name = SourceName::new("source_1");
        let version = "version_1".to_string();
        SyncTargetConfigBuilder::new(test.repo_id(), target.clone(), version.clone())
            .source_builder(source_name.clone())
            .set_prefix_bookmark_to_source_name()
            .build_source()?
            .build(&mut test.configs_storage);

        println!("Create initial source commit and bookmark");
        let init_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
            .add_file("file", "content")
            .commit()
            .await?;

        bookmark(&ctx, &test.blobrepo, source_name.to_string())
            .set_to(init_source_cs_id)
            .await?;

        let latest_target_cs_id = test
            .prepare_initial_commit_in_target(&ctx, &version, &target)
            .await?;

        let configs_storage: Arc<dyn MononokeMegarepoConfigs> = Arc::new(test.configs_storage);
        let sync_changeset = SyncChangeset::new(
            &configs_storage,
            &test.mononoke,
            &test.megarepo_mapping,
            &test.mutable_renames,
        );
        println!("Trying to sync already synced commit again");
        let res = sync_changeset
            .sync(
                &ctx,
                init_source_cs_id,
                &source_name,
                &target,
                latest_target_cs_id,
            )
            .await;
        assert!(res.is_err());

        let source_cs_id = CreateCommitContext::new(&ctx, &test.blobrepo, vec![init_source_cs_id])
            .add_file("anotherfile", "anothercontent")
            .commit()
            .await?;

        bookmark(&ctx, &test.blobrepo, source_name.to_string())
            .set_to(source_cs_id)
            .await?;

        println!("Syncing new commit");
        sync_changeset
            .sync(
                &ctx,
                source_cs_id,
                &source_name,
                &target,
                latest_target_cs_id,
            )
            .await?;

        let cs_id = resolve_cs_id(&ctx, &test.blobrepo, "target").await?;
        let mut wc = list_working_copy_utf8(&ctx, &test.blobrepo, cs_id).await?;

        // Remove file with commit remapping state because it's never present in source
        wc.remove(&MPath::new(REMAPPING_STATE_FILE)?);

        assert_eq!(
            wc,
            hashmap! {
                MPath::new("source_1/file")? => "content".to_string(),
                MPath::new("source_1/anotherfile")? => "anothercontent".to_string(),
            }
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_sync_changeset_octopus_merge(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut test = MegarepoTest::new(&ctx).await?;
        let target: Target = test.target("target".to_string());

        let source_name = SourceName::new("source_1");
        let version = "version_1".to_string();
        SyncTargetConfigBuilder::new(test.repo_id(), target.clone(), version.clone())
            .source_builder(source_name.clone())
            .set_prefix_bookmark_to_source_name()
            .copyfile("file", "copyfile")
            .build_source()?
            .build(&mut test.configs_storage);

        println!("Create initial source commit and bookmark");
        let init_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
            .add_file("file", "content")
            .commit()
            .await?;

        bookmark(&ctx, &test.blobrepo, source_name.to_string())
            .set_to(init_source_cs_id)
            .await?;

        let latest_target_cs_id = test
            .prepare_initial_commit_in_target(&ctx, &version, &target)
            .await?;

        let configs_storage: Arc<dyn MononokeMegarepoConfigs> = Arc::new(test.configs_storage);
        let sync_changeset = SyncChangeset::new(
            &configs_storage,
            &test.mononoke,
            &test.megarepo_mapping,
            &test.mutable_renames,
        );

        let merge_parent_1_source =
            CreateCommitContext::new(&ctx, &test.blobrepo, vec![init_source_cs_id])
                .add_file("file", "anothercontent")
                .add_file("file_from_parent_1", "parent_1")
                .commit()
                .await?;

        let merge_parent_2_source =
            CreateCommitContext::new(&ctx, &test.blobrepo, vec![init_source_cs_id])
                .add_file("file", "totallydifferentcontent")
                .add_file("file_from_parent_2", "parent_2")
                .commit()
                .await?;

        let merge_parent_3_source =
            CreateCommitContext::new(&ctx, &test.blobrepo, vec![init_source_cs_id])
                .add_file("file", "contentfromthirdparent")
                .add_file("file_from_parent_3", "parent_3")
                .commit()
                .await?;

        let merge_source = CreateCommitContext::new(
            &ctx,
            &test.blobrepo,
            vec![
                merge_parent_2_source,
                merge_parent_3_source,
                // Commit parent comming from the target last to ensure that
                // parent reordering works as expected.
                merge_parent_1_source,
            ],
        )
        .add_file("file", "mergeresolution")
        .add_file_with_copy_info(
            "copy_of_file",
            "totallydifferentcontent",
            (merge_parent_2_source, "file"),
        )
        .commit()
        .await?;

        bookmark(&ctx, &test.blobrepo, source_name.to_string())
            .set_to(merge_parent_1_source)
            .await?;
        println!("Syncing first merge parent");
        let merge_parent_1_target = sync_changeset
            .sync(
                &ctx,
                merge_parent_1_source,
                &source_name,
                &target,
                latest_target_cs_id,
            )
            .await?;

        bookmark(&ctx, &test.blobrepo, source_name.to_string())
            .set_to(merge_source)
            .await?;
        println!("Syncing merge commit parent");
        let merge_target = sync_changeset
            .sync(
                &ctx,
                merge_source,
                &source_name,
                &target,
                merge_parent_1_target,
            )
            .await?;

        let mut wc = list_working_copy_utf8(&ctx, &test.blobrepo, merge_target).await?;

        // Remove file with commit remapping state because it's never present in source
        wc.remove(&MPath::new(REMAPPING_STATE_FILE)?);

        assert_eq!(
            wc,
            hashmap! {
                MPath::new("source_1/file")? => "mergeresolution".to_string(),
                MPath::new("source_1/file_from_parent_1")? => "parent_1".to_string(),
                MPath::new("source_1/file_from_parent_2")? => "parent_2".to_string(),
                MPath::new("source_1/file_from_parent_3")? => "parent_3".to_string(),
                MPath::new("source_1/copy_of_file")? => "totallydifferentcontent".to_string(),
                MPath::new("copyfile")? => "mergeresolution".to_string(),
            }
        );

        let merge_target_cs = merge_target.load(&ctx, &test.blobrepo.blobstore()).await?;

        let copied_file_change_from_bonsai = match merge_target_cs
            .file_changes()
            .find(|(p, _)| p == &&MPath::new("source_1/copy_of_file").unwrap())
            .unwrap()
            .1
        {
            FileChange::Change(tc) => tc,
            _ => panic!(),
        };
        assert_eq!(
            copied_file_change_from_bonsai.copy_from().unwrap().0,
            MPath::new("source_1/file")?
        );

        // All parents are preserved.
        assert_eq!(merge_target_cs.parents().count(), 3);

        // The parent from target comes first.
        assert_eq!(
            merge_target_cs.parents().next().unwrap(),
            merge_parent_1_target,
        );
        Ok(())
    }

    #[fbinit::test]
    async fn test_sync_changeset_two_sources_one_with_diamond_merge(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut test = MegarepoTest::new(&ctx).await?;
        let target: Target = test.target("target".to_string());

        let source1_name = SourceName::new("source_1");
        let source2_name = SourceName::new("source_2");
        let version = "version_1".to_string();
        SyncTargetConfigBuilder::new(test.repo_id(), target.clone(), version.clone())
            .source_builder(source1_name.clone())
            .set_prefix_bookmark_to_source_name()
            .build_source()?
            .source_builder(source2_name.clone())
            .set_prefix_bookmark_to_source_name()
            .build_source()?
            .build(&mut test.configs_storage);

        println!("Create initial first source commit and bookmark");
        let init_source1_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
            .add_file("file1", "content1")
            .commit()
            .await?;

        bookmark(&ctx, &test.blobrepo, source1_name.to_string())
            .set_to(init_source1_cs_id)
            .await?;

        println!("Create initial second source commit and bookmark");
        let init_source2_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
            .add_file("file2", "content2")
            .commit()
            .await?;

        bookmark(&ctx, &test.blobrepo, source2_name.to_string())
            .set_to(init_source2_cs_id)
            .await?;

        let mut latest_target_cs_id = test
            .prepare_initial_commit_in_target(&ctx, &version, &target)
            .await?;

        print!("Syncing one commit to each of sources... 1");
        let configs_storage: Arc<dyn MononokeMegarepoConfigs> = Arc::new(test.configs_storage);
        let sync_changeset = SyncChangeset::new(
            &configs_storage,
            &test.mononoke,
            &test.megarepo_mapping,
            &test.mutable_renames,
        );
        let source1_cs_id =
            CreateCommitContext::new(&ctx, &test.blobrepo, vec![init_source1_cs_id])
                .add_file("anotherfile1", "anothercontent")
                .commit()
                .await?;
        bookmark(&ctx, &test.blobrepo, source1_name.to_string())
            .set_to(source1_cs_id)
            .await?;
        latest_target_cs_id = sync_changeset
            .sync(
                &ctx,
                source1_cs_id,
                &source1_name,
                &target,
                latest_target_cs_id,
            )
            .await?;
        println!(", 2");

        let source2_cs_id =
            CreateCommitContext::new(&ctx, &test.blobrepo, vec![init_source2_cs_id])
                .add_file("anotherfile2", "anothercontent")
                .commit()
                .await?;
        bookmark(&ctx, &test.blobrepo, source2_name.to_string())
            .set_to(source2_cs_id)
            .await?;
        latest_target_cs_id = sync_changeset
            .sync(
                &ctx,
                source2_cs_id,
                &source2_name,
                &target,
                latest_target_cs_id,
            )
            .await?;

        println!("Trying to sync already synced commit again");
        let res = sync_changeset
            .sync(
                &ctx,
                source1_cs_id,
                &source1_name,
                &target,
                latest_target_cs_id,
            )
            .await;
        assert!(res.is_err());
        println!("Trying to sync a diamond merge commit");

        let source1_diamond_merge_cs_id = CreateCommitContext::new(
            &ctx,
            &test.blobrepo,
            vec![source1_cs_id, init_source1_cs_id],
        )
        .add_file("anotherfile1", "content_from_diamond_merge")
        .commit()
        .await?;
        bookmark(&ctx, &test.blobrepo, source1_name.to_string())
            .set_to(source1_diamond_merge_cs_id)
            .await?;
        let _diamond_merge_synced = sync_changeset
            .sync(
                &ctx,
                source1_diamond_merge_cs_id,
                &source1_name,
                &target,
                latest_target_cs_id,
            )
            .await?;

        let target_cs_id = resolve_cs_id(&ctx, &test.blobrepo, "target").await?;
        let mut wc = list_working_copy_utf8(&ctx, &test.blobrepo, target_cs_id).await?;

        // Remove file with commit remapping state because it's never present in source
        wc.remove(&MPath::new(REMAPPING_STATE_FILE)?);

        assert_eq!(
            wc,
            hashmap! {
                MPath::new("source_1/file1")? => "content1".to_string(),
                MPath::new("source_1/anotherfile1")? => "content_from_diamond_merge".to_string(),
                MPath::new("source_2/file2")? => "content2".to_string(),
                MPath::new("source_2/anotherfile2")? => "anothercontent".to_string(),
            }
        );

        let target_cs = target_cs_id.load(&ctx, &test.blobrepo.blobstore()).await?;
        // All parents are preserved.
        assert_eq!(target_cs.parents().count(), 2);

        Ok(())
    }

    #[fbinit::test]
    async fn test_sync_changeset_repeat_same_request(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut test = MegarepoTest::new(&ctx).await?;
        let target: Target = test.target("target".to_string());

        let source_name = SourceName::new("source_1");
        let version = "version_1".to_string();
        SyncTargetConfigBuilder::new(test.repo_id(), target.clone(), version.clone())
            .source_builder(source_name.clone())
            .set_prefix_bookmark_to_source_name()
            .build_source()?
            .build(&mut test.configs_storage);

        println!("Create initial source commit and bookmark");
        let init_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
            .add_file("file", "content")
            .commit()
            .await?;

        bookmark(&ctx, &test.blobrepo, source_name.to_string())
            .set_to(init_source_cs_id)
            .await?;

        let latest_target_cs_id = test
            .prepare_initial_commit_in_target(&ctx, &version, &target)
            .await?;

        let configs_storage: Arc<dyn MononokeMegarepoConfigs> = Arc::new(test.configs_storage);
        let sync_changeset = SyncChangeset::new(
            &configs_storage,
            &test.mononoke,
            &test.megarepo_mapping,
            &test.mutable_renames,
        );

        let source_cs_id = CreateCommitContext::new(&ctx, &test.blobrepo, vec![init_source_cs_id])
            .add_file("anotherfile", "anothercontent")
            .commit()
            .await?;

        bookmark(&ctx, &test.blobrepo, source_name.to_string())
            .set_to(source_cs_id)
            .await?;

        println!("Syncing new commit");
        let res1 = sync_changeset
            .sync(
                &ctx,
                source_cs_id,
                &source_name,
                &target,
                latest_target_cs_id,
            )
            .await?;

        println!("Now syncing the same commit again - should succeed");
        let res2 = sync_changeset
            .sync(
                &ctx,
                source_cs_id,
                &source_name,
                &target,
                latest_target_cs_id,
            )
            .await?;

        assert_eq!(res1, res2);

        Ok(())
    }

    #[fbinit::test]
    async fn test_sync_changeset_squash_commit(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut test = MegarepoTest::new(&ctx).await?;
        let target: Target = test.target("target".to_string());

        let source_name = SourceName::new("source_1");
        let version = "version_1".to_string();
        SyncTargetConfigBuilder::new(test.repo_id(), target.clone(), version.clone())
            .source_builder(source_name.clone())
            .set_prefix_bookmark_to_source_name()
            .merge_mode(megarepo_config::MergeMode::squashed(
                megarepo_config::Squashed { squash_limit: 3 },
            ))
            .build_source()?
            .build(&mut test.configs_storage);

        println!("Create initial source commit and bookmark");
        let init_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
            .add_file("file", "content")
            .commit()
            .await?;

        bookmark(&ctx, &test.blobrepo, source_name.to_string())
            .set_to(init_source_cs_id)
            .await?;

        let latest_target_cs_id = test
            .prepare_initial_commit_in_target(&ctx, &version, &target)
            .await?;

        let configs_storage: Arc<dyn MononokeMegarepoConfigs> = Arc::new(test.configs_storage);
        let sync_changeset = SyncChangeset::new(
            &configs_storage,
            &test.mononoke,
            &test.megarepo_mapping,
            &test.mutable_renames,
        );

        let main_line = CreateCommitContext::new(&ctx, &test.blobrepo, vec![init_source_cs_id])
            .add_file("file_in_mainline", "mainline1")
            .commit()
            .await?;

        bookmark(&ctx, &test.blobrepo, source_name.to_string())
            .set_to(main_line)
            .await?;
        let main_line_target = sync_changeset
            .sync(&ctx, main_line, &source_name, &target, latest_target_cs_id)
            .await?;

        let side_branch_1 = CreateCommitContext::new(&ctx, &test.blobrepo, vec![init_source_cs_id])
            .add_file("file", "totallydifferentcontent")
            .add_file("file_in_sidebranch_1", "sidebranch1")
            .commit()
            .await?;

        let side_branch_2 = CreateCommitContext::new(&ctx, &test.blobrepo, vec![side_branch_1])
            .add_file("file", "amended")
            .add_file("file_in_sidebranch_2", "sidebranch2")
            .commit()
            .await?;

        let merge = CreateCommitContext::new(&ctx, &test.blobrepo, vec![side_branch_2, main_line])
            .add_file("file", "mergeresolution")
            .commit()
            .await?;
        println!("Syncing merge");
        bookmark(&ctx, &test.blobrepo, source_name.to_string())
            .set_to(merge)
            .await?;
        let merge_target = sync_changeset
            .sync(&ctx, merge, &source_name, &target, main_line_target)
            .await?;

        let _mcs = merge.load(&ctx, test.blobrepo.blobstore()).await?;

        // Find source repo and changeset that we need to sync
        let target_repo = sync_changeset.find_repo_by_id(&ctx, target.repo_id).await?;
        let merge_cs = merge_target
            .load(&ctx, target_repo.blob_repo().blobstore())
            .await?;

        let parents: Vec<_> = merge_cs.parents().collect();

        let mut wc = list_working_copy_utf8(&ctx, &test.blobrepo, merge_target).await?;

        // Remove file with commit remapping state because it's never present in source
        wc.remove(&MPath::new(REMAPPING_STATE_FILE)?);

        assert_eq!(parents.len(), 1);

        assert_eq!(
            wc,
            hashmap! {
                MPath::new("source_1/file")? => "mergeresolution".to_string(),
                MPath::new("source_1/file_in_sidebranch_1")? => "sidebranch1".to_string(),
                MPath::new("source_1/file_in_sidebranch_2")? => "sidebranch2".to_string(),
                MPath::new("source_1/file_in_mainline")? => "mainline1".to_string(),
            }
        );
        Ok(())
    }
}
