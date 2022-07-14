/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use blobrepo::save_bonsai_changesets;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateReason;
use commit_transformation::copy_file_contents;
use context::CoreContext;
use cross_repo_sync::types::Source;
use cross_repo_sync::types::Target;
use derived_data::BonsaiDerived;
use fsnodes::RootFsnodeId;
use futures::TryStreamExt;
use manifest::ManifestOps;
use mononoke_types::fsnode::FsnodeFile;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::DateTime;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::MPath;
use slog::info;
use std::collections::BTreeMap;

use crate::common::decode_latest_synced_state_extras;
use crate::common::encode_latest_synced_state_extras;

pub async fn add_source_repo(
    ctx: &CoreContext,
    source_repo: &BlobRepo,
    hyper_repo: &BlobRepo,
    source_bookmark: &Source<BookmarkName>,
    hyper_repo_bookmark: &Target<BookmarkName>,
    per_commit_file_changes_limit: Option<usize>,
) -> Result<(), Error> {
    let source_bcs_id = source_repo
        .get_bonsai_bookmark(ctx.clone(), source_bookmark)
        .await?
        .ok_or_else(|| anyhow!("{} not found", source_bookmark))?;

    // First list files that needs copying to hyper repo and prepend
    // source repo name to each path.
    let root_fsnode_id = RootFsnodeId::derive(ctx, source_repo, source_bcs_id).await?;
    let prefix = MPath::new(source_repo.name())?;
    let leaf_entries = root_fsnode_id
        .fsnode_id()
        .list_leaf_entries(ctx.clone(), source_repo.get_blobstore())
        // Shift path
        .map_ok(|(path, fsnode_file)| (prefix.join(&path), fsnode_file))
        .try_collect::<Vec<_>>()
        .await?;

    let parent = hyper_repo
        .get_bonsai_bookmark(ctx.clone(), hyper_repo_bookmark)
        .await?;
    if let Some(parent) = parent {
        ensure_no_file_intersection(ctx, hyper_repo, parent, &leaf_entries).await?;
    }

    info!(
        ctx.logger(),
        "found {} files in source repo, copying them to hyper repo...",
        leaf_entries.len()
    );
    copy_file_contents(
        ctx,
        source_repo,
        hyper_repo,
        leaf_entries.iter().map(|(_, fsnode)| *fsnode.content_id()),
        |i| {
            if i % 100 == 0 {
                info!(ctx.logger(), "copied {} files", i);
            }
        },
    )
    .await?;
    info!(ctx.logger(), "Finished copying");

    // Now creating a bonsai commit...
    let cs_id = create_new_bonsai_changeset_for_source_repo(
        ctx,
        source_repo,
        hyper_repo,
        leaf_entries.into_iter().map(|(path, file_change)| {
            (
                path,
                (
                    *file_change.file_type(),
                    file_change.size(),
                    *file_change.content_id(),
                ),
            )
        }),
        parent,
        source_bcs_id,
        per_commit_file_changes_limit,
    )
    .await?;

    // ... and move bookmark to point to this commit.
    let mut txn = hyper_repo.update_bookmark_transaction(ctx.clone());
    match parent {
        Some(parent) => {
            txn.update(
                hyper_repo_bookmark,
                cs_id,
                parent,
                BookmarkUpdateReason::ManualMove,
            )?;
        }
        None => {
            txn.create(hyper_repo_bookmark, cs_id, BookmarkUpdateReason::ManualMove)?;
        }
    };
    let success = txn.commit().await?;
    if !success {
        return Err(anyhow!(
            "failed to move {} bookmark in hyper repo",
            hyper_repo_bookmark
        ));
    }

    Ok(())
}

