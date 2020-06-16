/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use ascii::AsciiString;

use anyhow::{format_err, Error};
use blobstore::Loadable;
use bookmarks::{BookmarkName, BookmarkUpdateReason};
use cloned::cloned;
use context::CoreContext;
use cross_repo_sync::{
    rewrite_commit, update_mapping, upload_commits, CommitSyncRepos, CommitSyncer, Syncers,
};
use futures::{compat::Future01CompatExt, FutureExt, TryFutureExt};
use maplit::hashmap;
use megarepolib::{common::ChangesetArgs, perform_move};
use metaconfig_types::{
    CommitSyncConfig, CommitSyncDirection, DefaultSmallToLargeCommitSyncPathAction,
    SmallRepoCommitSyncConfig,
};
use mononoke_types::RepositoryId;
use mononoke_types::{ChangesetId, DateTime, MPath};
use sql::rusqlite::Connection as SqliteConnection;
use sql_construct::SqlConstruct;
use sql_ext::SqlConnections;
use std::{collections::HashMap, sync::Arc};
use synced_commit_mapping::{
    SqlSyncedCommitMapping, SyncedCommitMapping, SyncedCommitMappingEntry,
};
use tests_utils::{bookmark, CreateCommitContext};

// Helper function that takes a root commit from source repo and rebases it on master bookmark
// in target repo
pub async fn rebase_root_on_master<M>(
    ctx: CoreContext,
    commit_syncer: &CommitSyncer<M>,
    source_bcs_id: ChangesetId,
) -> Result<ChangesetId, Error>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    let bookmark_name = BookmarkName::new("master").unwrap();
    let source_bcs = source_bcs_id
        .load(ctx.clone(), commit_syncer.get_source_repo().blobstore())
        .compat()
        .await
        .unwrap();
    if !source_bcs.parents().collect::<Vec<_>>().is_empty() {
        return Err(format_err!("not a root commit"));
    }

    let maybe_bookmark_val = commit_syncer
        .get_target_repo()
        .get_bonsai_bookmark(ctx.clone(), &bookmark_name)
        .compat()
        .await?;

    let source_repo = commit_syncer.get_source_repo();
    let target_repo = commit_syncer.get_target_repo();

    let bookmark_val = maybe_bookmark_val.ok_or(format_err!("master not found"))?;
    let source_bcs_mut = source_bcs.into_mut();
    let maybe_rewritten = {
        cloned!(ctx);
        async move {
            let map = HashMap::new();
            rewrite_commit(
                ctx,
                source_bcs_mut,
                &map,
                commit_syncer.get_mover().clone(),
                source_repo.clone(),
            )
            .await
        }
    }
    .boxed()
    .compat()
    .compat()
    .await?;
    let mut target_bcs_mut = maybe_rewritten.unwrap();
    target_bcs_mut.parents = vec![bookmark_val];

    let target_bcs = target_bcs_mut.freeze()?;
    {
        cloned!(ctx, target_bcs);
        async move {
            upload_commits(
                ctx,
                vec![target_bcs],
                commit_syncer.get_source_repo().clone(),
                commit_syncer.get_target_repo().clone(),
            )
            .await
        }
    }
    .boxed()
    .compat()
    .compat()
    .await?;

    let mut txn = target_repo.update_bookmark_transaction(ctx.clone());
    txn.force_set(
        &bookmark_name,
        target_bcs.get_changeset_id(),
        BookmarkUpdateReason::TestMove {
            bundle_replay_data: None,
        },
    )
    .unwrap();
    txn.commit().compat().await.unwrap();

    let entry = SyncedCommitMappingEntry::new(
        target_repo.get_repoid(),
        target_bcs.get_changeset_id(),
        source_repo.get_repoid(),
        source_bcs_id,
        Some("TEST_VERSION_NAME".to_string()),
    );
    commit_syncer
        .get_mapping()
        .add(ctx.clone(), entry)
        .compat()
        .await?;

    Ok(target_bcs.get_changeset_id())
}

