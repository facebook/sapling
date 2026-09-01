/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::str::FromStr;
use std::time::Duration;

use anyhow::Context;
use anyhow::format_err;
use async_trait::async_trait;
use blobrepo_hg::BlobRepoHg;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bookmarks::BookmarkTransactionError;
use bookmarks::Bookmarks;
use cloned::cloned;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use fixtures::Linear;
use fixtures::ManyFilesDirs;
use fixtures::MergeEven;
use fixtures::TestRepoFixture;
use futures::future::TryFutureExt;
use futures::future::try_join_all;
use futures::stream;
use futures::stream::TryStreamExt;
use justknobs::test_helpers::JustKnobsInMemory;
use justknobs::test_helpers::KnobVal;
use justknobs::test_helpers::override_just_knobs;
use manifest::Entry;
use manifest::ManifestOps;
use maplit::btreemap;
use maplit::hashmap;
use maplit::hashset;
use mononoke_macros::mononoke;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::FileType;
use mononoke_types::GitLfs;
use mononoke_types::PrefixTrie;
use mononoke_types::RepositoryId;
use mutable_counters::MutableCounters;
use mutable_counters::MutableCountersRef;
use mutable_counters::SqlMutableCounters;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use sql_ext::Transaction;
use sql_ext::TransactionResult;
use test_repo_factory::TestRepoFactory;
use tests_utils::CreateCommitContext;
use tests_utils::bookmark;
use tests_utils::drawdag::extend_from_dag_with_actions;
use tests_utils::resolve_cs_id;

use super::*;

fn init_just_knobs_for_test() {
    override_just_knobs(JustKnobsInMemory::new(hashmap! {
        "scm/mononoke:pushrebase_enable_merge_resolution".to_string() => KnobVal::Bool(false),
        "scm/mononoke:pushrebase_merge_resolution_derive_fsnodes".to_string() => KnobVal::Bool(true),
        "scm/mononoke:per_bookmark_locking".to_string() => KnobVal::Bool(false),
        "scm/mononoke:derived_data_use_content_manifests".to_string() => KnobVal::Bool(false),
        "scm/mononoke:pushrebase_range_diff_use_content_manifests".to_string() => KnobVal::Bool(false),
    }));
}

#[facet::container]
#[derive(Clone)]
struct PushrebaseTestRepo {
    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    sql_bookmarks: dbbookmarks::SqlBookmarks,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    repo_derived_data: RepoDerivedData,

    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    filestore_config: FilestoreConfig,

    #[facet]
    mutable_counters: dyn MutableCounters,

    #[facet]
    commit_graph: CommitGraph,

    #[facet]
    commit_graph_writer: dyn CommitGraphWriter,
}

async fn fetch_bonsai_changesets(
    ctx: &CoreContext,
    repo: &(impl Repo + BonsaiHgMappingRef),
    commit_ids: &HashSet<HgChangesetId>,
) -> Result<HashSet<BonsaiChangeset>, PushrebaseError> {
    let futs = commit_ids.iter().map(async |hg_cs_id| {
        let bcs_id = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(ctx, *hg_cs_id)
            .await?
            .ok_or_else(|| {
                Error::from(PushrebaseInternalError::BonsaiNotFoundForHgChangeset(
                    *hg_cs_id,
                ))
            })?;

        let bcs = bcs_id
            .load(ctx, repo.repo_blobstore())
            .await
            .context("While initial bonsai changesets fetching")?;

        Result::<_, Error>::Ok(bcs)
    });

    let ret = try_join_all(futs).await?.into_iter().collect();
    Ok(ret)
}

async fn do_pushrebase(
    ctx: &CoreContext,
    repo: &(impl PushrebaseRepo + BonsaiHgMappingRef),
    config: &PushrebaseFlags,
    onto_bookmark: &BookmarkKey,
    pushed_set: &HashSet<HgChangesetId>,
) -> Result<PushrebaseOutcome, PushrebaseError> {
    init_just_knobs_for_test();
    let pushed = fetch_bonsai_changesets(ctx, repo, pushed_set).await?;

    let res = do_pushrebase_bonsai(ctx, repo, config, onto_bookmark, &pushed, &[]).await?;

    Ok(res)
}

async fn set_bookmark(
    ctx: CoreContext,
    repo: &(impl Repo + BonsaiHgMappingRef),
    book: &BookmarkKey,
    cs_id: &str,
) -> Result<(), Error> {
    let head = HgChangesetId::from_str(cs_id)?;
    let head = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(&ctx, head)
        .await?
        .ok_or_else(|| Error::msg(format_err!("Head not found: {cs_id:?}")))?;

    let mut txn = repo.bookmarks().create_transaction(ctx);
    txn.force_set(book, head, BookmarkUpdateReason::TestMove)?;
    txn.commit().await?;
    Ok(())
}

fn make_paths(paths: &[&str]) -> Vec<MPath> {
    let paths: Result<_, _> = paths.iter().map(MPath::new).collect();
    paths.unwrap()
}

fn master_bookmark() -> BookmarkKey {
    BookmarkKey::new("master").unwrap()
}

