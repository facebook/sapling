/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Error};
use blobrepo::{save_bonsai_changesets, BlobRepo};
use blobstore::Loadable;
use bookmarks::{BookmarkName, BookmarkUpdateReason};
use commit_transformation::copy_file_contents;
use context::CoreContext;
use derived_data::BonsaiDerived;
use fsnodes::RootFsnodeId;
use futures::TryStreamExt;
use manifest::ManifestOps;
use mononoke_types::{
    fsnode::FsnodeFile, BonsaiChangesetMut, ChangesetId, ContentId, DateTime, FileChange, FileType,
    MPath,
};
use slog::info;
use std::collections::BTreeMap;

use crate::common::{decode_latest_synced_state_extras, encode_latest_synced_state_extras};

pub async fn add_source_repo(
    ctx: &CoreContext,
    source_repo: &BlobRepo,
    hyper_repo: &BlobRepo,
    bookmark_name: &BookmarkName,
) -> Result<(), Error> {
    let source_bcs_id = source_repo
        .get_bonsai_bookmark(ctx.clone(), bookmark_name)
        .await?
        .ok_or_else(|| anyhow!("{} not found", bookmark_name))?;

    // First list files that needs copying to hyper repo and prepend
    // source repo name to each path.
    let root_fsnode_id = RootFsnodeId::derive(&ctx, &source_repo, source_bcs_id).await?;
    let prefix = MPath::new(source_repo.name().to_string())?;
    let leaf_entries = root_fsnode_id
        .fsnode_id()
        .list_leaf_entries(ctx.clone(), source_repo.get_blobstore())
        // Shift path
        .map_ok(|(path, fsnode_file)| (prefix.join(&path), fsnode_file))
        .try_collect::<Vec<_>>()
        .await?;

    let parent = hyper_repo
        .get_bonsai_bookmark(ctx.clone(), bookmark_name)
        .await?;
    if let Some(parent) = parent {
        ensure_no_file_intersection(&ctx, &hyper_repo, parent, &leaf_entries).await?;
    }

    info!(
        ctx.logger(),
        "found {} files in source repo, copying them to hyper repo...",
        leaf_entries.len()
    );
    copy_file_contents(
        &ctx,
        &source_repo,
        &hyper_repo,
        leaf_entries.iter().map(|(_, fsnode)| *fsnode.content_id()),
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
    )
    .await?;

    // ... and move bookmark to point to this commit.
    let mut txn = hyper_repo.update_bookmark_transaction(ctx.clone());
    match parent {
        Some(parent) => {
            txn.update(
                bookmark_name,
                cs_id,
                parent,
                BookmarkUpdateReason::ManualMove,
                None,
            )?;
        }
        None => {
            txn.create(bookmark_name, cs_id, BookmarkUpdateReason::ManualMove, None)?;
        }
    };
    let success = txn.commit().await?;
    if !success {
        return Err(anyhow!(
            "failed to move {} bookmark in hyper repo",
            bookmark_name
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
) -> Result<ChangesetId, Error> {
    let file_changes = leaf_entries.map(|(path, (ty, size, content_id))| {
        (path, FileChange::tracked(content_id, ty, size, None))
    });

    let mut extra = match hyper_parent {
        Some(parent) => {
            let parent = parent.load(ctx, &hyper_repo.get_blobstore()).await?;
            decode_latest_synced_state_extras(parent.extra())?
        }
        None => Default::default(),
    };

    // Append extra that shows what was the latest replayed commit from a given source-repo
    extra.insert(source_repo.name().to_string(), source_bcs_id);

    let bcs = BonsaiChangesetMut {
        parents: hyper_parent.into_iter().collect(),
        author: "hyperrepo".to_string(),
        author_date: DateTime::now(),
        committer: None,
        committer_date: None,
        message: format!(
            "Introducing new source repo {} to hyper repo {}",
            source_repo.name(),
            hyper_repo.name()
        ),
        file_changes: file_changes.collect(),
        is_snapshot: false,
        extra: encode_latest_synced_state_extras(&extra),
    }
    .freeze()?;

    let cs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx.clone(), hyper_repo.clone()).await?;

    Ok(cs_id)
}

async fn ensure_no_file_intersection(
    ctx: &CoreContext,
    hyper_repo: &BlobRepo,
    hyper_repo_cs_id: ChangesetId,
    leaf_entries: &[(MPath, FsnodeFile)],
) -> Result<(), Error> {
    let root_fsnode_id = RootFsnodeId::derive(&ctx, &hyper_repo, hyper_repo_cs_id).await?;
    let hyper_repo_files = root_fsnode_id
        .fsnode_id()
        .list_leaf_entries(ctx.clone(), hyper_repo.get_blobstore())
        .try_collect::<BTreeMap<_, _>>()
        .await?;

    for (path, _) in leaf_entries {
        if hyper_repo_files.contains_key(&path) {
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
    use mononoke_types::{MPath, RepositoryId};
    use test_repo_factory::TestRepoFactory;
    use tests_utils::{bookmark, list_working_copy_utf8, resolve_cs_id, CreateCommitContext};

    #[fbinit::test]
    async fn add_source_repo_simple(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut test_repo_factory = TestRepoFactory::new()?;

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

        let res =
            add_source_repo(&ctx, &source_repo, &hyper_repo, &BookmarkName::new("main")?).await;
        // Expect it to fail because source repo doesn't have a bookmark
        assert!(res.is_err());

        let root_cs_id = CreateCommitContext::new_root(&ctx, &source_repo)
            .add_file("1.txt", "content")
            .commit()
            .await?;

        bookmark(&ctx, &source_repo, "main")
            .set_to(root_cs_id)
            .await?;
        add_source_repo(&ctx, &source_repo, &hyper_repo, &BookmarkName::new("main")?).await?;

        assert_eq!(
            list_working_copy_utf8(
                &ctx,
                &hyper_repo,
                resolve_cs_id(&ctx, &hyper_repo, "main").await?,
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

        bookmark(&ctx, &second_source_repo, "main")
            .set_to(second_root_cs_id)
            .await?;
        add_source_repo(
            &ctx,
            &second_source_repo,
            &hyper_repo,
            &BookmarkName::new("main")?,
        )
        .await?;
        assert_eq!(
            list_working_copy_utf8(
                &ctx,
                &hyper_repo,
                resolve_cs_id(&ctx, &hyper_repo, "main").await?,
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
        let mut test_repo_factory = TestRepoFactory::new()?;

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

        bookmark(&ctx, &source_repo, "main")
            .set_to(root_cs_id)
            .await?;

        // Create a commit in hyper repo that would clash with files in the source repo
        let root_cs_id = CreateCommitContext::new_root(&ctx, &hyper_repo)
            .add_file("source_repo/1.txt", "content")
            .commit()
            .await?;

        bookmark(&ctx, &hyper_repo, "main")
            .set_to(root_cs_id)
            .await?;

        let res =
            add_source_repo(&ctx, &source_repo, &hyper_repo, &BookmarkName::new("main")?).await;
        assert!(res.is_err());

        Ok(())
    }
}