pub async fn init_small_large_repo(
    ctx: &CoreContext,
) -> Result<(Syncers<SqlSyncedCommitMapping>, CommitSyncConfig), Error> {
    let sqlite_con = SqliteConnection::open_in_memory()?;
    sqlite_con.execute_batch(SqlSyncedCommitMapping::CREATION_QUERY)?;
    let (megarepo, con) = blobrepo_factory::new_memblob_with_sqlite_connection_with_id(
        sqlite_con,
        RepositoryId::new(1),
    )?;

    let mapping =
        SqlSyncedCommitMapping::from_sql_connections(SqlConnections::new_single(con.clone()));
    let (smallrepo, _) =
        blobrepo_factory::new_memblob_with_connection_with_id(con.clone(), RepositoryId::new(0))?;

    let repos = CommitSyncRepos::SmallToLarge {
        small_repo: smallrepo.clone(),
        large_repo: megarepo.clone(),
        mover: Arc::new(prefix_mover),
        reverse_mover: Arc::new(reverse_prefix_mover),
        bookmark_renamer: Arc::new(identity_renamer),
        reverse_bookmark_renamer: Arc::new(identity_renamer),
        version_name: "TEST_VERSION_NAME".to_string(),
    };
    let small_to_large_commit_syncer = CommitSyncer::new(mapping.clone(), repos.clone());

    let repos = CommitSyncRepos::LargeToSmall {
        small_repo: smallrepo.clone(),
        large_repo: megarepo.clone(),
        mover: Arc::new(reverse_prefix_mover),
        reverse_mover: Arc::new(prefix_mover),
        bookmark_renamer: Arc::new(identity_renamer),
        reverse_bookmark_renamer: Arc::new(identity_renamer),
        version_name: "TEST_VERSION_NAME".to_string(),
    };
    let large_to_small_commit_syncer = CommitSyncer::new(mapping.clone(), repos.clone());

    let first_bcs_id = CreateCommitContext::new_root(&ctx, &smallrepo)
        .add_file("file", "content")
        .commit()
        .await?;
    let second_bcs_id = CreateCommitContext::new(&ctx, &smallrepo, vec![first_bcs_id])
        .add_file("file2", "content")
        .commit()
        .await?;

    small_to_large_commit_syncer
        .unsafe_preserve_commit(ctx.clone(), first_bcs_id)
        .await?;
    small_to_large_commit_syncer
        .unsafe_preserve_commit(ctx.clone(), second_bcs_id)
        .await?;
    bookmark(&ctx, &smallrepo, "premove")
        .set_to(second_bcs_id)
        .await?;
    bookmark(&ctx, &megarepo, "premove")
        .set_to(second_bcs_id)
        .await?;

    let move_cs_args = ChangesetArgs {
        author: "Author Authorov".to_string(),
        message: "move commit".to_string(),
        datetime: DateTime::from_rfc3339("2018-11-29T12:00:00.00Z").unwrap(),
        bookmark: None,
        mark_public: false,
    };
    let move_hg_cs = perform_move(
        &ctx,
        &megarepo,
        second_bcs_id,
        Arc::new(prefix_mover),
        move_cs_args,
    )
    .await?;

    let maybe_move_bcs_id = megarepo
        .get_bonsai_from_hg(ctx.clone(), move_hg_cs)
        .compat()
        .await?;
    let move_bcs_id = maybe_move_bcs_id.unwrap();

    // Master commit in the small repo after "big move"
    let small_master_bcs_id = CreateCommitContext::new(&ctx, &smallrepo, vec![second_bcs_id])
        .add_file("file3", "content3")
        .commit()
        .await?;

    // Master commit in large repo after "big move"
    let large_master_bcs_id = CreateCommitContext::new(&ctx, &megarepo, vec![move_bcs_id])
        .add_file("prefix/file3", "content3")
        .commit()
        .await?;

    bookmark(&ctx, &smallrepo, "master")
        .set_to(small_master_bcs_id)
        .await?;
    bookmark(&ctx, &megarepo, "master")
        .set_to(large_master_bcs_id)
        .await?;

    update_mapping(
        ctx.clone(),
        hashmap! { small_master_bcs_id => large_master_bcs_id},
        &small_to_large_commit_syncer,
    )
    .await?;

    println!(
        "small master: {}, large master: {}",
        small_master_bcs_id, large_master_bcs_id
    );
    println!(
        "{:?}",
        small_to_large_commit_syncer
            .get_commit_sync_outcome(ctx.clone(), small_master_bcs_id)
            .await?
    );

    let small_repo_sync_config = SmallRepoCommitSyncConfig {
        default_action: DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(MPath::new(
            "prefix",
        )?),
        map: hashmap! {},
        bookmark_prefix: AsciiString::new(),
        direction: CommitSyncDirection::SmallToLarge,
    };
    let commit_sync_config = CommitSyncConfig {
        large_repo_id: megarepo.get_repoid(),
        common_pushrebase_bookmarks: vec![],
        small_repos: hashmap! {
            smallrepo.get_repoid() => small_repo_sync_config,
        },
        version_name: "TEST_VERSION_NAME".to_string(),
    };

    Ok((
        Syncers {
            small_to_large: small_to_large_commit_syncer,
            large_to_small: large_to_small_commit_syncer,
        },
        commit_sync_config,
    ))
}

fn identity_renamer(b: &BookmarkName) -> Option<BookmarkName> {
    Some(b.clone())
}

fn prefix_mover(v: &MPath) -> Result<Option<MPath>, Error> {
    let prefix = MPath::new("prefix").unwrap();
    Ok(Some(MPath::join(&prefix, v)))
}

fn reverse_prefix_mover(v: &MPath) -> Result<Option<MPath>, Error> {
    let prefix = MPath::new("prefix").unwrap();
    if prefix.is_prefix_of(v) {
        Ok(v.remove_prefix_component(&prefix))
    } else {
        Ok(None)
    }
}