async fn push_and_verify(
    ctx: &CoreContext,
    repo: &(impl PushrebaseRepo + BonsaiHgMappingRef),
    parent: ChangesetId,
    bookmark: &BookmarkKey,
    content: BTreeMap<&str, Option<&str>>,
    should_succeed: bool,
) -> Result<(), Error> {
    let mut commit_ctx = CreateCommitContext::new(ctx, repo, vec![parent]);

    for (path, maybe_content) in content.iter() {
        let path: &str = path;
        commit_ctx = match maybe_content {
            Some(content) => commit_ctx.add_file(path, *content),
            None => commit_ctx.delete_file(path),
        };
    }

    let cs_id = commit_ctx.commit().await?;

    let hgcss = hashset![repo.derive_hg_changeset(ctx, cs_id).await?];

    let res = do_pushrebase(ctx, repo, &PushrebaseFlags::default(), bookmark, &hgcss).await;

    if should_succeed {
        assert!(res.is_ok());
    } else {
        should_have_conflicts(res);
    }

    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_one_commit(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
    // Bottom commit of the repo
    let parents = vec!["2d7d4ba9ce0a6ffd222de7785b249ead9c51c536"];
    let bcs_id = CreateCommitContext::new(&ctx, &repo, parents)
        .add_file("file", "content")
        .commit()
        .await?;

    let hg_cs = repo.derive_hg_changeset(&ctx, bcs_id).await?;

    let book = master_bookmark();
    bookmark(&ctx, &repo, book.clone())
        .set_to("a5ffa77602a066db7d5cfb9fb5823a0895717c5a")
        .await?;

    do_pushrebase(&ctx, &repo, &Default::default(), &book, &hashset![hg_cs])
        .map_err(|err| format_err!("{err:?}"))
        .await?;
    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_one_commit_transaction_hook(fb: FacebookInit) -> Result<(), Error> {
    #[derive(Copy, Clone)]
    struct Hook(RepositoryId);

    #[async_trait]
    impl PushrebaseHook for Hook {
        async fn in_critical_section(
            &self,
            _ctx: &CoreContext,
            _old_bookmark_value: Option<ChangesetId>,
        ) -> Result<Box<dyn PushrebaseCommitHook>, Error> {
            Ok(Box::new(*self) as Box<dyn PushrebaseCommitHook>)
        }
    }

    #[async_trait]
    impl PushrebaseCommitHook for Hook {
        fn post_rebase_changeset(
            &mut self,
            _bcs_old: ChangesetId,
            _bcs_new: &mut BonsaiChangesetMut,
        ) -> Result<(), Error> {
            Ok(())
        }

        async fn into_transaction_hook(
            self: Box<Self>,
            _ctx: &CoreContext,
            changesets: &RebasedChangesets,
        ) -> Result<Box<dyn PushrebaseTransactionHook>, Error> {
            let (_, (cs_id, _)) = changesets
                .iter()
                .next()
                .ok_or_else(|| Error::msg("No rebased changeset"))?;
            Ok(Box::new(TransactionHook(self.0, *cs_id)) as Box<dyn PushrebaseTransactionHook>)
        }
    }

    struct TransactionHook(RepositoryId, ChangesetId);

    #[async_trait]
    impl PushrebaseTransactionHook for TransactionHook {
        async fn populate_transaction(
            &self,
            ctx: &CoreContext,
            txn: Transaction,
        ) -> Result<Transaction, BookmarkTransactionError> {
            let key = format!("{}", self.1);

            let ret =
                SqlMutableCounters::set_counter_on_txn(ctx, self.0, &key, 1, None, txn).await?;

            match ret {
                TransactionResult::Succeeded(txn) => Ok(txn),
                TransactionResult::Failed => Err(Error::msg("Did not update").into()),
            }
        }
    }

    let ctx = CoreContext::test_mock(fb);
    let factory = TestRepoFactory::new(fb)?;
    let repo: PushrebaseTestRepo = factory.build().await?;
    Linear::init_repo(fb, &repo).await?;
    // Bottom commit of the repo
    let parents = vec!["2d7d4ba9ce0a6ffd222de7785b249ead9c51c536"];
    let bcs_id = CreateCommitContext::new(&ctx, &repo, parents)
        .add_file("file", "content")
        .commit()
        .await?;

    let bcs = bcs_id.load(&ctx, repo.repo_blobstore()).await?;

    let mut book = master_bookmark();

    bookmark(&ctx, &repo, book.clone())
        .set_to("a5ffa77602a066db7d5cfb9fb5823a0895717c5a")
        .await?;

    let hook: Box<dyn PushrebaseHook> = Box::new(Hook(repo.repo_identity().id()));
    let hooks = [hook];

    do_pushrebase_bonsai(
        &ctx,
        &repo,
        &Default::default(),
        &book,
        &hashset![bcs.clone()],
        &hooks,
    )
    .map_err(|err| format_err!("{err:?}"))
    .await?;

    let master_val = resolve_cs_id(&ctx, &repo, "master").await?;
    let key = format!("{master_val}");
    assert_eq!(
        repo.mutable_counters().get_counter(&ctx, &key).await?,
        Some(1),
    );

    // Now do the same with another non-existent bookmark,
    // make sure cs id is created.
    book = BookmarkKey::new("newbook")?;
    do_pushrebase_bonsai(
        &ctx,
        &repo,
        &Default::default(),
        &book,
        &hashset![bcs],
        &hooks,
    )
    .map_err(|err| format_err!("{err:?}"))
    .await?;

    let key = format!("{}", resolve_cs_id(&ctx, &repo, "newbook").await?);
    assert_eq!(
        repo.mutable_counters().get_counter(&ctx, &key).await?,
        Some(1),
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_stack(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
    // Bottom commit of the repo
    let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
    let p = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(&ctx, root)
        .await?
        .ok_or_else(|| Error::msg("Root is missing"))?;
    let bcs_id_1 = CreateCommitContext::new(&ctx, &repo, vec![p])
        .add_file("file", "content")
        .commit()
        .await?;
    let bcs_id_2 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_1])
        .add_file("file2", "content")
        .commit()
        .await?;

    assert_eq!(
        find_changed_files(&ctx, &repo, p, bcs_id_2).await?,
        make_paths(&["file", "file2"]),
    );

    let book = master_bookmark();
    set_bookmark(
        ctx.clone(),
        &repo,
        &book,
        "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
    )
    .await?;

    let hg_cs_1 = repo.derive_hg_changeset(&ctx, bcs_id_1).await?;
    let hg_cs_2 = repo.derive_hg_changeset(&ctx, bcs_id_2).await?;
    do_pushrebase(
        &ctx,
        &repo,
        &Default::default(),
        &book,
        &hashset![hg_cs_1, hg_cs_2],
    )
    .await?;
    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_stack_with_renames(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
    // Bottom commit of the repo
    let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
    let p = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(&ctx, root)
        .await?
        .ok_or_else(|| Error::msg("p is missing"))?;
    let bcs_id_1 = CreateCommitContext::new(&ctx, &repo, vec![p])
        .add_file("file", "content")
        .commit()
        .await?;
    let bcs_id_2 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_1])
        .add_file_with_copy_info("file_renamed", "content", (bcs_id_1, "file"))
        .commit()
        .await?;

    assert_eq!(
        find_changed_files(&ctx, &repo, p, bcs_id_2).await?,
        make_paths(&["file", "file_renamed"]),
    );

    let book = master_bookmark();
    set_bookmark(
        ctx.clone(),
        &repo,
        &book,
        "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
    )
    .await?;

    let hg_cs_1 = repo.derive_hg_changeset(&ctx, bcs_id_1).await?;
    let hg_cs_2 = repo.derive_hg_changeset(&ctx, bcs_id_2).await?;
    do_pushrebase(
        &ctx,
        &repo,
        &Default::default(),
        &book,
        &hashset![hg_cs_1, hg_cs_2],
    )
    .await?;

    Ok(())
}

fn stack_rebase_flags() -> PushrebaseFlags {
    PushrebaseFlags {
        rewritedates: false,
        merge_resolution_override: MergeResolutionOverride::ForceOff,
        ..Default::default()
    }
}

#[mononoke::fbinit_test]
async fn range_diff_manifest_kind_is_knob_routed_and_equivalent(
    fb: FacebookInit,
) -> Result<(), Error> {
    init_just_knobs_for_test();
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = TestRepoFactory::new(fb)?.build().await?;

    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("base", "base")
        .commit()
        .await?;
    let side = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("side", "side")
        .commit()
        .await?;
    let s1 = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("server_file", "server")
        .commit()
        .await?;
    let merge = CreateCommitContext::new(&ctx, &repo, vec![s1, side])
        .add_file("merge_file", "merge")
        .commit()
        .await?;

    // Knob off: the standard entry uses HG manifests.
    let hg = find_changed_files(&ctx, &repo, root, merge).await?;
    let content =
        find_changed_files_with(&ctx, &repo, root, merge, RangeDiffManifests::ContentCompat)
            .await?;
    assert_eq!(
        hg, content,
        "HG and content manifests must report the same changed files"
    );
    assert!(
        !hg.is_empty(),
        "the merge-range diff must see the server and side changes"
    );

    // The override is process-global: carry the full standard map.
    override_just_knobs(JustKnobsInMemory::new(hashmap! {
        "scm/mononoke:pushrebase_enable_merge_resolution".to_string() => KnobVal::Bool(false),
        "scm/mononoke:pushrebase_merge_resolution_derive_fsnodes".to_string() => KnobVal::Bool(true),
        "scm/mononoke:per_bookmark_locking".to_string() => KnobVal::Bool(false),
        "scm/mononoke:derived_data_use_content_manifests".to_string() => KnobVal::Bool(false),
        "scm/mononoke:pushrebase_range_diff_use_content_manifests".to_string() => KnobVal::Bool(true),
    }));
    let via_enabled_knob = find_changed_files(&ctx, &repo, root, merge).await?;
    assert_eq!(
        via_enabled_knob, content,
        "knob on routes to content manifests"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn rebase_stack_onto_moves_stack_and_is_deterministic(fb: FacebookInit) -> Result<(), Error> {
    init_just_knobs_for_test();
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = TestRepoFactory::new(fb)?.build().await?;

    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("base", "base")
        .commit()
        .await?;
    let stack_a = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("file_a", "a")
        .commit()
        .await?;
    let stack_b = CreateCommitContext::new(&ctx, &repo, vec![stack_a])
        .add_file("file_b", "b")
        .commit()
        .await?;
    let onto = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("server_file", "server")
        .commit()
        .await?;

    let rebased =
        rebase_stack_onto(&ctx, &repo, &stack_rebase_flags(), root, stack_b, onto).await?;

    assert_eq!(
        rebased.rebased_changesets.len(),
        2,
        "both stack commits should be rebased"
    );
    changesets_creation::save_changesets(&ctx, &repo, rebased.rebased_bonsais.clone()).await?;

    let new_a = rebased
        .rebased_changesets
        .iter()
        .find(|pair| pair.id_old == stack_a)
        .expect("should have a mapping for the bottom commit")
        .id_new;
    let new_a_bcs = new_a.load(&ctx, repo.repo_blobstore()).await?;
    assert_eq!(
        new_a_bcs.parents().collect::<Vec<_>>(),
        vec![onto],
        "rebased bottom commit should sit on onto"
    );
    let new_head = rebased.new_head.load(&ctx, repo.repo_blobstore()).await?;
    assert_eq!(
        new_head.parents().collect::<Vec<_>>(),
        vec![new_a],
        "rebased head should sit on the rebased bottom commit"
    );
    assert!(
        new_head
            .file_changes_map()
            .contains_key(&NonRootMPath::new("file_b")?),
        "rebased head keeps its file change"
    );

    let again = rebase_stack_onto(&ctx, &repo, &stack_rebase_flags(), root, stack_b, onto).await?;
    assert_eq!(
        again.new_head, rebased.new_head,
        "rebasing the same stack onto the same head twice should produce identical ids"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn rebase_stack_onto_conflict(fb: FacebookInit) -> Result<(), Error> {
    init_just_knobs_for_test();
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = TestRepoFactory::new(fb)?.build().await?;

    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("conflict_file", "base")
        .commit()
        .await?;
    let head = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("conflict_file", "client")
        .commit()
        .await?;
    let onto = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("conflict_file", "server")
        .commit()
        .await?;

    let res = rebase_stack_onto(&ctx, &repo, &stack_rebase_flags(), root, head, onto).await;
    let conflict_path = MPath::new("conflict_file")?;
    match res {
        Err(PushrebaseError::Conflicts(conflicts)) => {
            assert!(
                conflicts.iter().any(|c| c.left == conflict_path),
                "conflict should name the overlapping path, got {conflicts:?}"
            );
        }
        other => panic!("expected a conflict, got {:?}", other.map(|r| r.new_head)),
    }

    Ok(())
}

#[mononoke::fbinit_test]
async fn rebase_stack_onto_rejects_merge_in_stack(fb: FacebookInit) -> Result<(), Error> {
    init_just_knobs_for_test();
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = TestRepoFactory::new(fb)?.build().await?;

    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("base", "base")
        .commit()
        .await?;
    let d1 = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("d1", "d1")
        .commit()
        .await?;
    let d2 = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("d2", "d2")
        .commit()
        .await?;
    let merge = CreateCommitContext::new(&ctx, &repo, vec![d1, d2])
        .add_file("m", "m")
        .commit()
        .await?;
    let onto = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("server_file", "server")
        .commit()
        .await?;

    let err = rebase_stack_onto(&ctx, &repo, &stack_rebase_flags(), root, merge, onto)
        .await
        .expect_err("a stack containing a merge commit should be rejected");
    assert!(
        err.to_string().contains("merge commits"),
        "unexpected error: {err}"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn rebase_stack_onto_rejects_non_ancestor_root(fb: FacebookInit) -> Result<(), Error> {
    init_just_knobs_for_test();
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = TestRepoFactory::new(fb)?.build().await?;

    let base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("base", "base")
        .commit()
        .await?;
    let root = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file_a", "a")
        .commit()
        .await?;
    let head = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("file_b", "b")
        .commit()
        .await?;
    // A force-moved branch: onto is NOT a descendant of root.
    let onto = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("server_file", "server")
        .commit()
        .await?;

    let err = rebase_stack_onto(&ctx, &repo, &stack_rebase_flags(), root, head, onto)
        .await
        .expect_err("a root that is not an ancestor of onto should be rejected");
    assert!(
        err.to_string().contains("must be an ancestor"),
        "unexpected error: {err}"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn rebase_stack_onto_remaps_copy_info(fb: FacebookInit) -> Result<(), Error> {
    init_just_knobs_for_test();
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = TestRepoFactory::new(fb)?.build().await?;

    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("base", "base")
        .commit()
        .await?;
    let stack_a = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("file", "content")
        .commit()
        .await?;
    let stack_b = CreateCommitContext::new(&ctx, &repo, vec![stack_a])
        .add_file_with_copy_info("file_renamed", "content", (stack_a, "file"))
        .commit()
        .await?;
    let onto = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("server_file", "server")
        .commit()
        .await?;

    let rebased =
        rebase_stack_onto(&ctx, &repo, &stack_rebase_flags(), root, stack_b, onto).await?;
    changesets_creation::save_changesets(&ctx, &repo, rebased.rebased_bonsais.clone()).await?;

    let new_a = rebased
        .rebased_changesets
        .iter()
        .find(|pair| pair.id_old == stack_a)
        .expect("should have a mapping for the bottom commit")
        .id_new;
    let new_head = rebased.new_head.load(&ctx, repo.repo_blobstore()).await?;
    let fc = new_head
        .file_changes_map()
        .get(&NonRootMPath::new("file_renamed")?)
        .expect("rebased head should keep the renamed file");
    match fc {
        FileChange::Change(tc) => {
            assert_eq!(
                tc.copy_from().map(|(path, cs)| (path.clone(), *cs)),
                Some((NonRootMPath::new("file")?, new_a)),
                "copy_from should point at the REBASED source commit"
            );
        }
        other => panic!("expected a tracked change, got {other:?}"),
    }

    Ok(())
}

#[mononoke::fbinit_test]
async fn rebase_stack_onto_server_merge_range_without_hg(fb: FacebookInit) -> Result<(), Error> {
    init_just_knobs_for_test();
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = TestRepoFactory::new(fb)?.build().await?;

    // The server range root..onto contains a merge commit whose other
    // parent lies outside the range: the range diff for it must come
    // from the derived content manifests, not HG manifests.
    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("base", "base")
        .commit()
        .await?;
    let side_root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("side", "side")
        .commit()
        .await?;
    let s1 = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("server_file", "server")
        .commit()
        .await?;
    let onto = CreateCommitContext::new(&ctx, &repo, vec![s1, side_root])
        .add_file("merge_file", "merge")
        .commit()
        .await?;
    let head = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("file_a", "a")
        .commit()
        .await?;

    let rebased = rebase_stack_onto(&ctx, &repo, &stack_rebase_flags(), root, head, onto).await?;
    changesets_creation::save_changesets(&ctx, &repo, rebased.rebased_bonsais.clone()).await?;
    let new_head = rebased.new_head.load(&ctx, repo.repo_blobstore()).await?;
    assert_eq!(
        new_head.parents().collect::<Vec<_>>(),
        vec![onto],
        "stack should land on top of the server merge"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn rebase_stack_onto_merge_resolution_governs_same_file_edits(
    fb: FacebookInit,
) -> Result<(), Error> {
    init_just_knobs_for_test();
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = TestRepoFactory::new(fb)?.build().await?;

    const BASE: &str = "alpha\nbravo\ncharlie\ndelta\necho\nfoxtrot\n";
    const SERVER: &str = "alpha SERVER\nbravo\ncharlie\ndelta\necho\nfoxtrot\n";
    const CLIENT: &str = "alpha\nbravo\ncharlie\ndelta\necho\nfoxtrot CLIENT\n";

    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("shared.txt", BASE)
        .commit()
        .await?;
    let onto = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("shared.txt", SERVER)
        .commit()
        .await?;
    let head = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("shared.txt", CLIENT)
        .commit()
        .await?;

    let merging_flags = PushrebaseFlags {
        merge_resolution_override: MergeResolutionOverride::ForceOn,
        ..stack_rebase_flags()
    };
    let rebased = rebase_stack_onto(&ctx, &repo, &merging_flags, root, head, onto).await?;

    changesets_creation::save_changesets(&ctx, &repo, rebased.rebased_bonsais.clone()).await?;
    let result_cs = repo
        .derive_hg_changeset(&ctx, rebased.new_head)
        .await?
        .load(&ctx, repo.repo_blobstore())
        .await?;
    let file_entry = result_cs
        .manifestid()
        .find_entry(
            ctx.clone(),
            repo.repo_blobstore().clone(),
            NonRootMPath::new("shared.txt")?.into(),
        )
        .await?
        .expect("shared.txt should exist");
    let shared = match file_entry {
        Entry::Leaf((_, filenode_id)) => {
            let content_id = filenode_id
                .load(&ctx, repo.repo_blobstore())
                .await?
                .content_id();
            let bytes = filestore::fetch_concat(repo.repo_blobstore(), &ctx, content_id).await?;
            String::from_utf8(bytes.to_vec())?
        }
        _ => panic!("shared.txt should be a file"),
    };
    assert!(
        shared.contains("alpha SERVER") && shared.contains("foxtrot CLIENT"),
        "both disjoint edits should survive the merge, got: {shared:?}"
    );

    let err = rebase_stack_onto(&ctx, &repo, &stack_rebase_flags(), root, head, onto)
        .await
        .expect_err("ForceOff keeps path-level rejection");
    assert!(
        matches!(err, PushrebaseError::Conflicts(_)),
        "unexpected error: {err}"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn rebase_stack_onto_requires_rewritedates_off(fb: FacebookInit) -> Result<(), Error> {
    init_just_knobs_for_test();
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = TestRepoFactory::new(fb)?.build().await?;

    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("base", "base")
        .commit()
        .await?;
    let head = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("file_a", "a")
        .commit()
        .await?;

    let date_rewriting_flags = PushrebaseFlags {
        rewritedates: true,
        merge_resolution_override: MergeResolutionOverride::ForceOff,
        ..Default::default()
    };
    let err = rebase_stack_onto(&ctx, &repo, &date_rewriting_flags, root, head, root)
        .await
        .expect_err("date rewriting should be rejected");
    assert!(
        err.to_string().contains("rewritedates"),
        "unexpected error: {err}"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_multi_root(fb: FacebookInit) -> Result<(), Error> {
    // This test calls find_changed_files directly on a merge range,
    // which resolves the range-diff knob before do_pushrebase installs
    // the map.
    init_just_knobs_for_test();
    //
    // master -> o
    //           |
    //           :  o <- bcs3
    //           :  |
    //           :  o <- bcs2
    //           : /|
    //           |/ |
    //  root1 -> o  |
    //           |  o <- bcs1 (outside of rebase set)
    //           o /
    //           |/
    //  root0 -> o
    //
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
    let config = PushrebaseFlags::default();

    let root0 = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(
            &ctx,
            HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
        )
        .await?
        .ok_or_else(|| Error::msg("root0 is missing"))?;

    let root1 = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(
            &ctx,
            HgChangesetId::from_str("607314ef579bd2407752361ba1b0c1729d08b281")?,
        )
        .await?
        .ok_or_else(|| Error::msg("root0 is missing"))?;

    let bcs_id_1 = CreateCommitContext::new(&ctx, &repo, vec![root0])
        .add_file("f0", "f0")
        .delete_file("files")
        .commit()
        .await?;
    let bcs_id_2 = CreateCommitContext::new(&ctx, &repo, vec![root1, bcs_id_1])
        .add_file("f1", "f1")
        .commit()
        .await?;
    let bcs_id_3 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_2])
        .add_file("f2", "f2")
        .commit()
        .await?;

    let book = master_bookmark();
    set_bookmark(
        ctx.clone(),
        &repo,
        &book,
        "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
    )
    .await?;
    let bcs_id_master = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(
            &ctx,
            HgChangesetId::from_str("a5ffa77602a066db7d5cfb9fb5823a0895717c5a")?,
        )
        .await?
        .ok_or_else(|| Error::msg("bcs_id_master is missing"))?;

    let root = root1;
    assert_eq!(
        find_closest_root(
            &ctx,
            &repo,
            &config,
            &book,
            &hashmap! {root0 => ChildIndex(0), root1 => ChildIndex(0) },
        )
        .await?,
        root,
    );

    assert_eq!(
        find_changed_files(&ctx, &repo, root, bcs_id_3).await?,
        make_paths(&["f0", "f1", "f2"]),
    );

    let hg_cs_1 = repo.derive_hg_changeset(&ctx, bcs_id_1).await?;
    let hg_cs_2 = repo.derive_hg_changeset(&ctx, bcs_id_2).await?;
    let hg_cs_3 = repo.derive_hg_changeset(&ctx, bcs_id_3).await?;
    let bcs_id_rebased = do_pushrebase(
        &ctx,
        &repo,
        &config,
        &book,
        &hashset![hg_cs_1, hg_cs_2, hg_cs_3],
    )
    .await?;

    // should only rebase {bcs2, bcs3}
    let rebased = find_rebased_set(&ctx, &repo, bcs_id_master, bcs_id_rebased.head).await?;
    assert_eq!(rebased.len(), 2);
    let bcs2 = &rebased[0];
    let bcs3 = &rebased[1];

    // bcs3 parent correctly updated and contains only {bcs2}
    assert_eq!(
        bcs3.parents().collect::<Vec<_>>(),
        vec![bcs2.get_changeset_id()]
    );

    // bcs2 parents contains old bcs1 and old master bookmark
    assert_eq!(
        bcs2.parents().collect::<HashSet<_>>(),
        hashset! { bcs_id_1, bcs_id_master },
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_conflict(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
    let root = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(
            &ctx,
            HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
        )
        .await?
        .ok_or_else(|| Error::msg("Root is missing"))?;

    let bcs_id_1 = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("f0", "f0")
        .commit()
        .await?;
    let bcs_id_2 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_1])
        .add_file("9/file", "file")
        .commit()
        .await?;
    let bcs_id_3 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_2])
        .add_file("f1", "f1")
        .commit()
        .await?;

    let book = master_bookmark();
    set_bookmark(
        ctx.clone(),
        &repo,
        &book,
        "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
    )
    .await?;

    let hg_cs_1 = repo.derive_hg_changeset(&ctx, bcs_id_1).await?;
    let hg_cs_2 = repo.derive_hg_changeset(&ctx, bcs_id_2).await?;
    let hg_cs_3 = repo.derive_hg_changeset(&ctx, bcs_id_3).await?;
    let result = do_pushrebase(
        &ctx,
        &repo,
        &Default::default(),
        &book,
        &hashset![hg_cs_1, hg_cs_2, hg_cs_3],
    )
    .await;
    match result {
        Err(PushrebaseError::Conflicts(conflicts)) => {
            assert_eq!(
                conflicts,
                vec![PushrebaseConflict {
                    left: MPath::new("9")?,
                    right: MPath::new("9/file")?,
                },],
            );
        }
        _ => panic!("push-rebase should have failed with conflict"),
    }
    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_caseconflicting_rename(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
    let root = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(
            &ctx,
            HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
        )
        .await?
        .ok_or_else(|| Error::msg("Root is missing"))?;

    let bcs_id_1 = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("FILE", "file")
        .commit()
        .await?;
    let bcs_id_2 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_1])
        .delete_file("FILE")
        .commit()
        .await?;
    let bcs_id_3 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_2])
        .add_file("file", "file")
        .commit()
        .await?;

    let hgcss = hashset![
        repo.derive_hg_changeset(&ctx, bcs_id_1).await?,
        repo.derive_hg_changeset(&ctx, bcs_id_2).await?,
        repo.derive_hg_changeset(&ctx, bcs_id_3).await?,
    ];

    let book = master_bookmark();
    set_bookmark(
        ctx.clone(),
        &repo,
        &book,
        "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
    )
    .await?;

    do_pushrebase(&ctx, &repo, &Default::default(), &book, &hgcss).await?;

    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_caseconflicting_dirs(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
    let root = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(
            &ctx,
            HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
        )
        .await?
        .ok_or_else(|| Error::msg("Root is missing"))?;

    let bcs_id_1 = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("DIR/a", "a")
        .add_file("DIR/b", "b")
        .commit()
        .await?;
    let bcs_id_2 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_1])
        .add_file("dir/a", "a")
        .delete_file("DIR/a")
        .delete_file("DIR/b")
        .commit()
        .await?;
    let hgcss = hashset![
        repo.derive_hg_changeset(&ctx, bcs_id_1).await?,
        repo.derive_hg_changeset(&ctx, bcs_id_2).await?,
    ];

    let book = master_bookmark();
    set_bookmark(
        ctx.clone(),
        &repo,
        &book,
        "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
    )
    .await?;

    do_pushrebase(&ctx, &repo, &Default::default(), &book, &hgcss).await?;

    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_recursion_limit(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
    let root = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(
            &ctx,
            HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
        )
        .await?
        .ok_or_else(|| Error::msg("Root is missing"))?;

    // create a lot of commits
    let (_, bcss) = stream::iter((0..128usize).map(Ok))
        .try_fold((root, vec![]), async |(head, mut bcss), index| {
            let file = format!("f{index}");
            let content = format!("{index}");
            let bcs = CreateCommitContext::new(&ctx, &repo, vec![head])
                .add_file(file.as_str(), content)
                .commit()
                .await?;
            bcss.push(bcs);
            Result::<_, Error>::Ok((bcs, bcss))
        })
        .await?;

    let hgcss = try_join_all(bcss.iter().map(|bcs| repo.derive_hg_changeset(&ctx, *bcs))).await?;
    let book = master_bookmark();
    set_bookmark(
        ctx.clone(),
        &repo,
        &book,
        "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
    )
    .await?;
    do_pushrebase(
        &ctx,
        &repo,
        &Default::default(),
        &book.clone(),
        &hgcss.into_iter().collect(),
    )
    .await?;

    let bcs = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("file", "data")
        .commit()
        .await?;

    let hgcss = hashset![repo.derive_hg_changeset(&ctx, bcs).await?];

    // try rebase with small recursion limit
    let config = PushrebaseFlags {
        recursion_limit: Some(128),
        ..Default::default()
    };
    let result = do_pushrebase(&ctx, &repo, &config, &book, &hgcss).await;
    match result {
        Err(PushrebaseError::RootTooFarBehind) => {}
        _ => panic!("push-rebase should have failed because root too far behind"),
    }

    let config = PushrebaseFlags {
        recursion_limit: Some(256),
        ..Default::default()
    };
    do_pushrebase(&ctx, &repo, &config, &book, &hgcss).await?;

    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_rewritedates(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;
    let (commits, _dag) = extend_from_dag_with_actions(
        &ctx,
        &repo,
        r#"
            A-B-C
               \
                D
            # author_date: D "2020-01-01 01:00:00+04:00"
            # committer: D "Committer <committer@example.test>"
            # committer_date: D "2020-01-01 09:00:00-02:00"
            # bookmark: C keep
            # bookmark: C rewrite
        "#,
    )
    .await?;

    let config = PushrebaseFlags {
        rewritedates: false,
        ..Default::default()
    };
    let source = hashset![commits["D"].load(&ctx, repo.repo_blobstore()).await?];
    let bcs_keep_date = do_pushrebase_bonsai(
        &ctx,
        &repo,
        &config,
        &BookmarkKey::new("keep")?,
        &source,
        &[],
    )
    .await?;

    let config = PushrebaseFlags {
        rewritedates: true,
        ..Default::default()
    };
    let bcs_rewrite_date = do_pushrebase_bonsai(
        &ctx,
        &repo,
        &config,
        &BookmarkKey::new("rewrite")?,
        &source,
        &[],
    )
    .await?;

    let bcs = commits["D"].load(&ctx, repo.repo_blobstore()).await?;
    let bcs_keep_date = bcs_keep_date.head.load(&ctx, repo.repo_blobstore()).await?;
    let bcs_rewrite_date = bcs_rewrite_date
        .head
        .load(&ctx, repo.repo_blobstore())
        .await?;

    // For the keep variant, the time should not have changed.
    assert_eq!(bcs.author_date(), bcs_keep_date.author_date());
    assert_eq!(bcs.committer_date(), bcs_keep_date.committer_date());

    // For the rewrite variant, the time should be updated.
    assert!(bcs.author_date() < bcs_rewrite_date.author_date());
    assert!(bcs.committer_date() < bcs_rewrite_date.committer_date());

    // Timezone shouldn't have changed for either author or committer.
    assert_eq!(
        bcs.author_date().tz_offset_secs(),
        bcs_rewrite_date.author_date().tz_offset_secs()
    );
    assert_eq!(
        bcs.committer_date().unwrap().tz_offset_secs(),
        bcs_rewrite_date.committer_date().unwrap().tz_offset_secs()
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_case_conflict(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = ManyFilesDirs::get_repo(fb).await;
    let root = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(
            &ctx,
            HgChangesetId::from_str("5a28e25f924a5d209b82ce0713d8d83e68982bc8")?,
        )
        .await?
        .ok_or_else(|| Error::msg("Root is missing"))?;

    let bcs = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("Dir1/file_1_in_dir1", "data")
        .commit()
        .await?;

    let hgcss = hashset![repo.derive_hg_changeset(&ctx, bcs).await?];

    let book = master_bookmark();
    set_bookmark(
        ctx.clone(),
        &repo,
        &book,
        "2f866e7e549760934e31bf0420a873f65100ad63",
    )
    .await?;

    let result = do_pushrebase(&ctx, &repo, &Default::default(), &book, &hgcss).await;
    match result {
        Err(PushrebaseError::PotentialCaseConflict(conflict)) => {
            assert_eq!(conflict, NonRootMPath::new("Dir1/file_1_in_dir1")?)
        }
        _ => panic!("push-rebase should have failed with case conflict"),
    };

    // make sure that it is succeeds with disabled casefolding
    do_pushrebase(
        &ctx,
        &repo,
        &PushrebaseFlags {
            casefolding_check: false,
            ..Default::default()
        },
        &book,
        &hgcss,
    )
    .await?;

    Ok(())
}
#[mononoke::fbinit_test]

async fn pushrebase_case_conflict_exclusion(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = ManyFilesDirs::get_repo(fb).await;
    let root = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(
            &ctx,
            HgChangesetId::from_str("5a28e25f924a5d209b82ce0713d8d83e68982bc8")?,
        )
        .await?
        .ok_or_else(|| Error::msg("Root is missing"))?;

    let bcs1 = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("dir1/File_1_in_dir1", "data")
        .commit()
        .await?;

    let hgcs1 = hashset![repo.derive_hg_changeset(&ctx, bcs1).await?];

    let bcs2 = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("dir2/File_1_in_dir2", "data")
        .commit()
        .await?;

    let hgcs2 = hashset![repo.derive_hg_changeset(&ctx, bcs2).await?];

    let book = master_bookmark();
    set_bookmark(
        ctx.clone(),
        &repo,
        &book,
        "2f866e7e549760934e31bf0420a873f65100ad63",
    )
    .await?;

    let result = do_pushrebase(&ctx, &repo, &Default::default(), &book, &hgcs1).await;
    match result {
        Err(PushrebaseError::PotentialCaseConflict(conflict)) => {
            assert_eq!(conflict, NonRootMPath::new("dir1/File_1_in_dir1")?)
        }
        _ => panic!("push-rebase should have failed with case conflict"),
    };

    // make sure that it is succeeds with exclusion
    do_pushrebase(
        &ctx,
        &repo,
        &PushrebaseFlags {
            casefolding_check: true,
            casefolding_check_excluded_paths: PrefixTrie::from_iter(
                vec![Some(NonRootMPath::new("dir1")?)].into_iter(),
            ),
            ..Default::default()
        },
        &book,
        &hgcs1,
    )
    .await?;

    // revert bookmark back
    set_bookmark(
        ctx.clone(),
        &repo,
        &book,
        "2f866e7e549760934e31bf0420a873f65100ad63",
    )
    .await?;
    // make sure that exclusion doesn't exclude too much
    let result = do_pushrebase(
        &ctx,
        &repo,
        &PushrebaseFlags {
            casefolding_check: true,
            casefolding_check_excluded_paths: PrefixTrie::from_iter(
                vec![Some(NonRootMPath::new("dir1")?)].into_iter(),
            ),
            ..Default::default()
        },
        &book,
        &hgcs2,
    )
    .await;
    match result {
        Err(PushrebaseError::PotentialCaseConflict(conflict)) => {
            assert_eq!(conflict, NonRootMPath::new("dir2/File_1_in_dir2")?)
        }
        _ => panic!("push-rebase should have failed with case conflict"),
    };
    Ok(())
}

#[mononoke::test]
fn pushrebase_intersect_changed() -> Result<(), Error> {
    match intersect_changed_files(
        make_paths(&["a/b/c", "c", "a/b/d", "d/d", "b", "e/c"]),
        make_paths(&["d/f", "a/b/d/f", "c", "e"]),
    ) {
        Err(PushrebaseError::Conflicts(conflicts)) => assert_eq!(
            *conflicts,
            [
                PushrebaseConflict {
                    left: MPath::new("a/b/d")?,
                    right: MPath::new("a/b/d/f")?,
                },
                PushrebaseConflict {
                    left: MPath::new("c")?,
                    right: MPath::new("c")?,
                },
                PushrebaseConflict {
                    left: MPath::new("e/c")?,
                    right: MPath::new("e")?,
                },
            ]
        ),
        _ => panic!("should contain conflict"),
    };

    Ok(())
}

#[mononoke::test]
fn pushrebase_intersect_changed_with_reponame() -> Result<(), Error> {
    // Verifies intersect_changed_files detects exact path conflicts
    match intersect_changed_files(make_paths(&["a/b/c"]), make_paths(&["a/b/c"])) {
        Err(PushrebaseError::Conflicts(conflicts)) => {
            assert_eq!(conflicts.len(), 1);
            assert_eq!(
                conflicts[0],
                PushrebaseConflict {
                    left: MPath::new("a/b/c")?,
                    right: MPath::new("a/b/c")?,
                }
            );
            Ok(())
        }
        _ => Err(Error::msg("expected conflict")),
    }
}

#[mononoke::fbinit_test]
async fn pushrebase_executable_bit_change(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
    let path_1 = NonRootMPath::new("1")?;

    let root_hg = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
    let root_cs = root_hg.load(&ctx, repo.repo_blobstore()).await?;

    let root_1_id = root_cs
        .manifestid()
        .find_entry(
            ctx.clone(),
            repo.repo_blobstore().clone(),
            path_1.clone().into(),
        )
        .await?
        .and_then(|entry| Some(entry.into_leaf()?.1))
        .ok_or_else(|| Error::msg("path_1 missing in manifest"))?;

    // crate filechange with with same content as "1" but set executable bit
    let root = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(&ctx, root_hg)
        .await?
        .ok_or_else(|| Error::msg("Root missing"))?;
    let root_bcs = root.load(&ctx, repo.repo_blobstore()).await?;
    let file_1 = match root_bcs
        .file_changes()
        .find(|(path, _)| path == &&path_1)
        .ok_or_else(|| Error::msg("path_1 missing in file_changes"))?
        .1
    {
        FileChange::Change(tc) => tc.clone(),
        _ => return Err(Error::msg("path_1 change info missing")),
    };
    assert_eq!(file_1.file_type(), FileType::Regular);
    let file_1_exec = FileChange::tracked(
        file_1.content_id(),
        FileType::Executable,
        file_1.size(),
        /* copy_from */ None,
        GitLfs::FullContent,
    );

    let bcs = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file_change(path_1.clone(), file_1_exec.clone())
        .commit()
        .await?;

    let hgcss = hashset![repo.derive_hg_changeset(&ctx, bcs).await?];

    let book = master_bookmark();
    set_bookmark(
        ctx.clone(),
        &repo,
        &book,
        "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
    )
    .await?;

    let result = do_pushrebase(&ctx, &repo, &Default::default(), &book, &hgcss).await?;
    let result_bcs = result.head.load(&ctx, repo.repo_blobstore()).await?;
    let file_1_result = match result_bcs
        .file_changes()
        .find(|(path, _)| path == &&path_1)
        .ok_or_else(|| Error::msg("path_1 missing in file_changes"))?
        .1
    {
        FileChange::Change(tc) => tc.clone(),
        _ => return Err(Error::msg("path_1 change info missing")),
    };
    assert_eq!(FileChange::Change(file_1_result), file_1_exec);

    let result_hg = repo.derive_hg_changeset(&ctx, result.head).await?;
    let result_cs = result_hg.load(&ctx, repo.repo_blobstore()).await?;
    let result_1_id = result_cs
        .manifestid()
        .find_entry(
            ctx.clone(),
            repo.repo_blobstore().clone(),
            path_1.clone().into(),
        )
        .await?
        .and_then(|entry| Some(entry.into_leaf()?.1))
        .ok_or_else(|| Error::msg("path_1 missing in manifest"))?;

    // `result_1_id` should be equal to `root_1_id`, because executable flag
    // is not a part of file envelope
    assert_eq!(root_1_id, result_1_id);

    Ok(())
}

async fn count_commits_between(
    ctx: CoreContext,
    repo: &(impl Repo + BonsaiHgMappingRef),
    ancestor: HgChangesetId,
    descendant: BookmarkKey,
) -> Result<usize, Error> {
    let ancestor = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(&ctx, ancestor)
        .await?
        .ok_or_else(|| Error::msg("ancestor not found"))?;

    let descendant = repo
        .get_bookmark_hg(ctx.clone(), &descendant)
        .await?
        .ok_or_else(|| Error::msg("bookmark not found"))?;

    let descendant = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(&ctx, descendant)
        .await?
        .ok_or_else(|| Error::msg("bonsai not found"))?;

    let n = repo
        .commit_graph()
        .range_stream(&ctx, ancestor, descendant)
        .await?
        .count()
        .await;

    Ok(n)
}

#[derive(Copy, Clone)]
struct SleepHook;

#[async_trait]
impl PushrebaseHook for SleepHook {
    async fn in_critical_section(
        &self,
        _ctx: &CoreContext,
        _old_bookmark_value: Option<ChangesetId>,
    ) -> Result<Box<dyn PushrebaseCommitHook>, Error> {
        let us = rand::random_range(0..100);
        tokio::time::sleep(Duration::from_micros(us)).await;
        Ok(Box::new(*self) as Box<dyn PushrebaseCommitHook>)
    }
}

#[async_trait]
impl PushrebaseCommitHook for SleepHook {
    fn post_rebase_changeset(
        &mut self,
        _bcs_old: ChangesetId,
        _bcs_new: &mut BonsaiChangesetMut,
    ) -> Result<(), Error> {
        Ok(())
    }

    async fn into_transaction_hook(
        self: Box<Self>,
        _ctx: &CoreContext,
        _changesets: &RebasedChangesets,
    ) -> Result<Box<dyn PushrebaseTransactionHook>, Error> {
        Ok(Box::new(*self) as Box<dyn PushrebaseTransactionHook>)
    }
}

#[async_trait]
impl PushrebaseTransactionHook for SleepHook {
    async fn populate_transaction(
        &self,
        _ctx: &CoreContext,
        txn: Transaction,
    ) -> Result<Transaction, BookmarkTransactionError> {
        Ok(txn)
    }
}

#[mononoke::fbinit_test]
async fn pushrebase_simultaneously(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
    // Bottom commit of the repo
    let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
    let p = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(&ctx, root)
        .await?
        .ok_or_else(|| Error::msg("Root is missing"))?;
    let parents = vec![p];

    let book = master_bookmark();
    set_bookmark(
        ctx.clone(),
        &repo,
        &book,
        "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
    )
    .await?;

    let num_pushes = 10;
    let mut futs = vec![];
    for i in 0..num_pushes {
        cloned!(ctx, repo, book);

        let hooks = [Box::new(SleepHook) as Box<dyn PushrebaseHook>];

        let f = format!("file{i}");
        let bcs_id = CreateCommitContext::new(&ctx, &repo, parents.clone())
            .add_file(f.as_str(), "content")
            .commit()
            .await?;

        let bcs = bcs_id.load(&ctx, repo.repo_blobstore()).await?;

        let fut = async move {
            do_pushrebase_bonsai(
                &ctx,
                &repo,
                &Default::default(),
                &book,
                &hashset![bcs],
                &hooks,
            )
            .await
        };

        futs.push(fut);
    }

    let res = try_join_all(futs).await?;
    let mut has_retry_num_bigger_1 = false;
    for r in res {
        if r.retry_num.0 > 1 {
            has_retry_num_bigger_1 = true;
        }
    }

    assert!(has_retry_num_bigger_1);

    let previous_master = HgChangesetId::from_str("a5ffa77602a066db7d5cfb9fb5823a0895717c5a")?;
    let commits_between = count_commits_between(ctx, &repo, previous_master, book).await?;

    // `- 1` because range_stream is inclusive
    assert_eq!(commits_between - 1, num_pushes);

    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_create_new_bookmark(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
    // Bottom commit of the repo
    let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
    let p = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(&ctx, root)
        .await?
        .ok_or_else(|| Error::msg("Root is missing"))?;
    let parents = vec![p];

    let bcs_id = CreateCommitContext::new(&ctx, &repo, parents)
        .add_file("file", "content")
        .commit()
        .await?;

    let hg_cs = repo.derive_hg_changeset(&ctx, bcs_id).await?;

    let book = BookmarkKey::new("newbook")?;
    do_pushrebase(&ctx, &repo, &Default::default(), &book, &hashset![hg_cs]).await?;
    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_simultaneously_and_create_new(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
    // Bottom commit of the repo
    let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
    let p = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(&ctx, root)
        .await?
        .ok_or_else(|| Error::msg("Root is missing"))?;
    let parents = vec![p];

    let book = BookmarkKey::new("newbook")?;

    let num_pushes = 10;
    let mut futs = vec![];
    for i in 0..num_pushes {
        cloned!(ctx, repo, book);

        let hooks = [Box::new(SleepHook) as Box<dyn PushrebaseHook>];

        let f = format!("file{i}");
        let bcs_id = CreateCommitContext::new(&ctx, &repo, parents.clone())
            .add_file(f.as_str(), "content")
            .commit()
            .await?;

        let bcs = bcs_id.load(&ctx, repo.repo_blobstore()).await?;

        let fut = async move {
            do_pushrebase_bonsai(
                &ctx,
                &repo,
                &Default::default(),
                &book,
                &hashset![bcs],
                &hooks,
            )
            .await
        };

        futs.push(fut);
    }

    let res = try_join_all(futs).await?;
    let mut has_retry_num_bigger_1 = false;
    for r in res {
        if r.retry_num.0 > 1 {
            has_retry_num_bigger_1 = true;
        }
    }

    assert!(has_retry_num_bigger_1);

    let commits_between = count_commits_between(ctx, &repo, root, book).await?;
    // `- 1` because range_stream is inclusive
    assert_eq!(commits_between - 1, num_pushes);

    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_one_commit_with_bundle_id(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;
    // Bottom commit of the repo
    let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
    let p = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(&ctx, root)
        .await?
        .ok_or_else(|| Error::msg("Root is missing"))?;
    let parents = vec![p];

    let bcs_id = CreateCommitContext::new(&ctx, &repo, parents)
        .add_file("file", "content")
        .commit()
        .await?;
    let hg_cs = repo.derive_hg_changeset(&ctx, bcs_id).await?;

    let book = master_bookmark();
    set_bookmark(
        ctx.clone(),
        &repo,
        &book,
        "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
    )
    .await?;

    do_pushrebase(&ctx, &repo, &Default::default(), &book, &hashset![hg_cs]).await?;

    Ok(())
}

#[mononoke::fbinit_test]
async fn forbid_p2_root_rebases(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;

    let root = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(
            &ctx,
            HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
        )
        .await?
        .ok_or_else(|| Error::msg("Root is missing"))?;

    let bcs_id_0 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("merge_file", "merge content")
        .commit()
        .await?;
    let bcs_id_1 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_0, root])
        .add_file("file", "content")
        .commit()
        .await?;
    let hgcss = hashset![
        repo.derive_hg_changeset(&ctx, bcs_id_0).await?,
        repo.derive_hg_changeset(&ctx, bcs_id_1).await?,
    ];

    let book = master_bookmark();
    set_bookmark(
        ctx.clone(),
        &repo,
        &book,
        "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
    )
    .await?;

    let config_forbid_p2 = PushrebaseFlags {
        forbid_p2_root_rebases: true,
        ..Default::default()
    };

    assert!(
        do_pushrebase(&ctx, &repo, &config_forbid_p2, &book, &hgcss)
            .await
            .is_err()
    );

    let config_allow_p2 = PushrebaseFlags {
        forbid_p2_root_rebases: false,
        ..Default::default()
    };

    do_pushrebase(&ctx, &repo, &config_allow_p2, &book, &hgcss).await?;

    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_over_merge(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let p1 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("p1", "some content")
        .commit()
        .await?;

    let p2 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("p2", "some content")
        .commit()
        .await?;

    let merge = CreateCommitContext::new(&ctx, &repo, vec![p1, p2])
        .add_file("merge", "some content")
        .commit()
        .await?;

    let book = master_bookmark();

    let merge_hg_cs_id = repo.derive_hg_changeset(&ctx, merge).await?;

    set_bookmark(ctx.clone(), &repo, &book, &{
        // https://github.com/rust-lang/rust/pull/64856
        let r = format!("{merge_hg_cs_id}");
        r
    })
    .await?;

    // Modify a file touched in another branch - should fail
    push_and_verify(
        &ctx,
        &repo,
        p1,
        &book,
        btreemap! {"p2" => Some("some content")},
        false,
    )
    .await?;

    // Modify a file modified in th merge commit - should fail
    push_and_verify(
        &ctx,
        &repo,
        p1,
        &book,
        btreemap! {"merge" => Some("some content")},
        false,
    )
    .await?;

    // Any other files should succeed
    push_and_verify(
        &ctx,
        &repo,
        p1,
        &book,
        btreemap! {"p1" => Some("some content")},
        true,
    )
    .await?;

    push_and_verify(
        &ctx,
        &repo,
        p1,
        &book,
        btreemap! {"otherfile" => Some("some content")},
        true,
    )
    .await?;

    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_over_merge_even(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = MergeEven::get_repo(fb).await;

    // 4dcf230cd2f20577cb3e88ba52b73b376a2b3f69 - is a merge commit,
    // 3cda5c78aa35f0f5b09780d971197b51cad4613a is one of the ancestors
    let root = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(
            &ctx,
            HgChangesetId::from_str("3cda5c78aa35f0f5b09780d971197b51cad4613a")?,
        )
        .await?
        .ok_or_else(|| Error::msg("Root is missing"))?;

    // Modifies the same file "branch" - pushrebase should fail because of conflicts
    let bcs_id_should_fail = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("branch", "some content")
        .commit()
        .await?;

    let bcs_id_should_succeed = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("randomfile", "some content")
        .commit()
        .await?;

    let book = master_bookmark();

    let hgcss = hashset![repo.derive_hg_changeset(&ctx, bcs_id_should_fail).await?];

    let res = do_pushrebase(&ctx, &repo, &PushrebaseFlags::default(), &book, &hgcss).await;

    should_have_conflicts(res);
    let hgcss = hashset![
        repo.derive_hg_changeset(&ctx, bcs_id_should_succeed)
            .await?,
    ];

    do_pushrebase(&ctx, &repo, &PushrebaseFlags::default(), &book, &hgcss).await?;

    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_of_branch_merge(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    // Pushrebase two branch merges (bcs_id_first_merge and bcs_id_second_merge)
    // on top of master
    let bcs_id_base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("base", "base")
        .commit()
        .await?;

    let bcs_id_p1 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_base])
        .add_file("p1", "p1")
        .commit()
        .await?;

    let bcs_id_p2 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_base])
        .add_file("p2", "p2")
        .commit()
        .await?;

    let bcs_id_first_merge = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_p1, bcs_id_p2])
        .add_file("merge", "merge")
        .commit()
        .await?;

    let bcs_id_second_merge =
        CreateCommitContext::new(&ctx, &repo, vec![bcs_id_first_merge, bcs_id_p2])
            .add_file("merge2", "merge")
            .commit()
            .await?;

    // Modify base file again
    let bcs_id_master = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_p1])
        .add_file("base", "base2")
        .commit()
        .await?;

    let hg_cs = repo.derive_hg_changeset(&ctx, bcs_id_master).await?;

    let book = master_bookmark();
    set_bookmark(ctx.clone(), &repo, &book, &{
        // https://github.com/rust-lang/rust/pull/64856
        let r = format!("{hg_cs}");
        r
    })
    .await?;

    let hgcss = hashset![
        repo.derive_hg_changeset(&ctx, bcs_id_first_merge).await?,
        repo.derive_hg_changeset(&ctx, bcs_id_second_merge).await?,
    ];

    do_pushrebase(&ctx, &repo, &PushrebaseFlags::default(), &book, &hgcss).await?;

    let new_master = get_bookmark_value(&ctx, &repo, &BookmarkKey::new("master")?)
        .await?
        .ok_or_else(|| Error::msg("master not set"))?;

    let master_hg = repo.derive_hg_changeset(&ctx, new_master).await?;

    ensure_content(
        &ctx,
        master_hg,
        &repo,
        btreemap! {
                "base".to_string()=> "base2".to_string(),
                "merge".to_string()=> "merge".to_string(),
                "merge2".to_string()=> "merge".to_string(),
                "p1".to_string()=> "p1".to_string(),
                "p2".to_string()=> "p2".to_string(),
        },
    )
    .await?;

    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_of_branch_merge_with_removal(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    // Pushrebase two branch merges (bcs_id_first_merge and bcs_id_second_merge)
    // on top of master
    let bcs_id_base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("base", "base")
        .commit()
        .await?;

    let bcs_id_p1 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_base])
        .add_file("p1", "p1")
        .commit()
        .await?;

    let bcs_id_p2 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_base])
        .add_file("p2", "p2")
        .commit()
        .await?;

    let bcs_id_merge = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_p1, bcs_id_p2])
        .add_file("merge", "merge")
        .commit()
        .await?;

    // Modify base file again
    let bcs_id_master = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_p1])
        .delete_file("base")
        .add_file("anotherfile", "anotherfile")
        .commit()
        .await?;

    let hg_cs = repo.derive_hg_changeset(&ctx, bcs_id_master).await?;

    let book = master_bookmark();
    set_bookmark(ctx.clone(), &repo, &book, &{
        // https://github.com/rust-lang/rust/pull/64856
        let r = format!("{hg_cs}");
        r
    })
    .await?;

    let hgcss = hashset![repo.derive_hg_changeset(&ctx, bcs_id_merge).await?,];

    do_pushrebase(&ctx, &repo, &PushrebaseFlags::default(), &book, &hgcss).await?;

    let new_master = get_bookmark_value(&ctx, &repo, &BookmarkKey::new("master")?)
        .await?
        .ok_or_else(|| Error::msg("master not set"))?;

    let master_hg = repo.derive_hg_changeset(&ctx, new_master).await?;

    ensure_content(
        &ctx,
        master_hg,
        &repo,
        btreemap! {
                "anotherfile".to_string() => "anotherfile".to_string(),
                "merge".to_string()=> "merge".to_string(),
                "p1".to_string()=> "p1".to_string(),
                "p2".to_string()=> "p2".to_string(),
        },
    )
    .await?;

    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_of_branch_merge_with_rename(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    // Pushrebase two branch merges (bcs_id_first_merge and bcs_id_second_merge)
    // on top of master
    let bcs_id_base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("base", "base")
        .commit()
        .await?;

    let bcs_id_p1 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_base])
        .add_file("p1", "p1")
        .commit()
        .await?;

    let bcs_id_p2 = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_base])
        .add_file("p2", "p2")
        .commit()
        .await?;

    let bcs_id_merge = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_p1, bcs_id_p2])
        .add_file("merge", "merge")
        .commit()
        .await?;

    // Remove base file
    let bcs_id_pre_pre_master = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_p1])
        .delete_file("base")
        .commit()
        .await?;

    // Move to base file
    let bcs_id_pre_master = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_pre_pre_master])
        .add_file_with_copy_info("base", "somecontent", (bcs_id_pre_pre_master, "p1"))
        .commit()
        .await?;

    let bcs_id_master = CreateCommitContext::new(&ctx, &repo, vec![bcs_id_pre_master])
        .add_file("somefile", "somecontent")
        .commit()
        .await?;

    let hg_cs = repo.derive_hg_changeset(&ctx, bcs_id_master).await?;

    let book = master_bookmark();
    set_bookmark(ctx.clone(), &repo, &book, &{
        // https://github.com/rust-lang/rust/pull/64856
        let r = format!("{hg_cs}");
        r
    })
    .await?;

    let hgcss = hashset![repo.derive_hg_changeset(&ctx, bcs_id_merge).await?];

    do_pushrebase(&ctx, &repo, &PushrebaseFlags::default(), &book, &hgcss).await?;

    let new_master = get_bookmark_value(&ctx, &repo.clone(), &BookmarkKey::new("master")?)
        .await?
        .ok_or_else(|| Error::msg("master is not set"))?;

    let master_hg = repo.derive_hg_changeset(&ctx, new_master).await?;

    ensure_content(
        &ctx,
        master_hg,
        &repo,
        btreemap! {
                "base".to_string() => "somecontent".to_string(),
                "somefile".to_string() => "somecontent".to_string(),
                "merge".to_string()=> "merge".to_string(),
                "p1".to_string()=> "p1".to_string(),
                "p2".to_string()=> "p2".to_string(),
        },
    )
    .await?;

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_pushrebase_new_repo_merge_no_new_file_changes(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;

    // First commit in the new repo
    let other_first_commit = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("otherrepofile", "otherrepocontent")
        .commit()
        .await?;

    let bcs_id = CreateCommitContext::new_root(&ctx, &repo)
        // Bottom commit of the main repo
        .add_parent("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")
        .add_parent(other_first_commit)
        .commit()
        .await?;

    let hg_cs = repo.derive_hg_changeset(&ctx, bcs_id).await?;

    let result = do_pushrebase(
        &ctx,
        &repo,
        &Default::default(),
        &master_bookmark(),
        &hashset![hg_cs],
    )
    .map_err(|err| format_err!("{err:?}"))
    .await?;

    let bcs = result.head.load(&ctx, repo.repo_blobstore()).await?;
    assert_eq!(bcs.file_changes().collect::<Vec<_>>(), vec![]);

    let master_hg = repo.derive_hg_changeset(&ctx, result.head).await?;

    ensure_content(
        &ctx,
        master_hg,
        &repo,
        btreemap! {
                "1".to_string()=> "1\n".to_string(),
                "2".to_string()=> "2\n".to_string(),
                "3".to_string()=> "3\n".to_string(),
                "4".to_string()=> "4\n".to_string(),
                "5".to_string()=> "5\n".to_string(),
                "6".to_string()=> "6\n".to_string(),
                "7".to_string()=> "7\n".to_string(),
                "8".to_string()=> "8\n".to_string(),
                "9".to_string()=> "9\n".to_string(),
                "10".to_string()=> "modified10\n".to_string(),

                "files".to_string()=> "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n".to_string(),
                "otherrepofile".to_string()=> "otherrepocontent".to_string(),
        },
    )
    .await?;

    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_test_failpushrebase_extra(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;

    // Create one commit on top of latest commit in the linear repo
    let before_head_commit = "79a13814c5ce7330173ec04d279bf95ab3f652fb";
    let head_bcs_id = CreateCommitContext::new(&ctx, &repo, vec![before_head_commit])
        .add_file("file", "content")
        .add_extra(FAIL_PUSHREBASE_EXTRA.to_string(), vec![])
        .commit()
        .await?;

    bookmark(&ctx, &repo, "head").set_to(head_bcs_id).await?;

    let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![before_head_commit])
        .add_file("file", "content2")
        .commit()
        .await?;

    let hg_cs = repo.derive_hg_changeset(&ctx, bcs_id).await?;

    let err = do_pushrebase(
        &ctx,
        &repo,
        &Default::default(),
        &BookmarkKey::new("head")?,
        &hashset![hg_cs],
    )
    .await;

    match err {
        Err(PushrebaseError::ForceFailPushrebase(_)) => {}
        _ => {
            return Err(format_err!(
                "unexpected result: expected ForceFailPushrebase error, found {err:?}"
            ));
        }
    };

    // Now create the same commit on top of head commit - pushrebase should succeed
    let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![head_bcs_id])
        .add_file("file", "content2")
        .commit()
        .await?;

    let hg_cs = repo.derive_hg_changeset(&ctx, bcs_id).await?;

    do_pushrebase(
        &ctx,
        &repo,
        &Default::default(),
        &BookmarkKey::new("head")?,
        &hashset![hg_cs],
    )
    .map_err(|err| format_err!("{err:?}"))
    .await?;

    Ok(())
}