async fn create_new_bonsai_changeset_for_source_repo(
    ctx: &CoreContext,
    source_repo: &BlobRepo,
    hyper_repo: &BlobRepo,
    leaf_entries: impl Iterator<Item = (MPath, (FileType, u64, ContentId))>,
    hyper_parent: Option<ChangesetId>,
    source_bcs_id: ChangesetId,
    limit: Option<usize>,
) -> Result<ChangesetId, Error> {
    let file_changes = leaf_entries
        .map(|(path, (ty, size, content_id))| {
            (path, FileChange::tracked(content_id, ty, size, None))
        })
        .collect::<Vec<_>>();

    let file_changes_per_commit = match limit {
        // `if !file_changes.is_empty()` makes sure that if file changes are empty then we still
        // create a bonsai changeset for them.
        Some(limit) if !file_changes.is_empty() => file_changes.chunks(limit).collect::<Vec<_>>(),
        _ => {
            vec![file_changes.as_ref()]
        }
    };

    let len = file_changes_per_commit.len();
    info!(ctx.logger(), "about to create {} commits", len);

    let mut bcss = vec![];
    let mut parent = hyper_parent;
    for (idx, chunk) in file_changes_per_commit.into_iter().enumerate() {
        let mut bcs = BonsaiChangesetMut {
            parents: parent.into_iter().collect(),
            author: "hyperrepo".to_string(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message: format!(
                "Introducing new source repo {} to hyper repo {}, idx {} out of {}",
                source_repo.name(),
                hyper_repo.name(),
                idx,
                len
            ),
            file_changes: chunk.iter().cloned().collect(),
            is_snapshot: false,
            extra: Default::default(),
        };

        if idx + 1 == len {
            // Intentionally add extras only on top commit to make it easier to tell
            // apart last commit (i.e. commit that has all files from the source)
            // from intermediate commits.
            let mut extra = match hyper_parent {
                Some(parent) => {
                    let parent = parent.load(ctx, &hyper_repo.get_blobstore()).await?;
                    decode_latest_synced_state_extras(parent.extra())?
                }
                None => Default::default(),
            };

            // Append extra that shows what was the latest replayed commit from a given source-repo
            extra.insert(source_repo.name().to_string(), source_bcs_id);

            bcs.extra = encode_latest_synced_state_extras(&extra);
        }

        let bcs = bcs.freeze()?;
        info!(ctx.logger(), "creating {}", bcs.get_changeset_id());
        parent = Some(bcs.get_changeset_id());
        bcss.push(bcs);
    }

    let cs_id = bcss
        .last()
        .ok_or_else(|| anyhow!("no commits were created"))?
        .get_changeset_id();
    save_bonsai_changesets(bcss, ctx.clone(), &hyper_repo).await?;

    Ok(cs_id)
}

async fn ensure_no_file_intersection(
    ctx: &CoreContext,
    hyper_repo: &BlobRepo,
    hyper_repo_cs_id: ChangesetId,
    leaf_entries: &[(MPath, FsnodeFile)],
) -> Result<(), Error> {
    let root_fsnode_id = RootFsnodeId::derive(ctx, hyper_repo, hyper_repo_cs_id).await?;
    let hyper_repo_files = root_fsnode_id
        .fsnode_id()
        .list_leaf_entries(ctx.clone(), hyper_repo.get_blobstore())
        .try_collect::<BTreeMap<_, _>>()
        .await?;

    for (path, _) in leaf_entries {
        if hyper_repo_files.contains_key(path) {
            return Err(anyhow!("File {} is already present in hyper repo!", path));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fbinit::FacebookInit;
    use maplit::hashmap;
    use mononoke_types::MPath;
    use mononoke_types::RepositoryId;
    use test_repo_factory::TestRepoFactory;
    use tests_utils::bookmark;
    use tests_utils::list_working_copy_utf8;
    use tests_utils::resolve_cs_id;
    use tests_utils::CreateCommitContext;

    #[fbinit::test]
    async fn add_source_repo_simple(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut test_repo_factory = TestRepoFactory::new(fb)?;

        let source_repo: BlobRepo = test_repo_factory
            .with_id(RepositoryId::new(0))
            .with_name("source_repo")
            .build()?;
        let second_source_repo: BlobRepo = test_repo_factory
            .with_id(RepositoryId::new(1))
            .with_name("second_source_repo")
            .build()?;
        let hyper_repo: BlobRepo = test_repo_factory
            .with_id(RepositoryId::new(2))
            .with_name("hyper_repo")
            .build()?;

        let book = Source(BookmarkName::new("main")?);
        let hyper_repo_book = Target(BookmarkName::new("hyper_repo_main")?);
        let res = add_source_repo(
            &ctx,
            &source_repo,
            &hyper_repo,
            &book,
            &hyper_repo_book,
            None,
        )
        .await;
        // Expect it to fail because source repo doesn't have a bookmark
        assert!(res.is_err());

        let root_cs_id = CreateCommitContext::new_root(&ctx, &source_repo)
            .add_file("1.txt", "content")
            .commit()
            .await?;

        bookmark(&ctx, &source_repo, &book.0)
            .set_to(root_cs_id)
            .await?;
        add_source_repo(
            &ctx,
            &source_repo,
            &hyper_repo,
            &book,
            &hyper_repo_book,
            None,
        )
        .await?;

        assert_eq!(
            list_working_copy_utf8(
                &ctx,
                &hyper_repo,
                resolve_cs_id(&ctx, &hyper_repo, &hyper_repo_book.0).await?,
            )
            .await?,
            hashmap! {
                MPath::new("source_repo/1.txt")? => "content".to_string(),
            }
        );

        // Now create a commit in a second repo and add it to a second repo
        let second_root_cs_id = CreateCommitContext::new_root(&ctx, &second_source_repo)
            .add_file("2.txt", "content_2")
            .commit()
            .await?;

        bookmark(&ctx, &second_source_repo, &book.0)
            .set_to(second_root_cs_id)
            .await?;
        add_source_repo(
            &ctx,
            &second_source_repo,
            &hyper_repo,
            &book,
            &hyper_repo_book,
            None,
        )
        .await?;
        assert_eq!(
            list_working_copy_utf8(
                &ctx,
                &hyper_repo,
                resolve_cs_id(&ctx, &hyper_repo, &hyper_repo_book.0).await?,
            )
            .await?,
            hashmap! {
                MPath::new("source_repo/1.txt")? => "content".to_string(),
                MPath::new("second_source_repo/2.txt")? => "content_2".to_string(),
            }
        );

        Ok(())
    }

    #[fbinit::test]
    async fn add_source_repo_should_fail_file_intersections(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut test_repo_factory = TestRepoFactory::new(fb)?;

        let source_repo: BlobRepo = test_repo_factory
            .with_id(RepositoryId::new(0))
            .with_name("source_repo")
            .build()?;
        let hyper_repo: BlobRepo = test_repo_factory
            .with_id(RepositoryId::new(2))
            .with_name("hyper_repo")
            .build()?;

        let root_cs_id = CreateCommitContext::new_root(&ctx, &source_repo)
            .add_file("1.txt", "content")
            .commit()
            .await?;
        let book = Source(BookmarkName::new("main")?);
        let hyper_repo_book = Target(BookmarkName::new("hyper_repo_main")?);

        bookmark(&ctx, &source_repo, &book.0)
            .set_to(root_cs_id)
            .await?;

        // Create a commit in hyper repo that would clash with files in the source repo
        let root_cs_id = CreateCommitContext::new_root(&ctx, &hyper_repo)
            .add_file("source_repo/1.txt", "content")
            .commit()
            .await?;

        bookmark(&ctx, &hyper_repo, &hyper_repo_book.0)
            .set_to(root_cs_id)
            .await?;

        let res = add_source_repo(
            &ctx,
            &source_repo,
            &hyper_repo,
            &Source(BookmarkName::new("main")?),
            &Target(BookmarkName::new("hyper_repo_main")?),
            None,
        )
        .await;
        assert!(res.is_err());

        Ok(())
    }

    #[fbinit::test]
    async fn add_source_repo_simple_with_limit(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut test_repo_factory = TestRepoFactory::new(fb)?;

        let source_repo: BlobRepo = test_repo_factory
            .with_id(RepositoryId::new(0))
            .with_name("source_repo")
            .build()?;
        let hyper_repo: BlobRepo = test_repo_factory
            .with_id(RepositoryId::new(2))
            .with_name("hyper_repo")
            .build()?;

        let book = Source(BookmarkName::new("main")?);
        let hyper_repo_book = Target(BookmarkName::new("hyper_repo_main")?);

        let root_cs_id = CreateCommitContext::new_root(&ctx, &source_repo)
            .add_file("1.txt", "content_1")
            .add_file("2.txt", "content_2")
            .commit()
            .await?;

        bookmark(&ctx, &source_repo, &book.0)
            .set_to(root_cs_id)
            .await?;
        add_source_repo(
            &ctx,
            &source_repo,
            &hyper_repo,
            &book,
            &hyper_repo_book,
            Some(1),
        )
        .await?;

        let tip = resolve_cs_id(&ctx, &hyper_repo, &hyper_repo_book.0).await?;
        assert_eq!(
            list_working_copy_utf8(&ctx, &hyper_repo, tip,).await?,
            hashmap! {
                MPath::new("source_repo/1.txt")? => "content_1".to_string(),
                MPath::new("source_repo/2.txt")? => "content_2".to_string(),
            }
        );

        let tip = tip.load(&ctx, hyper_repo.blobstore()).await?;
        assert_eq!(
            tip.file_changes().map(|(path, _)| path).collect::<Vec<_>>(),
            vec![&MPath::new("source_repo/2.txt")?],
        );

        Ok(())
    }

    #[fbinit::test]
    async fn add_source_repo_empty_wc(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut test_repo_factory = TestRepoFactory::new(fb)?;

        let source_repo: BlobRepo = test_repo_factory
            .with_id(RepositoryId::new(0))
            .with_name("source_repo")
            .build()?;
        let hyper_repo: BlobRepo = test_repo_factory
            .with_id(RepositoryId::new(2))
            .with_name("hyper_repo")
            .build()?;

        let book = Source(BookmarkName::new("main")?);
        let hyper_repo_book = Target(BookmarkName::new("hyper_repo_main")?);

        let root_cs_id = CreateCommitContext::new_root(&ctx, &source_repo)
            .add_file("1.txt", "content_1")
            .commit()
            .await?;
        let next_cs_id = CreateCommitContext::new(&ctx, &source_repo, vec![root_cs_id])
            .delete_file("1.txt")
            .commit()
            .await?;
        bookmark(&ctx, &source_repo, &book.0)
            .set_to(next_cs_id)
            .await?;

        add_source_repo(
            &ctx,
            &source_repo,
            &hyper_repo,
            &book,
            &hyper_repo_book,
            Some(1),
        )
        .await?;

        let tip = resolve_cs_id(&ctx, &hyper_repo, &hyper_repo_book.0).await?;
        assert_eq!(
            list_working_copy_utf8(&ctx, &hyper_repo, tip,).await?,
            hashmap! {}
        );

        Ok(())
    }
}
