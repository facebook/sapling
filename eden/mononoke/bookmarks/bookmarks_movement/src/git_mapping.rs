/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Functions for maintaining git mappings during bookmark movement.

use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use blobstore::Loadable;
use bonsai_git_mapping::extract_git_sha1_from_bonsai_extra;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_git_mapping::BonsaiGitMappingEntry;
use bookmarks::BookmarkTransactionHook;
use cloned::cloned;
use context::CoreContext;
use futures::future::try_join;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use futures::stream::FuturesOrdered;
use futures::stream::StreamExt;
use metaconfig_types::PushrebaseParams;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;

use crate::BookmarkMovementError;
use crate::Repo;

/// New mapping entries that should be added to the mapping.
struct NewMappingEntries {
    /// New entries to add.
    new_mapping_entries: Vec<BonsaiGitMappingEntry>,

    /// Count of entries that were created from newly uploaded changesets.
    from_new_changesets: usize,

    /// Count of entries that were created from ancestors with no mapping.
    from_ancestors_no_mapping: usize,
}

/// Description of a changeset for which a mapping needs to be generated.
struct ChangesetMappingNeeded<'a> {
    /// Whether knowledge of this need came from this changeset being
    /// a new changeset.
    from_new_changeset: bool,

    /// The changeset id that needs a mapping.
    cs_id: ChangesetId,

    /// The bonsai changeset that needs a mapping.
    bcs: Cow<'a, BonsaiChangeset>,
}

/// Find ancestors of `start` which have git mapping extras but do not
/// have a git mapping entry set in db, and generate their mapping entries.
async fn new_mapping_entries(
    ctx: &CoreContext,
    repo: &impl Repo,
    start: ChangesetId,
    new_changesets: &HashMap<ChangesetId, BonsaiChangeset>,
) -> Result<NewMappingEntries> {
    let mut new_mapping_entries = Vec::new();
    let mut from_new_changesets = 0;
    let mut from_ancestors_no_mapping = 0;

    let mut visited = HashSet::new();
    let mut queue = FuturesOrdered::new();
    let mut get_new_queue_entry = |cs_id: ChangesetId| {
        if visited.insert(cs_id) {
            Some(async move {
                if let Some(bcs) = new_changesets.get(&cs_id) {
                    Ok::<_, Error>(Some(ChangesetMappingNeeded {
                        from_new_changeset: true,
                        cs_id,
                        bcs: Cow::Borrowed(bcs),
                    }))
                } else {
                    let bcs_fut = {
                        cloned!(ctx, repo);
                        async move {
                            cs_id
                                .load(&ctx, repo.repo_blobstore())
                                .map_err(Error::from)
                                .await
                        }
                    };

                    let mapping_fut = repo.bonsai_git_mapping().get(ctx, cs_id.into());

                    let (bcs, git_mapping) = try_join(bcs_fut, mapping_fut).await?;
                    if git_mapping.is_empty() {
                        Ok(Some(ChangesetMappingNeeded {
                            from_new_changeset: false,
                            cs_id,
                            bcs: Cow::Owned(bcs),
                        }))
                    } else {
                        // The mapping is already known for this changeset, so
                        // generating the mapping is not needed.
                        Ok(None)
                    }
                }
            })
        } else {
            None
        }
    };

    queue.extend(get_new_queue_entry(start));

    while let Some(entry) = queue.next().await {
        let needed = match entry? {
            Some(cs_mapping_needed) => cs_mapping_needed,
            None => {
                // The mapping is already known for this entry, so we can stop
                // traversing here.
                continue;
            }
        };

        let git_sha1 = match extract_git_sha1_from_bonsai_extra(needed.bcs.extra())
            .with_context(|| format!("Failed to extract Git Sha1 from {}", needed.cs_id))?
        {
            Some(git_sha1) => git_sha1,
            None => {
                // Don't traverse past commits that do not have git sha1 set
                // This is done deliberately to avoid retraversing these commits over
                // and over each time new mappings are added.
                continue;
            }
        };

        for p in needed.bcs.parents() {
            queue.extend(get_new_queue_entry(p));
        }
        if needed.from_new_changeset {
            from_new_changesets += 1;
        } else {
            from_ancestors_no_mapping += 1;
        }
        let mapping_entry = BonsaiGitMappingEntry {
            git_sha1,
            bcs_id: needed.cs_id,
        };
        new_mapping_entries.push(mapping_entry);
    }

    Ok(NewMappingEntries {
        new_mapping_entries,
        from_new_changesets,
        from_ancestors_no_mapping,
    })
}