async fn ensure_content(
    ctx: &CoreContext,
    hg_cs_id: HgChangesetId,
    repo: &impl Repo,
    expected: BTreeMap<String, String>,
) -> Result<(), Error> {
    let cs = hg_cs_id.load(ctx, repo.repo_blobstore()).await?;

    let entries = cs
        .manifestid()
        .list_all_entries(ctx.clone(), repo.repo_blobstore().clone())
        .try_collect::<Vec<_>>()
        .await?;

    let mut actual = BTreeMap::new();
    for (path, entry) in entries {
        match entry {
            Entry::Leaf((_, filenode_id)) => {
                let store = repo.repo_blobstore();
                let content_id = filenode_id.load(ctx, store).await?.content_id();
                let content = filestore::fetch_concat(store, ctx, content_id).await?;

                let s = String::from_utf8_lossy(content.as_ref()).into_owned();
                actual.insert(
                    format!("{}", Option::<NonRootMPath>::from(path).unwrap()),
                    s,
                );
            }
            Entry::Tree(_) => {}
        }
    }

    assert_eq!(expected, actual);

    Ok(())
}

#[mononoke::fbinit_test]
async fn batched_pushrebase_two_stacks(fb: FacebookInit) -> Result<(), Error> {
    #[derive(Copy, Clone)]
    struct Hook(RepositoryId);

    #[async_trait]
    impl PushrebaseHook for Hook {
        async fn in_critical_section(
            &self,
            _ctx: &CoreContext,
            _old_bookmark_value: Option<ChangesetId>,
        ) -> Result<Box<dyn PushrebaseCommitHook>, Error> {
            Ok(Box::new(*self) as Box<dyn PushrebaseCommitHook>)
        }
    }

    #[async_trait]
    impl PushrebaseCommitHook for Hook {
        fn post_rebase_changeset(
            &mut self,
            _bcs_old: ChangesetId,
            _bcs_new: &mut BonsaiChangesetMut,
        ) -> Result<(), Error> {
            Ok(())
        }

        async fn into_transaction_hook(
            self: Box<Self>,
            _ctx: &CoreContext,
            changesets: &RebasedChangesets,
        ) -> Result<Box<dyn PushrebaseTransactionHook>, Error> {
            Ok(Box::new(TransactionHook(self.0, changesets.len()))
                as Box<dyn PushrebaseTransactionHook>)
        }
    }

    struct TransactionHook(RepositoryId, usize);

    #[async_trait]
    impl PushrebaseTransactionHook for TransactionHook {
        async fn populate_transaction(
            &self,
            ctx: &CoreContext,
            txn: Transaction,
        ) -> Result<Transaction, BookmarkTransactionError> {
            let ret = SqlMutableCounters::set_counter_on_txn(
                ctx,
                self.0,
                "batched_hook_changesets",
                self.1 as i64,
                None,
                txn,
            )
            .await?;

            match ret {
                TransactionResult::Succeeded(txn) => Ok(txn),
                TransactionResult::Failed => Err(Error::msg("Did not update").into()),
            }
        }
    }

    let ctx = CoreContext::test_mock(fb);
    let factory = TestRepoFactory::new(fb)?;
    let repo: PushrebaseTestRepo = factory.build().await?;
    let (commits, _dag) = Linear::init_repo(fb, &repo).await?;

    let master_cs = commits["K"];
    let root_bcs_id = commits["A"];
    let root_bcs_id_b = commits["B"];

    // Stack A: adds "fileA"
    let bcs_id_a = CreateCommitContext::new(&ctx, &repo, vec![root_bcs_id])
        .add_file("fileA", "content_a")
        .commit()
        .await?;
    let bcs_a = bcs_id_a.load(&ctx, repo.repo_blobstore()).await?;

    // Stack B: adds "fileB"
    let bcs_id_b = CreateCommitContext::new(&ctx, &repo, vec![root_bcs_id_b])
        .add_file("fileB", "content_b")
        .commit()
        .await?;
    let bcs_b = bcs_id_b.load(&ctx, repo.repo_blobstore()).await?;

    // Index both stacks
    let bookmark = master_bookmark();
    let config = PushrebaseFlags::default();
    let PushrebaseRequestIndex {
        changed_files: cf_a,
        changesets: changesets_a,
        head: head_a,
        root: root_a,
    } = index_pushrebase_request(&ctx, &repo, &config, &bookmark, &hashset![bcs_a]).await?;
    let PushrebaseRequestIndex {
        changed_files: cf_b,
        changesets: changesets_b,
        head: head_b,
        root: root_b,
    } = index_pushrebase_request(&ctx, &repo, &config, &bookmark, &hashset![bcs_b]).await?;

    // Build PushrebaseRequests with oneshot channels
    let (tx_a, rx_a) = oneshot::channel();
    let (tx_b, rx_b) = oneshot::channel();

    // Only the first request needs hooks (batched pushrebase uses hooks from requests[0])
    let hook_a: Box<dyn PushrebaseHook> = Box::new(Hook(repo.repo_identity().id()));
    let hook_b: Box<dyn PushrebaseHook> = Box::new(Hook(repo.repo_identity().id()));

    let req_a = PushrebaseRequest {
        changed_files: cf_a,
        changesets: changesets_a,
        head: head_a,
        root: root_a,
        conflict_check_base: root_a,
        carried_merge_file_info: vec![],
        retry_num: PushrebaseRetryNum(0),
        hooks: vec![hook_a],
        response_tx: tx_a,
    };

    let req_b = PushrebaseRequest {
        changed_files: cf_b,
        changesets: changesets_b,
        head: head_b,
        root: root_b,
        conflict_check_base: root_b,
        carried_merge_file_info: vec![],
        retry_num: PushrebaseRetryNum(0),
        hooks: vec![hook_b],
        response_tx: tx_b,
    };

    // Call do_batched_pushrebase
    let requeued = do_batched_pushrebase(&ctx, &repo, &config, &bookmark, vec![req_a, req_b]).await;

    // No CAS failures expected
    assert!(requeued.is_empty(), "Expected no re-queued requests");

    // Both receivers should get Ok outcomes
    let outcome_a = rx_a.await.unwrap().map_err(|e| format_err!("{e:?}"))?;
    let outcome_b = rx_b.await.unwrap().map_err(|e| format_err!("{e:?}"))?;

    // outcome_a sees the original bookmark value (it was rebased first)
    assert_eq!(outcome_a.old_bookmark_value, Some(master_cs));
    // outcome_b sees the head after A was rebased (running_head before B's rebase)
    assert_eq!(outcome_b.old_bookmark_value, Some(outcome_a.head));

    // Both should have one rebased changeset
    assert_eq!(outcome_a.rebased_changesets.len(), 1);
    assert_eq!(outcome_b.rebased_changesets.len(), 1);

    // Pushrebase distance should be 10 (A to K in the Linear fixture)
    assert_eq!(outcome_a.pushrebase_distance.0, 10);
    assert_eq!(outcome_b.pushrebase_distance.0, 9);

    // The final bookmark should point to outcome_b's head (second stack lands on top of first)
    let new_master = resolve_cs_id(&ctx, &repo, "master").await?;
    assert_eq!(new_master, outcome_b.head);

    // Verify the hook fired: the transaction hook should have written the total
    // number of rebased changesets (2, one per stack) to the mutable counter
    assert_eq!(
        repo.mutable_counters()
            .get_counter(&ctx, "batched_hook_changesets")
            .await?,
        Some(2),
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn batched_pushrebase_one_conflict(fb: FacebookInit) -> Result<(), Error> {
    init_just_knobs_for_test();
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits, _dag): (PushrebaseTestRepo, _, _) = Linear::get_repo_and_dag(fb).await;

    let root_bcs_id = commits["A"];

    // Stack A: adds a new file (no conflict)
    let bcs_id_a = CreateCommitContext::new(&ctx, &repo, vec![root_bcs_id])
        .add_file("new_file", "content")
        .commit()
        .await?;
    let bcs_a = bcs_id_a.load(&ctx, repo.repo_blobstore()).await?;

    // Stack B: modifies "files" which is also modified by commits B-K (conflict)
    let bcs_id_b = CreateCommitContext::new(&ctx, &repo, vec![root_bcs_id])
        .add_file("files", "conflicting content")
        .commit()
        .await?;
    let bcs_b = bcs_id_b.load(&ctx, repo.repo_blobstore()).await?;

    // Index both stacks
    let bookmark = master_bookmark();
    let config = PushrebaseFlags::default();
    let PushrebaseRequestIndex {
        changed_files: cf_a,
        changesets: changesets_a,
        head: head_a,
        root: root_a,
    } = index_pushrebase_request(&ctx, &repo, &config, &bookmark, &hashset![bcs_a]).await?;
    let PushrebaseRequestIndex {
        changed_files: cf_b,
        changesets: changesets_b,
        head: head_b,
        root: root_b,
    } = index_pushrebase_request(&ctx, &repo, &config, &bookmark, &hashset![bcs_b]).await?;

    let (tx_a, rx_a) = oneshot::channel();
    let (tx_b, rx_b) = oneshot::channel();

    let req_a = PushrebaseRequest {
        changed_files: cf_a,
        changesets: changesets_a,
        head: head_a,
        root: root_a,
        conflict_check_base: root_a,
        carried_merge_file_info: vec![],
        retry_num: PushrebaseRetryNum(0),
        hooks: vec![],
        response_tx: tx_a,
    };

    let req_b = PushrebaseRequest {
        changed_files: cf_b,
        changesets: changesets_b,
        head: head_b,
        root: root_b,
        conflict_check_base: root_b,
        carried_merge_file_info: vec![],
        retry_num: PushrebaseRetryNum(0),
        hooks: vec![],
        response_tx: tx_b,
    };

    let requeued = do_batched_pushrebase(&ctx, &repo, &config, &bookmark, vec![req_a, req_b]).await;
    assert!(requeued.is_empty(), "Expected no re-queued requests");

    // Stack A should succeed
    let outcome_a = rx_a.await.unwrap().map_err(|e| format_err!("{e:?}"))?;
    assert_eq!(outcome_a.rebased_changesets.len(), 1);

    // Stack B should fail with conflicts
    let result_b = rx_b.await.unwrap();
    assert!(result_b.is_err(), "Expected stack B to fail with conflicts");
    match result_b.unwrap_err().inner() {
        PushrebaseError::Conflicts(_) => {}
        other => panic!("Expected Conflicts error, got: {other:?}"),
    }

    // Bookmark should still be updated (stack A succeeded)
    let new_master = resolve_cs_id(&ctx, &repo, "master").await?;
    assert_eq!(new_master, outcome_a.head);

    Ok(())
}

/// Verify that a rebase failure in one request cannot corrupt hook
/// state that flows into the bookmark transaction for other requests.
///
/// Setup: three requests [A, B, C] in one batch.  The shared commit
/// hook records an assignment for every changeset it sees (mimicking
/// globalrev), then fails on request B's changeset — *after* recording
/// the phantom assignment.  With the bug, the loop would continue to
/// C, and `into_transaction_hook` would fire with 3 recorded
/// assignments but only 2 entries in `all_rebased_changesets`.  A hook
/// without its own count-check (or one that uses the recorded count
/// for sequencing) would write corrupt data to the transaction.
///
/// The fix aborts the batch on the first `create_rebased_changesets`
/// failure, so `into_transaction_hook` is never reached and no data
/// is written.
#[mononoke::fbinit_test]
async fn batched_pushrebase_rebase_failure_prevents_corrupt_hook_data(
    fb: FacebookInit,
) -> Result<(), Error> {
    #[derive(Clone)]
    struct TrackingHook(RepositoryId);

    /// Commit hook that records one assignment per changeset it sees,
    /// then fails on the Nth call.  The assignment is recorded
    /// *before* the error check — this is the state contamination.
    struct TrackingCommitHook {
        repo_id: RepositoryId,
        assignments: usize,
        fail_on_call: usize,
    }

    #[async_trait]
    impl PushrebaseHook for TrackingHook {
        async fn in_critical_section(
            &self,
            _ctx: &CoreContext,
            _old_bookmark_value: Option<ChangesetId>,
        ) -> Result<Box<dyn PushrebaseCommitHook>, Error> {
            Ok(Box::new(TrackingCommitHook {
                repo_id: self.0,
                assignments: 0,
                // Requests are processed in order: A(call 1), B(call 2), C(call 3).
                // Fail on call 2 (request B) after recording its assignment.
                fail_on_call: 2,
            }))
        }
    }

    #[async_trait]
    impl PushrebaseCommitHook for TrackingCommitHook {
        fn post_rebase_changeset(
            &mut self,
            _bcs_old: ChangesetId,
            _bcs_new: &mut BonsaiChangesetMut,
        ) -> Result<(), Error> {
            // Record the assignment FIRST (mirrors globalrev hook which
            // calls set_on_changeset + insert + increment before returning).
            self.assignments += 1;
            if self.assignments == self.fail_on_call {
                return Err(anyhow::anyhow!("simulated rebase failure"));
            }
            Ok(())
        }

        async fn into_transaction_hook(
            self: Box<Self>,
            ctx: &CoreContext,
            _changesets: &RebasedChangesets,
        ) -> Result<Box<dyn PushrebaseTransactionHook>, Error> {
            // Write the number of recorded assignments to a mutable
            // counter.  If hook state was contaminated by the failed
            // request, this value is WRONG — it includes a phantom
            // assignment for a changeset that was never committed.
            Ok(Box::new(WriteCountHook {
                repo_id: self.repo_id,
                count: self.assignments,
                ctx: ctx.clone(),
            }))
        }
    }

    struct WriteCountHook {
        repo_id: RepositoryId,
        count: usize,
        ctx: CoreContext,
    }

    #[async_trait]
    impl PushrebaseTransactionHook for WriteCountHook {
        async fn populate_transaction(
            &self,
            _ctx: &CoreContext,
            txn: Transaction,
        ) -> Result<Transaction, BookmarkTransactionError> {
            let ret = SqlMutableCounters::set_counter_on_txn(
                &self.ctx,
                self.repo_id,
                "hook_assignments",
                self.count as i64,
                None,
                txn,
            )
            .await?;
            match ret {
                TransactionResult::Succeeded(txn) => Ok(txn),
                TransactionResult::Failed => Err(Error::msg("counter write failed").into()),
            }
        }
    }

    let ctx = CoreContext::test_mock(fb);
    let factory = TestRepoFactory::new(fb)?;
    let repo: PushrebaseTestRepo = factory.build().await?;
    let (commits, _dag) = Linear::init_repo(fb, &repo).await?;

    let root = commits["A"];
    let bookmark = master_bookmark();
    let config = PushrebaseFlags::default();

    // Three non-conflicting stacks, each with one changeset.
    let mut requests = Vec::new();
    let mut receivers = Vec::new();
    for name in ["fileA", "fileB", "fileC"] {
        let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file(name, "content")
            .commit()
            .await?;
        let bcs = bcs_id.load(&ctx, repo.repo_blobstore()).await?;
        let idx = index_pushrebase_request(&ctx, &repo, &config, &bookmark, &hashset![bcs]).await?;
        let (tx, rx) = oneshot::channel();
        requests.push(PushrebaseRequest {
            changed_files: idx.changed_files,
            changesets: idx.changesets,
            head: idx.head,
            root: idx.root,
            conflict_check_base: idx.root,
            carried_merge_file_info: vec![],
            retry_num: PushrebaseRetryNum(0),
            hooks: vec![Box::new(TrackingHook(repo.repo_identity().id()))],
            response_tx: tx,
        });
        receivers.push(rx);
    }

    let requeued = do_batched_pushrebase(&ctx, &repo, &config, &bookmark, requests).await;

    // Request B (index 1) should have received the hook error.
    let result_b = receivers.remove(1).await.unwrap();
    assert!(result_b.is_err(), "Request B should have failed");

    // Requests A and C should be requeued — NOT resolved with
    // corrupt hook data flowing into the transaction.
    assert_eq!(requeued.len(), 2, "Requests A and C should be requeued");

    // The transaction hook must NOT have fired.  If it did, it would
    // have written a count of 3 (including the phantom assignment
    // from failed request B) — that's data corruption.
    assert_eq!(
        repo.mutable_counters()
            .get_counter(&ctx, "hook_assignments")
            .await?,
        None,
        "into_transaction_hook should not have been reached",
    );

    Ok(())
}

fn should_have_conflicts(res: Result<PushrebaseOutcome, PushrebaseError>) {
    match res {
        Err(err) => match err {
            PushrebaseError::Conflicts(_) => {}
            _ => {
                panic!("pushrebase should have had conflicts");
            }
        },
        Ok(_) => {
            panic!("pushrebase should have failed");
        }
    }
}

fn init_just_knobs_for_merge_test() {
    override_just_knobs(JustKnobsInMemory::new(hashmap! {
        "scm/mononoke:pushrebase_enable_merge_resolution".to_string() => KnobVal::Bool(true),
        "scm/mononoke:pushrebase_merge_resolution_derive_fsnodes".to_string() => KnobVal::Bool(true),
        "scm/mononoke:pushrebase_range_diff_use_content_manifests".to_string() => KnobVal::Bool(false),
    }));
}

#[mononoke::fbinit_test]
async fn pushrebase_merge_resolution_clean(fb: FacebookInit) -> Result<(), Error> {
    // Test: server and client modify different parts of the same file.
    // With merge resolution enabled, pushrebase should succeed and
    // the resulting file should contain both modifications.
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

    // Create a base commit with a multi-line file
    let base_content = "line1\nline2\nline3\nline4\nline5\n";
    let base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file.txt", base_content)
        .commit()
        .await?;

    // Server-side commit: modify the first line
    let server_content = "modified_line1\nline2\nline3\nline4\nline5\n";
    let server = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", server_content)
        .commit()
        .await?;

    // Set bookmark to the server commit
    let book = BookmarkKey::new("master")?;
    let hg_server = repo.derive_hg_changeset(&ctx, server).await?;
    set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_server}")).await?;

    // Client-side commit (based on base, not server): modify the last line
    let client_content = "line1\nline2\nline3\nline4\nmodified_line5\n";
    let client = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", client_content)
        .commit()
        .await?;

    let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;

    // Enable merge resolution
    init_just_knobs_for_merge_test();

    let result = do_pushrebase_bonsai(
        &ctx,
        &repo,
        &Default::default(),
        &book,
        &hashset![client_bcs],
        &[],
    )
    .await?;

    // Verify the merged content has both modifications
    let result_hg = repo.derive_hg_changeset(&ctx, result.head).await?;
    let expected_content = "modified_line1\nline2\nline3\nline4\nmodified_line5\n";
    ensure_content(
        &ctx,
        result_hg,
        &repo,
        btreemap! {
            "file.txt".to_string() => expected_content.to_string(),
        },
    )
    .await?;

    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_merge_resolution_conflict(fb: FacebookInit) -> Result<(), Error> {
    // Test: server and client modify the SAME line of a file.
    // Even with merge resolution enabled, pushrebase should fail
    // because the merge has a true content-level conflict.
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

    // Create a base commit with a multi-line file
    let base_content = "line1\nline2\nline3\n";
    let base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file.txt", base_content)
        .commit()
        .await?;

    // Server-side commit: modify line 2
    let server_content = "line1\nserver_modified\nline3\n";
    let server = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", server_content)
        .commit()
        .await?;

    // Set bookmark to the server commit
    let book = BookmarkKey::new("master")?;
    let hg_server = repo.derive_hg_changeset(&ctx, server).await?;
    set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_server}")).await?;

    // Client-side commit (based on base): also modify line 2 differently
    let client_content = "line1\nclient_modified\nline3\n";
    let client = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", client_content)
        .commit()
        .await?;

    let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;

    // Enable merge resolution
    init_just_knobs_for_merge_test();

    let result = do_pushrebase_bonsai(
        &ctx,
        &repo,
        &Default::default(),
        &book,
        &hashset![client_bcs],
        &[],
    )
    .await;

    // Should fail with conflicts because of overlapping edits
    should_have_conflicts(result);

    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_merge_resolution_carry_forward_on_retry(fb: FacebookInit) -> Result<(), Error> {
    // Test: On CAS retry, attempt 2 uses a narrow range (S1→S2) that
    // does NOT contain the original conflict. The carried MergedFileInfo
    // from attempt 1 is used via reconciliation.
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

    let base_content = "\
line 1
line 2
line 3
line 4
line 5
line 6
line 7
line 8
";
    let base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file.txt", base_content)
        .commit()
        .await?;

    // Server commit S1: adds "line 2.1" between line 2 and line 3
    let s1_content = "\
line 1
line 2
line 2.1
line 3
line 4
line 5
line 6
line 7
line 8
";
    let s1 = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", s1_content)
        .commit()
        .await?;

    // Server commit S2: unrelated change (different file) — simulates
    // another push that moves the bookmark after S1
    let s2 = CreateCommitContext::new(&ctx, &repo, vec![s1])
        .add_file("unrelated.txt", "unrelated change\n")
        .commit()
        .await?;

    // Set bookmark to S2 (after both server commits)
    let book = BookmarkKey::new("master")?;
    let hg_s2 = repo.derive_hg_changeset(&ctx, s2).await?;
    set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_s2}")).await?;

    // Client commit: adds "line 6.1" between line 6 and line 7
    // (based on base, NOT on server commits)
    let client_content = "\
line 1
line 2
line 3
line 4
line 5
line 6
line 6.1
line 7
line 8
";
    let client = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", client_content)
        .commit()
        .await?;

    let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;

    init_just_knobs_for_merge_test();

    let client_cf = find_changed_files(&ctx, &repo, base, client).await?;

    // --- Attempt 1: full range base → S1 (first bookmark position) ---
    let result1 = check_pushrebase_conflicts(
        &ctx,
        &repo,
        &Default::default(),
        base,
        base,
        s1,
        std::slice::from_ref(&client_bcs),
        &client_cf,
    )
    .await?;
    assert!(
        result1.merged_file_overrides.is_some(),
        "Attempt 1 should detect the conflict and produce merge overrides"
    );

    // --- Attempt 2: narrow range S1 → S2 (only the delta after CAS fail) ---
    // S2 only changes unrelated.txt, so no conflict with client's file.txt
    let result2 = check_pushrebase_conflicts(
        &ctx,
        &repo,
        &Default::default(),
        base,
        s1,
        s2,
        std::slice::from_ref(&client_bcs),
        &client_cf,
    )
    .await?;
    assert!(
        result2.merged_file_overrides.is_none(),
        "Narrow range S1→S2 should have no conflicts (unrelated.txt only)"
    );

    // Simulate carry-forward: attempt 1 produces overrides, CAS fails,
    // attempt 2 sees no new conflicts but carried info is used.
    let carried = result1.merged_file_overrides.clone().unwrap();
    let reconciled = match result2.merged_file_overrides {
        Some(ref delta) => reconcile_merge_file_info(&carried, delta),
        None => carried,
    };

    // Rebase with the reconciled (carried) overrides
    let (new_head, _, rebased_bonsais) = create_rebased_changesets(
        &ctx,
        &repo,
        &Default::default(),
        find_rebased_set(&ctx, &repo, base, client).await?,
        base,
        client,
        s2,
        &mut [],
        Some(reconciled),
    )
    .await?;
    changesets_creation::save_changesets(&ctx, &repo, rebased_bonsais).await?;

    // Check the file content at the rebased head
    let result_hg = repo.derive_hg_changeset(&ctx, new_head).await?;
    let result_cs = result_hg.load(&ctx, repo.repo_blobstore()).await?;
    let manifest = result_cs.manifestid();
    let file_path = NonRootMPath::new("file.txt")?;
    let file_entry = manifest
        .find_entry(ctx.clone(), repo.repo_blobstore().clone(), file_path.into())
        .await?
        .expect("file.txt should exist");

    let file_content = match file_entry {
        Entry::Leaf((_, filenode_id)) => {
            let content_id = filenode_id
                .load(&ctx, repo.repo_blobstore())
                .await?
                .content_id();
            let bytes = filestore::fetch_concat(repo.repo_blobstore(), &ctx, content_id).await?;
            String::from_utf8(bytes.to_vec())?
        }
        _ => panic!("file.txt should be a file"),
    };

    // Both changes are preserved via carry-forward
    assert!(
        file_content.contains("line 6.1"),
        "rebased commit should have client's line 6.1"
    );
    assert!(
        file_content.contains("line 2.1"),
        "rebased commit should have server's line 2.1. Actual:\n{file_content}",
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_merge_resolution_carry_forward_with_new_server_changes(
    fb: FacebookInit,
) -> Result<(), Error> {
    // Test: When the same file is changed in both base→S1 and S1→S2,
    // the carry-forward reconciliation updates server_content_id from
    // the delta so the rebase uses the latest server content.
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

    let base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("a.txt", "a1\na2\na3\na4\na5\na6\na7\na8\n")
        .add_file("b.txt", "b1\nb2\nb3\nb4\nb5\nb6\nb7\nb8\n")
        .commit()
        .await?;

    // Server commit S1: modifies a.txt line 1 (conflict with client)
    let s1 = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("a.txt", "SERVER_a1\na2\na3\na4\na5\na6\na7\na8\n")
        .commit()
        .await?;

    // Server commit S2: modifies b.txt line 1 (conflict with client)
    let s2 = CreateCommitContext::new(&ctx, &repo, vec![s1])
        .add_file("b.txt", "SERVER_b1\nb2\nb3\nb4\nb5\nb6\nb7\nb8\n")
        .commit()
        .await?;

    let book = BookmarkKey::new("master")?;
    let hg_s2 = repo.derive_hg_changeset(&ctx, s2).await?;
    set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_s2}")).await?;

    // Client modifies both files at the END (non-overlapping with server)
    let client = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("a.txt", "a1\na2\na3\na4\na5\na6\na7\nCLIENT_a8\n")
        .add_file("b.txt", "b1\nb2\nb3\nb4\nb5\nb6\nb7\nCLIENT_b8\n")
        .commit()
        .await?;

    let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;

    init_just_knobs_for_merge_test();

    let client_cf = find_changed_files(&ctx, &repo, base, client).await?;

    // --- Attempt 1: base → S1 (only a.txt conflict detected) ---
    let result1 = check_pushrebase_conflicts(
        &ctx,
        &repo,
        &Default::default(),
        base,
        base,
        s1,
        std::slice::from_ref(&client_bcs),
        &client_cf,
    )
    .await?;
    assert!(
        result1.merged_file_overrides.is_some(),
        "Attempt 1 should resolve a.txt conflict"
    );
    let carried = result1.merged_file_overrides.unwrap();
    assert_eq!(carried.len(), 1, "Only a.txt should be in carried info");

    // --- Attempt 2: narrow range S1 → S2 (b.txt conflict detected) ---
    let result2 = check_pushrebase_conflicts(
        &ctx,
        &repo,
        &Default::default(),
        base,
        s1,
        s2,
        std::slice::from_ref(&client_bcs),
        &client_cf,
    )
    .await?;
    assert!(
        result2.merged_file_overrides.is_some(),
        "Attempt 2 should resolve b.txt conflict in narrow range"
    );
    let delta = result2.merged_file_overrides.unwrap();
    assert_eq!(delta.len(), 1, "Only b.txt should be in delta info");

    // Reconcile: carried has a.txt, delta has b.txt → union of both
    let reconciled = reconcile_merge_file_info(&carried, &delta);
    assert_eq!(
        reconciled.len(),
        2,
        "Reconciled should have both a.txt and b.txt"
    );

    // Rebase with reconciled overrides
    let (new_head, _, rebased_bonsais) = create_rebased_changesets(
        &ctx,
        &repo,
        &Default::default(),
        find_rebased_set(&ctx, &repo, base, client).await?,
        base,
        client,
        s2,
        &mut [],
        Some(reconciled),
    )
    .await?;
    changesets_creation::save_changesets(&ctx, &repo, rebased_bonsais).await?;

    // Verify both files have merged content
    let result_hg = repo.derive_hg_changeset(&ctx, new_head).await?;
    ensure_content(
        &ctx,
        result_hg,
        &repo,
        btreemap! {
            "a.txt".to_string() => "SERVER_a1\na2\na3\na4\na5\na6\na7\nCLIENT_a8\n".to_string(),
            "b.txt".to_string() => "SERVER_b1\nb2\nb3\nb4\nb5\nb6\nb7\nCLIENT_b8\n".to_string(),
        },
    )
    .await?;

    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_merge_resolution_stack_non_head_conflict(
    fb: FacebookInit,
) -> Result<(), Error> {
    // Regression test: 2-commit stack where the FIRST commit (not HEAD)
    // touches a conflicting file. The merge override must be applied to
    // that first commit, not HEAD; otherwise the first commit keeps stale
    // content that reverts the server's changes.
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

    let base_content = "\
line 1
line 2
line 3
line 4
line 5
line 6
line 7
line 8
";
    let base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file.txt", base_content)
        .add_file("other.txt", "other\n")
        .commit()
        .await?;

    // Server adds "line 2.1" between line 2 and line 3 (top region)
    let server_content = "\
line 1
line 2
line 2.1
line 3
line 4
line 5
line 6
line 7
line 8
";
    let server = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", server_content)
        .commit()
        .await?;

    let book = BookmarkKey::new("master")?;
    let hg_server = repo.derive_hg_changeset(&ctx, server).await?;
    set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_server}")).await?;

    // Client commit 1: adds "line 6.1" between line 6 and line 7 (bottom region)
    let client_content_1 = "\
line 1
line 2
line 3
line 4
line 5
line 6
line 6.1
line 7
line 8
";
    let client_1 = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", client_content_1)
        .commit()
        .await?;

    // Client commit 2 (HEAD): only touches other.txt, NOT file.txt
    let client_2 = CreateCommitContext::new(&ctx, &repo, vec![client_1])
        .add_file("other.txt", "modified other\n")
        .commit()
        .await?;

    let client_bcs_1 = client_1.load(&ctx, repo.repo_blobstore()).await?;
    let client_bcs_2 = client_2.load(&ctx, repo.repo_blobstore()).await?;

    init_just_knobs_for_merge_test();

    let result = do_pushrebase_bonsai(
        &ctx,
        &repo,
        &Default::default(),
        &book,
        &hashset![client_bcs_1.clone(), client_bcs_2.clone()],
        &[],
    )
    .await?;

    let expected_merged = "\
line 1
line 2
line 2.1
line 3
line 4
line 5
line 6
line 6.1
line 7
line 8
";

    // HEAD has the correct merged content
    let result_hg = repo.derive_hg_changeset(&ctx, result.head).await?;
    ensure_content(
        &ctx,
        result_hg,
        &repo,
        btreemap! {
            "file.txt".to_string() => expected_merged.to_string(),
            "other.txt".to_string() => "modified other\n".to_string(),
        },
    )
    .await?;

    // Read file.txt from the FIRST rebased commit
    let rebased_1 = result
        .rebased_changesets
        .iter()
        .find(|pair| pair.id_old == client_1)
        .map(|pair| pair.id_new)
        .expect("first commit should be in rebased set");

    let rebased_1_hg = repo.derive_hg_changeset(&ctx, rebased_1).await?;
    let rebased_1_cs = rebased_1_hg.load(&ctx, repo.repo_blobstore()).await?;
    let rebased_1_manifest = rebased_1_cs.manifestid();
    let file_path = NonRootMPath::new("file.txt")?;
    let file_entry = rebased_1_manifest
        .find_entry(ctx.clone(), repo.repo_blobstore().clone(), file_path.into())
        .await?
        .expect("file.txt should exist in first rebased commit");

    let file_content = match file_entry {
        Entry::Leaf((_, filenode_id)) => {
            let content_id = filenode_id
                .load(&ctx, repo.repo_blobstore())
                .await?
                .content_id();
            let bytes = filestore::fetch_concat(repo.repo_blobstore(), &ctx, content_id).await?;
            String::from_utf8(bytes.to_vec())?
        }
        _ => panic!("file.txt should be a file"),
    };

    assert!(
        file_content.contains("line 6.1"),
        "first rebased commit should have line 6.1 (client's change)"
    );

    // FIX: With cascading merge, the first rebased commit now has the
    // server's "line 2.1" because the merge is applied per-commit
    // during the rebase, not just to HEAD.
    assert!(
        file_content.contains("line 2.1"),
        "first rebased commit should have server's 'line 2.1' \
         (cascading merge applied per-commit). Actual:\n{file_content}",
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_stack_non_head_conflict_without_merge_resolution(
    fb: FacebookInit,
) -> Result<(), Error> {
    // Same scenario but merge resolution DISABLED: pushrebase should fail.
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

    let base_content = "\
line 1
line 2
line 3
line 4
line 5
";
    let base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file.txt", base_content)
        .add_file("other.txt", "other\n")
        .commit()
        .await?;

    let server_content = "\
line 1
line 2
line 2.1
line 3
line 4
line 5
";
    let server = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", server_content)
        .commit()
        .await?;

    let book = BookmarkKey::new("master")?;
    let hg_server = repo.derive_hg_changeset(&ctx, server).await?;
    set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_server}")).await?;

    let client_content = "\
line 1
line 2
line 3
line 4
line 5
line 5.1
";
    let client_1 = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", client_content)
        .commit()
        .await?;
    let client_2 = CreateCommitContext::new(&ctx, &repo, vec![client_1])
        .add_file("other.txt", "modified\n")
        .commit()
        .await?;

    let client_bcs_1 = client_1.load(&ctx, repo.repo_blobstore()).await?;
    let client_bcs_2 = client_2.load(&ctx, repo.repo_blobstore()).await?;

    init_just_knobs_for_test();

    let result = do_pushrebase_bonsai(
        &ctx,
        &repo,
        &Default::default(),
        &book,
        &hashset![client_bcs_1, client_bcs_2],
        &[],
    )
    .await;

    should_have_conflicts(result);

    Ok(())
}

#[mononoke::test]
fn reconcile_merge_file_info_basic() {
    use mononoke_types::hash::Blake2;

    let id_a = ContentId::new(Blake2::from_byte_array([1; 32]));
    let id_b = ContentId::new(Blake2::from_byte_array([2; 32]));
    let id_c = ContentId::new(Blake2::from_byte_array([3; 32]));
    let id_d = ContentId::new(Blake2::from_byte_array([4; 32]));

    let make_info = |path: &str, base: ContentId, server: ContentId| -> MergedFileInfo {
        MergedFileInfo {
            path: NonRootMPath::new(path).unwrap(),
            base_content_id: base,
            server_content_id: server,
            file_type: FileType::Regular,
        }
    };

    // Test 1: empty carried + non-empty delta returns delta
    let delta = vec![make_info("f1", id_a, id_b)];
    let result = reconcile_merge_file_info(&[], &delta);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].path, NonRootMPath::new("f1").unwrap());
    assert_eq!(result[0].server_content_id, id_b);

    // Test 2: non-empty carried + empty delta returns carried
    let carried = vec![make_info("f1", id_a, id_b)];
    let result = reconcile_merge_file_info(&carried, &[]);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].server_content_id, id_b);

    // Test 3: overlapping path updates server_content_id from delta
    let carried = vec![make_info("f1", id_a, id_b)];
    let delta = vec![make_info("f1", id_a, id_c)];
    let result = reconcile_merge_file_info(&carried, &delta);
    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0].server_content_id, id_c,
        "server_content_id should be updated from delta"
    );
    assert_eq!(
        result[0].base_content_id, id_a,
        "base_content_id should remain from carried"
    );

    // Test 4: non-overlapping paths produce union
    let carried = vec![make_info("f1", id_a, id_b)];
    let delta = vec![make_info("f2", id_c, id_d)];
    let result = reconcile_merge_file_info(&carried, &delta);
    assert_eq!(result.len(), 2, "Should have both f1 and f2");
    let has_f1 = result
        .iter()
        .any(|i| i.path == NonRootMPath::new("f1").unwrap());
    let has_f2 = result
        .iter()
        .any(|i| i.path == NonRootMPath::new("f2").unwrap());
    assert!(has_f1, "Should contain f1 from carried");
    assert!(has_f2, "Should contain f2 from delta");
}

#[mononoke::fbinit_test]
async fn pushrebase_merge_resolution_server_deletion_on_retry(
    fb: FacebookInit,
) -> Result<(), Error> {
    // Test: If a previously-conflicting file is deleted on the server
    // in the delta range, the narrow-range check should detect the
    // conflict (file deleted vs client modified) and fail.
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

    init_just_knobs_for_merge_test();

    let base_content = "line1\nline2\nline3\nline4\nline5\n";
    let base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file.txt", base_content)
        .commit()
        .await?;

    // S1: modify first line (resolvable conflict with client)
    let s1_content = "modified_line1\nline2\nline3\nline4\nline5\n";
    let s1 = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", s1_content)
        .commit()
        .await?;

    // S2: delete file.txt
    let s2 = CreateCommitContext::new(&ctx, &repo, vec![s1])
        .delete_file("file.txt")
        .commit()
        .await?;

    // Set bookmark to S2
    let book = BookmarkKey::new("master")?;
    let hg_s2 = repo.derive_hg_changeset(&ctx, s2).await?;
    set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_s2}")).await?;

    // Client: modify last line (based on base)
    let client_content = "line1\nline2\nline3\nline4\nmodified_line5\n";
    let client = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", client_content)
        .commit()
        .await?;

    let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;
    let client_cf = find_changed_files(&ctx, &repo, base, client).await?;

    // Attempt 1: base → S1 (detects conflict, produces MergedFileInfo)
    let result1 = check_pushrebase_conflicts(
        &ctx,
        &repo,
        &Default::default(),
        base,
        base,
        s1,
        std::slice::from_ref(&client_bcs),
        &client_cf,
    )
    .await?;
    assert!(
        result1.merged_file_overrides.is_some(),
        "Attempt 1 should resolve the conflict"
    );

    // Attempt 2: S1 → S2 (file deleted — should fail with conflict)
    let result2 = check_pushrebase_conflicts(
        &ctx,
        &repo,
        &Default::default(),
        base,
        s1,
        s2,
        std::slice::from_ref(&client_bcs),
        &client_cf,
    )
    .await;

    // File was deleted on server but client modifies it — irreconcilable
    match result2 {
        Err(PushrebaseError::Conflicts(_)) => { /* expected */ }
        Err(e) => {
            panic!("Expected Conflicts error for file deleted on server, got error: {e}",)
        }
        Ok(_) => panic!("Expected Conflicts error for file deleted on server, but got Ok",),
    }

    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_merge_resolution_no_conflict_in_delta(fb: FacebookInit) -> Result<(), Error> {
    // Test: When merge resolution succeeds on attempt 1 and a subsequent
    // unrelated server commit moves the bookmark, the carried MergedFileInfo
    // is correctly used on retry (no conflict in the delta range).
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

    init_just_knobs_for_merge_test();

    let base_content = "line1\nline2\nline3\nline4\nline5\n";
    let base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file.txt", base_content)
        .commit()
        .await?;

    // S1: modify first line
    let s1_content = "modified_line1\nline2\nline3\nline4\nline5\n";
    let s1 = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", s1_content)
        .commit()
        .await?;

    // S2: unrelated change
    let s2 = CreateCommitContext::new(&ctx, &repo, vec![s1])
        .add_file("other.txt", "unrelated\n")
        .commit()
        .await?;

    let book = BookmarkKey::new("master")?;
    let hg_s2 = repo.derive_hg_changeset(&ctx, s2).await?;
    set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_s2}")).await?;

    // Client: modify last line
    let client_content = "line1\nline2\nline3\nline4\nmodified_line5\n";
    let client = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", client_content)
        .commit()
        .await?;

    let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;
    let client_cf = find_changed_files(&ctx, &repo, base, client).await?;

    // Attempt 1: base → S1
    let result1 = check_pushrebase_conflicts(
        &ctx,
        &repo,
        &Default::default(),
        base,
        base,
        s1,
        std::slice::from_ref(&client_bcs),
        &client_cf,
    )
    .await?;
    assert!(
        result1.merged_file_overrides.is_some(),
        "Attempt 1 should detect and resolve the conflict"
    );
    let carried = result1.merged_file_overrides.unwrap();

    // Attempt 2: S1 → S2 (no conflict in delta)
    let result2 = check_pushrebase_conflicts(
        &ctx,
        &repo,
        &Default::default(),
        base,
        s1,
        s2,
        std::slice::from_ref(&client_bcs),
        &client_cf,
    )
    .await?;
    assert!(
        result2.merged_file_overrides.is_none(),
        "No conflicts in narrow range S1→S2"
    );

    // Reconcile: carried info used as-is (no delta)
    let reconciled = carried;

    // Rebase with carried overrides
    let (new_head, _, rebased_bonsais) = create_rebased_changesets(
        &ctx,
        &repo,
        &Default::default(),
        find_rebased_set(&ctx, &repo, base, client).await?,
        base,
        client,
        s2,
        &mut [],
        Some(reconciled),
    )
    .await?;
    changesets_creation::save_changesets(&ctx, &repo, rebased_bonsais).await?;

    // Verify merged content
    let result_hg = repo.derive_hg_changeset(&ctx, new_head).await?;
    let result_cs = result_hg.load(&ctx, repo.repo_blobstore()).await?;
    let manifest = result_cs.manifestid();
    let file_path = NonRootMPath::new("file.txt")?;
    let file_entry = manifest
        .find_entry(ctx.clone(), repo.repo_blobstore().clone(), file_path.into())
        .await?
        .expect("file.txt should exist");

    let file_content = match file_entry {
        Entry::Leaf((_, filenode_id)) => {
            let content_id = filenode_id
                .load(&ctx, repo.repo_blobstore())
                .await?
                .content_id();
            let bytes = filestore::fetch_concat(repo.repo_blobstore(), &ctx, content_id).await?;
            String::from_utf8(bytes.to_vec())?
        }
        _ => panic!("file.txt should be a file"),
    };

    assert!(
        file_content.contains("modified_line1"),
        "should have server's change"
    );
    assert!(
        file_content.contains("modified_line5"),
        "should have client's change"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn batched_pushrebase_merge_resolution(fb: FacebookInit) -> Result<(), Error> {
    // Test: batched pushrebase resolves merge conflicts when server and
    // client modify different parts of the same file.
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

    init_just_knobs_for_merge_test();

    let base_content = "line1\nline2\nline3\nline4\nline5\n";
    let base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file.txt", base_content)
        .commit()
        .await?;

    // Server commit: modify first line
    let server_content = "modified_line1\nline2\nline3\nline4\nline5\n";
    let server = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", server_content)
        .commit()
        .await?;

    // Set bookmark to server commit
    let bookmark = BookmarkKey::new("master")?;
    let hg_server = repo.derive_hg_changeset(&ctx, server).await?;
    set_bookmark(ctx.clone(), &repo, &bookmark, &format!("{hg_server}")).await?;

    // Client: modify last line (based on base)
    let client_content = "line1\nline2\nline3\nline4\nmodified_line5\n";
    let client_cs_id = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", client_content)
        .commit()
        .await?;

    let client_bcs = client_cs_id.load(&ctx, repo.repo_blobstore()).await?;
    let config = PushrebaseFlags::default();
    let idx =
        index_pushrebase_request(&ctx, &repo, &config, &bookmark, &hashset![client_bcs]).await?;

    let (tx, rx) = oneshot::channel();
    let request = PushrebaseRequest {
        changed_files: idx.changed_files,
        changesets: idx.changesets,
        head: idx.head,
        root: idx.root,
        conflict_check_base: idx.root,
        carried_merge_file_info: vec![],
        retry_num: PushrebaseRetryNum(0),
        hooks: vec![],
        response_tx: tx,
    };

    let requeued = do_batched_pushrebase(&ctx, &repo, &config, &bookmark, vec![request]).await;
    assert!(requeued.is_empty(), "Should not be requeued");

    let outcome = rx.await.unwrap().unwrap();
    assert!(
        outcome.merge_resolved_paths.is_some(),
        "Should report merge resolved paths"
    );
    let resolved_paths = outcome.merge_resolved_paths.unwrap();
    assert_eq!(resolved_paths.len(), 1);
    assert_eq!(resolved_paths[0], NonRootMPath::new("file.txt")?);

    // Verify merged content
    let result_hg = repo.derive_hg_changeset(&ctx, outcome.head).await?;
    let result_cs = result_hg.load(&ctx, repo.repo_blobstore()).await?;
    let manifest = result_cs.manifestid();
    let file_path = NonRootMPath::new("file.txt")?;
    let file_entry = manifest
        .find_entry(ctx.clone(), repo.repo_blobstore().clone(), file_path.into())
        .await?
        .expect("file.txt should exist");

    let file_content = match file_entry {
        Entry::Leaf((_, filenode_id)) => {
            let content_id = filenode_id
                .load(&ctx, repo.repo_blobstore())
                .await?
                .content_id();
            let bytes = filestore::fetch_concat(repo.repo_blobstore(), &ctx, content_id).await?;
            String::from_utf8(bytes.to_vec())?
        }
        _ => panic!("file.txt should be a file"),
    };

    assert!(
        file_content.contains("modified_line1"),
        "should have server's change"
    );
    assert!(
        file_content.contains("modified_line5"),
        "should have client's change"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn batched_pushrebase_merge_resolution_carry_forward(fb: FacebookInit) -> Result<(), Error> {
    // Test: when batched pushrebase is re-queued after CAS failure,
    // carried_merge_file_info is preserved and reconciled on retry.
    // We simulate this by setting conflict_check_base to S1 and
    // providing carried MergedFileInfo from a prior attempt.
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

    init_just_knobs_for_merge_test();

    let base_content = "line1\nline2\nline3\nline4\nline5\n";
    let base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file.txt", base_content)
        .commit()
        .await?;

    // S1: modify first line (conflict with client)
    let s1_content = "modified_line1\nline2\nline3\nline4\nline5\n";
    let s1 = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", s1_content)
        .commit()
        .await?;

    // S2: unrelated change (no conflict in delta S1→S2)
    let s2 = CreateCommitContext::new(&ctx, &repo, vec![s1])
        .add_file("other.txt", "unrelated\n")
        .commit()
        .await?;

    let bookmark = BookmarkKey::new("master")?;
    let hg_s2 = repo.derive_hg_changeset(&ctx, s2).await?;
    set_bookmark(ctx.clone(), &repo, &bookmark, &format!("{hg_s2}")).await?;

    // Client: modify last line
    let client_content = "line1\nline2\nline3\nline4\nmodified_line5\n";
    let client_cs_id = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", client_content)
        .commit()
        .await?;

    let client_bcs = client_cs_id.load(&ctx, repo.repo_blobstore()).await?;
    let config = PushrebaseFlags::default();
    let idx =
        index_pushrebase_request(&ctx, &repo, &config, &bookmark, &hashset![client_bcs]).await?;

    // First, get MergedFileInfo from a base→S1 check (simulating attempt 1)
    let result1 = check_pushrebase_conflicts(
        &ctx,
        &repo,
        &Default::default(),
        base,
        base,
        s1,
        std::slice::from_ref(idx.changesets.first().unwrap()),
        &idx.changed_files,
    )
    .await?;
    let carried = result1
        .merged_file_overrides
        .expect("Should have overrides from attempt 1");

    // Now simulate a retry: conflict_check_base = S1, carried info from attempt 1
    let (tx, rx) = oneshot::channel();
    let request = PushrebaseRequest {
        changed_files: idx.changed_files,
        changesets: idx.changesets,
        head: idx.head,
        root: idx.root,
        conflict_check_base: s1,
        carried_merge_file_info: carried,
        retry_num: PushrebaseRetryNum(1),
        hooks: vec![],
        response_tx: tx,
    };

    let requeued = do_batched_pushrebase(&ctx, &repo, &config, &bookmark, vec![request]).await;
    assert!(requeued.is_empty(), "Should not be requeued");

    let outcome = rx.await.unwrap().unwrap();
    assert!(
        outcome.merge_resolved_paths.is_some(),
        "Should report merge resolved paths via carry-forward"
    );

    // Verify merged content
    let result_hg = repo.derive_hg_changeset(&ctx, outcome.head).await?;
    let result_cs = result_hg.load(&ctx, repo.repo_blobstore()).await?;
    let manifest = result_cs.manifestid();
    let file_path = NonRootMPath::new("file.txt")?;
    let file_entry = manifest
        .find_entry(ctx.clone(), repo.repo_blobstore().clone(), file_path.into())
        .await?
        .expect("file.txt should exist");

    let file_content = match file_entry {
        Entry::Leaf((_, filenode_id)) => {
            let content_id = filenode_id
                .load(&ctx, repo.repo_blobstore())
                .await?
                .content_id();
            let bytes = filestore::fetch_concat(repo.repo_blobstore(), &ctx, content_id).await?;
            String::from_utf8(bytes.to_vec())?
        }
        _ => panic!("file.txt should be a file"),
    };

    assert!(
        file_content.contains("modified_line1"),
        "should have server's change via carry-forward"
    );
    assert!(
        file_content.contains("modified_line5"),
        "should have client's change"
    );

    Ok(())
}

fn init_just_knobs_for_noop_rejection_test(reject: bool) {
    override_just_knobs(JustKnobsInMemory::new(hashmap! {
        "scm/mononoke:pushrebase_enable_merge_resolution".to_string() => KnobVal::Bool(true),
        "scm/mononoke:pushrebase_merge_resolution_derive_fsnodes".to_string() => KnobVal::Bool(true),
        "scm/mononoke:pushrebase_reject_noop_merge_commits".to_string() => KnobVal::Bool(reject),
        "scm/mononoke:pushrebase_range_diff_use_content_manifests".to_string() => KnobVal::Bool(false),
    }));
}

#[mononoke::fbinit_test]
async fn pushrebase_noop_merge_detected_only_when_jk_off(fb: FacebookInit) -> Result<(), Error> {
    // JK off: client wrote identical content to server. The commit should
    // still land as a no-op (current Phase 1 dry-run behavior). Detection
    // is logged but no rejection happens.
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

    let base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file.txt", "line1\nline2\nline3\n")
        .commit()
        .await?;

    let shared_content = "line1\nSHARED_EDIT\nline3\n";
    let server = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", shared_content)
        .commit()
        .await?;

    let book = BookmarkKey::new("master")?;
    let hg_server = repo.derive_hg_changeset(&ctx, server).await?;
    set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_server}")).await?;

    // Client wrote IDENTICAL content (same `shared_content`) — duplicate change.
    let client = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", shared_content)
        .commit()
        .await?;
    let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;

    init_just_knobs_for_noop_rejection_test(false);

    // Should succeed (JK off → detection only, no rejection).
    let result = do_pushrebase_bonsai(
        &ctx,
        &repo,
        &Default::default(),
        &book,
        &hashset![client_bcs],
        &[],
    )
    .await?;

    assert!(result.rebased_changesets.len() == 1, "one commit rebased");
    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_noop_merge_rejected_single_file(fb: FacebookInit) -> Result<(), Error> {
    // JK on: single-file duplicate-content commit → rejected with Conflicts
    // on that path, matching pre-merge-resolution behavior.
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

    let base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file.txt", "line1\nline2\nline3\n")
        .commit()
        .await?;

    let shared_content = "line1\nSHARED_EDIT\nline3\n";
    let server = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", shared_content)
        .commit()
        .await?;

    let book = BookmarkKey::new("master")?;
    let hg_server = repo.derive_hg_changeset(&ctx, server).await?;
    set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_server}")).await?;

    let client = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", shared_content)
        .commit()
        .await?;
    let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;

    init_just_knobs_for_noop_rejection_test(true);

    let result = do_pushrebase_bonsai(
        &ctx,
        &repo,
        &Default::default(),
        &book,
        &hashset![client_bcs],
        &[],
    )
    .await;

    match result {
        Err(PushrebaseError::Conflicts(conflicts)) => {
            assert_eq!(
                conflicts.len(),
                1,
                "Should report one conflict for the single duplicate path"
            );
            let path_str = format!("{}", conflicts[0].left);
            assert_eq!(
                path_str, "file.txt",
                "Conflict should name the duplicate file"
            );
        }
        other => panic!("Expected Conflicts error, got: {other:?}"),
    }
    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_noop_merge_mixed_commit_lands(fb: FacebookInit) -> Result<(), Error> {
    // NEGATIVE test: commit has both a duplicate-content file AND a
    // non-conflicting real change. The commit has a real net change so
    // it must NOT be flagged as no-op — should land normally.
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

    // Base has only the file that will conflict
    let base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("dup.txt", "line1\nline2\nline3\n")
        .commit()
        .await?;

    let shared_content = "line1\nSHARED_EDIT\nline3\n";
    let server = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("dup.txt", shared_content)
        .commit()
        .await?;

    let book = BookmarkKey::new("master")?;
    let hg_server = repo.derive_hg_changeset(&ctx, server).await?;
    set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_server}")).await?;

    // Client: duplicate edit to dup.txt + real new file `new.txt`
    let client = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("dup.txt", shared_content)
        .add_file("new.txt", "brand new content\n")
        .commit()
        .await?;
    let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;

    init_just_knobs_for_noop_rejection_test(true);

    // Should succeed despite the duplicate file_change because new.txt
    // is a real change.
    let result = do_pushrebase_bonsai(
        &ctx,
        &repo,
        &Default::default(),
        &book,
        &hashset![client_bcs],
        &[],
    )
    .await?;

    assert_eq!(
        result.rebased_changesets.len(),
        1,
        "One commit should be rebased — mixed commit must not be rejected"
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_noop_merge_in_stack_rejects_entire_stack(
    fb: FacebookInit,
) -> Result<(), Error> {
    // JK on: 2-commit stack where the FIRST commit becomes a no-op (its
    // only file_change is a duplicate). The entire stack should be
    // rejected, not just the no-op commit.
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

    let base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("dup.txt", "line1\nline2\nline3\n")
        .commit()
        .await?;

    let shared_content = "line1\nSHARED_EDIT\nline3\n";
    let server = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("dup.txt", shared_content)
        .commit()
        .await?;

    let book = BookmarkKey::new("master")?;
    let hg_server = repo.derive_hg_changeset(&ctx, server).await?;
    set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_server}")).await?;

    // Client commit 1: duplicate edit to dup.txt only — would become no-op
    let c1 = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("dup.txt", shared_content)
        .commit()
        .await?;
    // Client commit 2 (HEAD): real change to a different file
    let c2 = CreateCommitContext::new(&ctx, &repo, vec![c1])
        .add_file("other.txt", "real content\n")
        .commit()
        .await?;
    let c1_bcs = c1.load(&ctx, repo.repo_blobstore()).await?;
    let c2_bcs = c2.load(&ctx, repo.repo_blobstore()).await?;

    init_just_knobs_for_noop_rejection_test(true);

    let result = do_pushrebase_bonsai(
        &ctx,
        &repo,
        &Default::default(),
        &book,
        &hashset![c1_bcs, c2_bcs],
        &[],
    )
    .await;

    match result {
        Err(PushrebaseError::Conflicts(conflicts)) => {
            let path_strs: HashSet<String> =
                conflicts.iter().map(|c| format!("{}", c.left)).collect();
            assert!(
                path_strs.contains("dup.txt"),
                "Conflicts should include dup.txt from the no-op c1"
            );
        }
        other => panic!("Expected Conflicts error rejecting the entire stack, got: {other:?}"),
    }
    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_noop_merge_rejected_all_files_duplicate(fb: FacebookInit) -> Result<(), Error> {
    // JK on: client commit touches two files (A and B); server made the
    // identical edits to both. Every file_change is a duplicate, so the
    // commit becomes a no-op and must be rejected with conflicts on both
    // paths.
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

    let base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("a.txt", "line1\nline2\nline3\n")
        .add_file("b.txt", "alpha\nbeta\ngamma\n")
        .commit()
        .await?;

    let shared_a = "line1\nA_EDIT\nline3\n";
    let shared_b = "alpha\nB_EDIT\ngamma\n";
    let server = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("a.txt", shared_a)
        .add_file("b.txt", shared_b)
        .commit()
        .await?;

    let book = BookmarkKey::new("master")?;
    let hg_server = repo.derive_hg_changeset(&ctx, server).await?;
    set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_server}")).await?;

    // Client wrote IDENTICAL content to both files.
    let client = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("a.txt", shared_a)
        .add_file("b.txt", shared_b)
        .commit()
        .await?;
    let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;

    init_just_knobs_for_noop_rejection_test(true);

    let result = do_pushrebase_bonsai(
        &ctx,
        &repo,
        &Default::default(),
        &book,
        &hashset![client_bcs],
        &[],
    )
    .await;

    match result {
        Err(PushrebaseError::Conflicts(conflicts)) => {
            let path_strs: HashSet<String> =
                conflicts.iter().map(|c| format!("{}", c.left)).collect();
            assert!(
                path_strs.contains("a.txt") && path_strs.contains("b.txt"),
                "Conflicts must include both duplicate paths, got: {path_strs:?}"
            );
        }
        other => panic!("Expected Conflicts error, got: {other:?}"),
    }
    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_noop_merge_genuine_merge_not_flagged(fb: FacebookInit) -> Result<(), Error> {
    // NEGATIVE test: client and server edited DIFFERENT lines of the same
    // file. local_content_id != other_id, so our new check is not
    // triggered. Cascading merge resolves cleanly, override ≠ other, the
    // commit makes a real net change, and is not classified as no-op even
    // when the JK is on.
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

    let base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file.txt", "line1\nline2\nline3\n")
        .commit()
        .await?;

    // Server edits line 3 only.
    let server_content = "line1\nline2\nSERVER_EDIT\n";
    let server = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", server_content)
        .commit()
        .await?;

    let book = BookmarkKey::new("master")?;
    let hg_server = repo.derive_hg_changeset(&ctx, server).await?;
    set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_server}")).await?;

    // Client edits line 1 only — non-overlapping with server.
    let client_content = "CLIENT_EDIT\nline2\nline3\n";
    let client = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", client_content)
        .commit()
        .await?;
    let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;

    init_just_knobs_for_noop_rejection_test(true);

    // Should succeed — genuine 3-way merge, no duplicate content detected.
    let result = do_pushrebase_bonsai(
        &ctx,
        &repo,
        &Default::default(),
        &book,
        &hashset![client_bcs],
        &[],
    )
    .await?;

    assert_eq!(
        result.rebased_changesets.len(),
        1,
        "Genuine merge must land — no duplicate paths detected"
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_noop_merge_local_equals_base_not_flagged(
    fb: FacebookInit,
) -> Result<(), Error> {
    // NEGATIVE test: server edited file.txt away from base then edited it
    // BACK to the original base content. So path is in merge_paths
    // (server touched it), but base_id == other_id (server's net change
    // is zero). Client's edit goes through the existing
    // `base_id == other_id` short-circuit, NOT our new
    // `local_content_id == other_id` check, so duplicate_paths stays
    // empty and the commit is not flagged as no-op.
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

    let base_content = "line1\nline2\nline3\n";
    let base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file.txt", base_content)
        .commit()
        .await?;

    // Server commit 1: edit away from base.
    let server1 = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", "line1\nSERVER_INTERMEDIATE\nline3\n")
        .commit()
        .await?;

    // Server commit 2: revert back to base content. base_id == other_id
    // for this path now (root and current_master agree on content).
    let server2 = CreateCommitContext::new(&ctx, &repo, vec![server1])
        .add_file("file.txt", base_content)
        .commit()
        .await?;

    let book = BookmarkKey::new("master")?;
    let hg_server2 = repo.derive_hg_changeset(&ctx, server2).await?;
    set_bookmark(ctx.clone(), &repo, &book, &format!("{hg_server2}")).await?;

    // Client edits file.txt to brand-new content.
    let client = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("file.txt", "line1\nCLIENT_EDIT\nline3\n")
        .commit()
        .await?;
    let client_bcs = client.load(&ctx, repo.repo_blobstore()).await?;

    init_just_knobs_for_noop_rejection_test(true);

    // Should succeed — base==other path is hit, no duplicate detected.
    let result = do_pushrebase_bonsai(
        &ctx,
        &repo,
        &Default::default(),
        &book,
        &hashset![client_bcs],
        &[],
    )
    .await?;

    assert_eq!(
        result.rebased_changesets.len(),
        1,
        "base==other branch must not trip our duplicate-content detection"
    );
    Ok(())
}

fn pessimistic_config() -> PushrebaseFlags {
    PushrebaseFlags {
        pessimistic_locking_bookmarks: vec![master_bookmark()],
        ..Default::default()
    }
}

fn init_just_knobs_for_pessimistic_test() {
    override_just_knobs(JustKnobsInMemory::new(hashmap! {
        "scm/mononoke:pushrebase_enable_merge_resolution".to_string() => KnobVal::Bool(false),
        "scm/mononoke:pushrebase_merge_resolution_derive_fsnodes".to_string() => KnobVal::Bool(true),
        "scm/mononoke:per_bookmark_locking".to_string() => KnobVal::Bool(true),
        "scm/mononoke:pushrebase_range_diff_use_content_manifests".to_string() => KnobVal::Bool(false),
    }));
}

// NOTE: Full end-to-end pessimistic pushrebase cannot be tested with
// SQLite unit tests because TestRepoFactory shares a single SQLite
// connection across all facets. LockedBookmarkTransaction holds the
// connection open during rebase, and save_changesets ->
// CommitGraphWriter::add_many tries to acquire the same connection,
// causing a deadlock. Full E2E is covered by integration tests (MySQL).

#[mononoke::fbinit_test]
async fn pessimistic_pushrebase_conflict(fb: FacebookInit) -> Result<(), Error> {
    init_just_knobs_for_pessimistic_test();
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;

    let book = master_bookmark();
    bookmark(&ctx, &repo, book.clone())
        .set_to("a5ffa77602a066db7d5cfb9fb5823a0895717c5a")
        .await?;

    let root = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
    let p = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(&ctx, root)
        .await?
        .ok_or_else(|| Error::msg("Root is missing"))?;

    let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![p])
        .add_file("files", "conflicting content")
        .commit()
        .await?;
    let bcs = bcs_id.load(&ctx, repo.repo_blobstore()).await?;

    let config = pessimistic_config();
    let result = do_pushrebase_bonsai(&ctx, &repo, &config, &book, &hashset![bcs], &[]).await;

    should_have_conflicts(result);
    Ok(())
}

#[mononoke::fbinit_test]
async fn pessimistic_dispatch_selection(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = Linear::get_repo(fb).await;

    let book = master_bookmark();
    let other_book = BookmarkKey::new("other_bookmark")?;
    let config = pessimistic_config();
    let repo_id_str = repo.repo_identity().id().to_string();

    init_just_knobs_for_test();
    let use_pessimistic = justknobs::eval(
        "scm/mononoke:per_bookmark_locking",
        None,
        Some(&repo_id_str),
    ) && config.pessimistic_locking_bookmarks.contains(&book);
    assert!(!use_pessimistic, "should be optimistic when knob is off");

    init_just_knobs_for_pessimistic_test();
    let use_pessimistic = justknobs::eval(
        "scm/mononoke:per_bookmark_locking",
        None,
        Some(&repo_id_str),
    ) && config.pessimistic_locking_bookmarks.contains(&other_book);
    assert!(
        !use_pessimistic,
        "should be optimistic when bookmark not in pessimistic list"
    );

    let use_pessimistic = justknobs::eval(
        "scm/mononoke:per_bookmark_locking",
        None,
        Some(&repo_id_str),
    ) && config.pessimistic_locking_bookmarks.contains(&book);
    assert!(
        use_pessimistic,
        "should be pessimistic when knob is on and bookmark is in list"
    );

    let config_empty = PushrebaseFlags::default();
    let use_pessimistic = justknobs::eval(
        "scm/mononoke:per_bookmark_locking",
        None,
        Some(&repo_id_str),
    ) && config_empty.pessimistic_locking_bookmarks.contains(&book);
    assert!(
        !use_pessimistic,
        "should be optimistic with empty pessimistic_locking_bookmarks"
    );

    drop(ctx);
    Ok(())
}

#[mononoke::fbinit_test]
async fn pessimistic_locked_transaction_lifecycle(fb: FacebookInit) -> Result<(), Error> {
    init_just_knobs_for_pessimistic_test();
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;

    let book = master_bookmark();

    let root_cs = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file", "content")
        .commit()
        .await?;

    bookmark(&ctx, &repo, book.clone()).set_to(root_cs).await?;

    let child_cs = CreateCommitContext::new(&ctx, &repo, vec![root_cs])
        .add_file("file2", "content2")
        .commit()
        .await?;

    let sql_bookmarks = repo.sql_bookmarks();
    let locked_txn = sql_bookmarks.start_locked_transaction(&ctx, &book).await?;

    assert_eq!(locked_txn.current_value(), Some(root_cs));

    let log_id = locked_txn
        .commit(&ctx, child_cs, BookmarkUpdateReason::Pushrebase, vec![])
        .await?;

    assert!(log_id.is_some(), "CAS should succeed under the lock");

    let new_value = repo
        .bookmarks()
        .get(ctx.clone(), &book, bookmarks::Freshness::MostRecent)
        .await?;
    assert_eq!(new_value, Some(child_cs));

    Ok(())
}

// Bookmark points at a server commit that edited line 1; client commit
// edits line 5. Path-level conflict that MR can resolve, vanilla can't.
async fn setup_non_overlapping_conflict(
    ctx: &CoreContext,
    repo: &PushrebaseTestRepo,
) -> Result<(BonsaiChangeset, BookmarkKey), Error> {
    let base_content = "line1\nline2\nline3\nline4\nline5\n";
    let base = CreateCommitContext::new_root(ctx, repo)
        .add_file("file.txt", base_content)
        .commit()
        .await?;

    let server_content = "modified_line1\nline2\nline3\nline4\nline5\n";
    let server = CreateCommitContext::new(ctx, repo, vec![base])
        .add_file("file.txt", server_content)
        .commit()
        .await?;

    let book = BookmarkKey::new("master")?;
    let hg_server = repo.derive_hg_changeset(ctx, server).await?;
    set_bookmark(ctx.clone(), repo, &book, &format!("{hg_server}")).await?;

    let client_content = "line1\nline2\nline3\nline4\nmodified_line5\n";
    let client = CreateCommitContext::new(ctx, repo, vec![base])
        .add_file("file.txt", client_content)
        .commit()
        .await?;
    let client_bcs = client.load(ctx, repo.repo_blobstore()).await?;

    Ok((client_bcs, book))
}

#[mononoke::fbinit_test]
async fn pushrebase_merge_resolution_override_forces_off(fb: FacebookInit) -> Result<(), Error> {
    // JK on, override off → must conflict.
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;
    let (client_bcs, book) = setup_non_overlapping_conflict(&ctx, &repo).await?;

    init_just_knobs_for_merge_test();

    let config = PushrebaseFlags {
        merge_resolution_override: MergeResolutionOverride::ForceOff,
        ..Default::default()
    };

    let result =
        do_pushrebase_bonsai(&ctx, &repo, &config, &book, &hashset![client_bcs], &[]).await;

    should_have_conflicts(result);
    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_merge_resolution_override_forces_on(fb: FacebookInit) -> Result<(), Error> {
    // JK off, override on → must merge cleanly.
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;
    let (client_bcs, book) = setup_non_overlapping_conflict(&ctx, &repo).await?;

    override_just_knobs(JustKnobsInMemory::new(hashmap! {
        "scm/mononoke:pushrebase_enable_merge_resolution".to_string() => KnobVal::Bool(false),
        "scm/mononoke:pushrebase_merge_resolution_derive_fsnodes".to_string() => KnobVal::Bool(true),
        "scm/mononoke:pushrebase_range_diff_use_content_manifests".to_string() => KnobVal::Bool(false),
    }));

    let config = PushrebaseFlags {
        merge_resolution_override: MergeResolutionOverride::ForceOn,
        ..Default::default()
    };

    let result =
        do_pushrebase_bonsai(&ctx, &repo, &config, &book, &hashset![client_bcs], &[]).await?;

    let result_hg = repo.derive_hg_changeset(&ctx, result.head).await?;
    let expected_content = "modified_line1\nline2\nline3\nline4\nmodified_line5\n";
    ensure_content(
        &ctx,
        result_hg,
        &repo,
        btreemap! {
            "file.txt".to_string() => expected_content.to_string(),
        },
    )
    .await?;
    Ok(())
}

#[mononoke::fbinit_test]
async fn pushrebase_merge_resolution_override_none_falls_through(
    fb: FacebookInit,
) -> Result<(), Error> {
    // No override, JK off → must conflict (historical path).
    let ctx = CoreContext::test_mock(fb);
    let repo: PushrebaseTestRepo = test_repo_factory::build_empty(fb).await?;
    let (client_bcs, book) = setup_non_overlapping_conflict(&ctx, &repo).await?;

    override_just_knobs(JustKnobsInMemory::new(hashmap! {
        "scm/mononoke:pushrebase_enable_merge_resolution".to_string() => KnobVal::Bool(false),
        "scm/mononoke:pushrebase_merge_resolution_derive_fsnodes".to_string() => KnobVal::Bool(true),
        "scm/mononoke:pushrebase_range_diff_use_content_manifests".to_string() => KnobVal::Bool(false),
    }));

    let config = PushrebaseFlags {
        merge_resolution_override: MergeResolutionOverride::UseJk,
        ..Default::default()
    };

    let result =
        do_pushrebase_bonsai(&ctx, &repo, &config, &book, &hashset![client_bcs], &[]).await;

    should_have_conflicts(result);
    Ok(())
}