fn upload_mapping_entries_bookmark_txn_hook(
    bonsai_git_mapping: Arc<dyn BonsaiGitMapping>,
    entries: NewMappingEntries,
) -> BookmarkTransactionHook {
    let entries = Arc::new(entries);
    Arc::new(move |ctx, sql_txn| {
        // We expect new_changesets + ancestors_no_mapping = inserting.  For
        // pushes, ancestors_no_mapping should be zero.  For requests made via
        // APIs, it may be non-zero.
        ctx.scuba()
            .clone()
            .add("git_mapping_inserting", entries.new_mapping_entries.len())
            .add("git_mapping_new_changesets", entries.from_new_changesets)
            .add(
                "git_mapping_ancestors_no_mapping",
                entries.from_ancestors_no_mapping,
            )
            .log_with_msg("Inserting git mapping", None);

        let bonsai_git_mapping = bonsai_git_mapping.clone();
        let mapping_entries = entries.new_mapping_entries.clone();
        async move {
            let sql_txn = bonsai_git_mapping
                .bulk_add_git_mapping_in_transaction(&ctx, mapping_entries.as_slice(), sql_txn)
                .map_err(Error::from)
                .await?;
            ctx.scuba()
                .clone()
                .log_with_msg("Inserted git mapping", None);
            Ok(sql_txn)
        }
        .boxed()
    })
}

/// Generate a bookmark transaction hook that will populate the git mapping
/// with new entries for the new mapped commits reachable from `new_head`.
///
/// Commits that have just been added to the repository (and so their
/// bonsai changeset is known and it is also known that they do not have a
/// mapping) can be passed in via `new_changesets` to skip checking the
/// mapping for whether the changeset is already present and to avoid
/// fetching the bonsai changeset.
pub(crate) async fn populate_git_mapping_txn_hook(
    ctx: &CoreContext,
    repo: &impl Repo,
    pushrebase_params: &PushrebaseParams,
    new_head: ChangesetId,
    new_changesets: &HashMap<ChangesetId, BonsaiChangeset>,
) -> Result<Option<BookmarkTransactionHook>, BookmarkMovementError> {
    if pushrebase_params.populate_git_mapping {
        let entries = new_mapping_entries(ctx, repo, new_head, new_changesets).await?;
        Ok(Some(upload_mapping_entries_bookmark_txn_hook(
            repo.bonsai_git_mapping_arc(),
            entries,
        )))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use blobrepo::AsBlobRepo;
    use blobrepo::BlobRepo;
    use bonsai_git_mapping::CONVERT_REVISION_EXTRA;
    use bonsai_git_mapping::HGGIT_SOURCE_EXTRA;
    use bookmarks::BookmarkName;
    use bookmarks::BookmarkUpdateReason;
    use bookmarks::BookmarksRef;
    use borrowed::borrowed;
    use fbinit::FacebookInit;
    use maplit::hashmap;
    use maplit::hashset;
    use mononoke_api_types::InnerRepo;
    use mononoke_types::hash::GitSha1;
    use mononoke_types_mocks::hash::FIVES_GIT_SHA1;
    use mononoke_types_mocks::hash::FOURS_GIT_SHA1;
    use mononoke_types_mocks::hash::ONES_GIT_SHA1;
    use mononoke_types_mocks::hash::SIXES_GIT_SHA1;
    use mononoke_types_mocks::hash::THREES_GIT_SHA1;
    use mononoke_types_mocks::hash::TWOS_GIT_SHA1;
    use repo_blobstore::RepoBlobstoreRef;
    use test_repo_factory::TestRepoFactory;
    use tests_utils::drawdag::changes;
    use tests_utils::drawdag::create_from_dag_with_changes;
    use tests_utils::CreateCommitContext;

    fn add_git_extras(
        context: CreateCommitContext<BlobRepo>,
        hash: GitSha1,
    ) -> CreateCommitContext<BlobRepo> {
        context
            .add_extra(
                CONVERT_REVISION_EXTRA.to_string(),
                format!("{}", hash).as_bytes().to_vec(),
            )
            .add_extra(HGGIT_SOURCE_EXTRA.to_string(), b"git".to_vec())
    }

    fn mapping_entries(entries: &Vec<BonsaiGitMappingEntry>) -> HashSet<(ChangesetId, GitSha1)> {
        entries
            .iter()
            .map(|entry| (entry.bcs_id, entry.git_sha1))
            .collect()
    }

    async fn apply_entries(
        ctx: &CoreContext,
        repo: &impl Repo,
        bookmark: &BookmarkName,
        old_target: ChangesetId,
        new_target: ChangesetId,
        entries: NewMappingEntries,
    ) -> Result<()> {
        let mut txn = repo.bookmarks().create_transaction(ctx.clone());
        txn.update(
            bookmark,
            new_target,
            old_target,
            BookmarkUpdateReason::TestMove,
        )?;
        let ok = txn
            .commit_with_hook(upload_mapping_entries_bookmark_txn_hook(
                repo.bonsai_git_mapping_arc(),
                entries,
            ))
            .await?;
        assert!(ok);
        Ok(())
    }

    #[fbinit::test]
    async fn test_new_mapping_entries(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: InnerRepo = TestRepoFactory::new(fb)?.build()?;
        let bookmark = BookmarkName::new("main")?;
        borrowed!(ctx, repo);

        let dag = create_from_dag_with_changes(
            ctx,
            repo.as_blob_repo(),
            r##"
                Z-A-B-C-D
                     \
                      X-Y
                       \
                        E-F
            "##,
            changes! {
                "A" => |c| add_git_extras(c, ONES_GIT_SHA1),
                "B" => |c| add_git_extras(c, TWOS_GIT_SHA1),
                "C" => |c| add_git_extras(c, THREES_GIT_SHA1),
                "D" => |c| add_git_extras(c, FOURS_GIT_SHA1),
                "E" => |c| add_git_extras(c, FIVES_GIT_SHA1),
                "F" => |c| add_git_extras(c, SIXES_GIT_SHA1),
            },
        )
        .await?;

        let a = *dag.get("A").unwrap();
        let b = *dag.get("B").unwrap();
        let c = *dag.get("C").unwrap();
        let d = *dag.get("D").unwrap();
        let e = *dag.get("E").unwrap();
        let f = *dag.get("F").unwrap();
        let y = *dag.get("Y").unwrap();
        let z = *dag.get("Z").unwrap();
        let a_bcs = a.load(ctx, repo.repo_blobstore()).await?;
        let b_bcs = b.load(ctx, repo.repo_blobstore()).await?;
        let f_bcs = f.load(ctx, repo.repo_blobstore()).await?;
        let y_bcs = y.load(ctx, repo.repo_blobstore()).await?;

        let mut txn = repo.bookmarks().create_transaction(ctx.clone());
        txn.create(&bookmark, z, BookmarkUpdateReason::TestMove)?;
        let ok = txn.commit().await?;
        assert!(ok);

        // Initial create with two changesets.
        let entries = new_mapping_entries(
            ctx,
            repo,
            b,
            &hashmap! {
                a => a_bcs,
                b => b_bcs,
            },
        )
        .await?;

        assert_eq!(
            mapping_entries(&entries.new_mapping_entries),
            hashset! { (a, ONES_GIT_SHA1), (b, TWOS_GIT_SHA1) },
        );
        assert_eq!(entries.from_new_changesets, 2);
        assert_eq!(entries.from_ancestors_no_mapping, 0);

        apply_entries(ctx, repo, &bookmark, z, b, entries).await?;

        // Addition using existing changesets.
        let entries = new_mapping_entries(ctx, repo, d, &hashmap! {}).await?;
        assert_eq!(
            mapping_entries(&entries.new_mapping_entries),
            hashset! { (c, THREES_GIT_SHA1), (d, FOURS_GIT_SHA1) },
        );
        assert_eq!(entries.from_new_changesets, 0);
        assert_eq!(entries.from_ancestors_no_mapping, 2);

        apply_entries(ctx, repo, &bookmark, b, d, entries).await?;

        // Move to commits with no mapping.
        let entries = new_mapping_entries(ctx, repo, y, &hashmap! {y => y_bcs}).await?;
        assert_eq!(mapping_entries(&entries.new_mapping_entries), hashset! {});
        assert_eq!(entries.from_new_changesets, 0);
        assert_eq!(entries.from_ancestors_no_mapping, 0);

        apply_entries(ctx, repo, &bookmark, d, y, entries).await?;

        // Move to descendants of commit with no mapping.
        let entries = new_mapping_entries(ctx, repo, f, &hashmap! {f => f_bcs}).await?;
        assert_eq!(
            mapping_entries(&entries.new_mapping_entries),
            hashset! {
                (e, FIVES_GIT_SHA1), (f, SIXES_GIT_SHA1)
            }
        );
        assert_eq!(entries.from_new_changesets, 1);
        assert_eq!(entries.from_ancestors_no_mapping, 1);

        apply_entries(ctx, repo, &bookmark, y, f, entries).await?;

        Ok(())
    }
}
