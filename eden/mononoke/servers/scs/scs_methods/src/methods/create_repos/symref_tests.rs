/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use fbinit::FacebookInit;
use futures::FutureExt;
use git_source_of_truth::SqlGitSourceOfTruthConfigBuilder;
use git_symbolic_refs::RefType;
use git_symbolic_refs::SqlGitSymbolicRefsBuilder;
use justknobs::test_helpers::JustKnobsInMemory;
use justknobs::test_helpers::KnobVal;
use justknobs::test_helpers::with_just_knobs_async;
use mononoke_macros::mononoke;
use sql_construct::SqlConstruct;

use super::*;

struct TestSymrefStoreProvider {
    stores: HashMap<RepositoryId, Arc<dyn GitSymbolicRefs>>,
}

#[async_trait::async_trait]
impl SymrefStoreProvider for TestSymrefStoreProvider {
    async fn repo_store(
        &self,
        repo_id: RepositoryId,
    ) -> Result<Arc<dyn GitSymbolicRefs>, scs_errors::ServiceError> {
        self.stores.get(&repo_id).cloned().ok_or_else(|| {
            scs_errors::internal_error(format!("no test symref store for repo {repo_id}")).into()
        })
    }
}

struct FailingSymrefStore {
    repo_id: RepositoryId,
}

#[async_trait::async_trait]
impl GitSymbolicRefs for FailingSymrefStore {
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn get_ref_by_symref(
        &self,
        _ctx: &CoreContext,
        _symref: String,
    ) -> Result<Option<GitSymbolicRefsEntry>> {
        anyhow::bail!("FailingSymrefStore always fails")
    }

    async fn get_symrefs_by_ref(
        &self,
        _ctx: &CoreContext,
        _ref_name: String,
        _ref_type: RefType,
    ) -> Result<Option<Vec<String>>> {
        anyhow::bail!("FailingSymrefStore always fails")
    }

    async fn add_or_update_entries(
        &self,
        _ctx: &CoreContext,
        _entries: Vec<GitSymbolicRefsEntry>,
    ) -> Result<()> {
        anyhow::bail!("FailingSymrefStore always fails")
    }

    async fn delete_symrefs(&self, _ctx: &CoreContext, _symrefs: Vec<String>) -> Result<()> {
        anyhow::bail!("FailingSymrefStore always fails")
    }

    async fn list_all_symrefs(&self, _ctx: &CoreContext) -> Result<Vec<GitSymbolicRefsEntry>> {
        anyhow::bail!("FailingSymrefStore always fails")
    }
}

fn sqlite_store(repo_id: RepositoryId) -> Result<Arc<dyn GitSymbolicRefs>> {
    Ok(Arc::new(
        SqlGitSymbolicRefsBuilder::with_sqlite_in_memory()?.build(repo_id),
    ))
}

fn request_with_branch(name: &str, branch: Option<&str>) -> thrift::RepoCreationRequest {
    thrift::RepoCreationRequest {
        repo_name: name.to_string(),
        scm_type: thrift::RepoScmType::GIT,
        size_bucket: thrift::RepoSizeBucket::SMALL,
        default_branch: branch.map(str::to_string),
        ..Default::default()
    }
}

fn requests_of(
    repo_ids_and_requests: &[(RepositoryId, thrift::RepoCreationRequest)],
) -> Vec<thrift::RepoCreationRequest> {
    repo_ids_and_requests
        .iter()
        .map(|(_id, request)| request.clone())
        .collect()
}

async fn head_branch(
    ctx: &CoreContext,
    store: &Arc<dyn GitSymbolicRefs>,
) -> Result<Option<String>> {
    Ok(store
        .get_ref_by_symref(ctx, HEAD_SYMREF.to_string())
        .await?
        .map(|entry| entry.ref_name))
}

fn symref_jk(enabled: bool) -> JustKnobsInMemory {
    JustKnobsInMemory::new(HashMap::from([(
        WRITE_DEFAULT_BRANCH_SYMREF_JK.to_string(),
        KnobVal::Bool(enabled),
    )]))
}

#[mononoke::fbinit_test]
async fn writes_head_only_for_requests_with_default_branch(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let store_with = sqlite_store(RepositoryId::new(1))?;
    let store_without = sqlite_store(RepositoryId::new(2))?;
    let stores = TestSymrefStoreProvider {
        stores: HashMap::from([
            (RepositoryId::new(1), store_with.clone()),
            (RepositoryId::new(2), store_without.clone()),
        ]),
    };
    let repo_ids_and_requests = vec![
        (
            RepositoryId::new(1),
            request_with_branch("repo/with", Some("main")),
        ),
        (
            RepositoryId::new(2),
            request_with_branch("repo/without", None),
        ),
    ];

    with_just_knobs_async(
        symref_jk(true),
        async {
            let enabled = validate_default_branches(&requests_of(&repo_ids_and_requests))
                .expect("valid default branches must pass validation");
            assert!(
                enabled,
                "JK on + a set default_branch must enable symref writes",
            );
            let written =
                write_default_branch_symrefs(&ctx, &stores, &repo_ids_and_requests, enabled)
                    .await
                    .expect("writing symrefs for a mixed batch should succeed");
            assert_eq!(
                written,
                vec![RepositoryId::new(1)],
                "only the repo with default_branch should be reported as written",
            );
            anyhow::Ok(())
        }
        .boxed(),
    )
    .await?;

    assert_eq!(
        head_branch(&ctx, &store_with).await?,
        Some("main".to_string()),
        "the repo with default_branch must get a HEAD row",
    );
    assert_eq!(
        head_branch(&ctx, &store_without).await?,
        None,
        "the repo without default_branch must NOT get a HEAD row",
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn jk_off_writes_nothing(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let store = sqlite_store(RepositoryId::new(1))?;
    let stores = TestSymrefStoreProvider {
        stores: HashMap::from([(RepositoryId::new(1), store.clone())]),
    };
    let repo_ids_and_requests = vec![(
        RepositoryId::new(1),
        request_with_branch("repo/with", Some("main")),
    )];

    with_just_knobs_async(
        symref_jk(false),
        async {
            let enabled = validate_default_branches(&requests_of(&repo_ids_and_requests))
                .expect("the gated-off path must not reject the request");
            assert!(!enabled, "writes must be disabled with the kill switch off");
            let written =
                write_default_branch_symrefs(&ctx, &stores, &repo_ids_and_requests, enabled)
                    .await
                    .expect("the gated-off path should succeed as a no-op");
            assert!(
                written.is_empty(),
                "nothing should be reported as written with the kill switch off",
            );
            anyhow::Ok(())
        }
        .boxed(),
    )
    .await?;

    assert_eq!(
        head_branch(&ctx, &store).await?,
        None,
        "no HEAD row should be written with the kill switch off",
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn jk_off_invalid_branch_is_fully_inert(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let store = sqlite_store(RepositoryId::new(1))?;
    let stores = TestSymrefStoreProvider {
        stores: HashMap::from([(RepositoryId::new(1), store.clone())]),
    };
    let repo_ids_and_requests = vec![(
        RepositoryId::new(1),
        request_with_branch("repo/with", Some("refs/heads/x")),
    )];

    with_just_knobs_async(
        symref_jk(false),
        async {
            let enabled = validate_default_branches(&requests_of(&repo_ids_and_requests))
                .expect("an invalid default_branch must NOT be rejected with the kill switch off");
            assert!(!enabled, "writes must be disabled with the kill switch off");
            let written =
                write_default_branch_symrefs(&ctx, &stores, &repo_ids_and_requests, enabled)
                    .await
                    .expect("the gated-off path should succeed as a no-op");
            assert!(
                written.is_empty(),
                "nothing should be reported as written with the kill switch off",
            );
            anyhow::Ok(())
        }
        .boxed(),
    )
    .await?;

    assert_eq!(
        head_branch(&ctx, &store).await?,
        None,
        "no HEAD row should be written with the kill switch off",
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn jk_on_invalid_branch_is_rejected(_fb: FacebookInit) -> Result<()> {
    with_just_knobs_async(
        symref_jk(true),
        async {
            let err = validate_default_branches(&[request_with_branch(
                "repo/with",
                Some("refs/heads/x"),
            )])
            .expect_err("an invalid default_branch must be rejected with the kill switch on");
            assert!(
                matches!(err, scs_errors::ServiceError::Request(_)),
                "expected Request error, got: {err:?}"
            );
            anyhow::Ok(())
        }
        .boxed(),
    )
    .await?;
    Ok(())
}

#[mononoke::fbinit_test]
async fn no_default_branch_targets_short_circuit_before_jk(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let store = sqlite_store(RepositoryId::new(1))?;
    let stores = TestSymrefStoreProvider {
        stores: HashMap::from([(RepositoryId::new(1), store.clone())]),
    };
    let repo_ids_and_requests = vec![(
        RepositoryId::new(1),
        request_with_branch("repo/without", None),
    )];

    // Deliberately no `with_just_knobs_async`: a JK eval against the unset test store panics.
    let enabled = validate_default_branches(&requests_of(&repo_ids_and_requests))
        .expect("a batch with no default_branch must succeed without a JK eval");
    assert!(!enabled, "a batch with no default_branch enables nothing");
    let written = write_default_branch_symrefs(&ctx, &stores, &repo_ids_and_requests, enabled)
        .await
        .expect("a batch with no default_branch must succeed without a JK eval");
    assert!(
        written.is_empty(),
        "nothing should be reported as written for a batch with no default_branch",
    );
    assert_eq!(
        head_branch(&ctx, &store).await?,
        None,
        "no HEAD row should be written for a batch with no default_branch",
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn poll_cleanup_deletes_symrefs_and_sot_rows(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let sot = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
    // An unseeded sequence refuses to allocate.
    sot.seed_repo_id_sequence(&ctx, 100_902).await?;
    sot.insert_repos(
        &ctx,
        &[
            (
                RepositoryId::new(1),
                RepositoryName("repo/a".to_string()),
                GitSourceOfTruth::Reserved,
            ),
            (
                RepositoryId::new(2),
                RepositoryName("repo/b".to_string()),
                GitSourceOfTruth::Reserved,
            ),
        ],
    )
    .await?;
    sot.update_mutation_id_by_repo_names_for_reserved_repos(
        &ctx,
        &[
            RepositoryName("repo/a".to_string()),
            RepositoryName("repo/b".to_string()),
        ],
        4242,
    )
    .await?;

    let store_a = sqlite_store(RepositoryId::new(1))?;
    let store_b = sqlite_store(RepositoryId::new(2))?;
    write_default_branch_symref(&ctx, store_a.as_ref(), "main")
        .await
        .expect("seeding repo/a's HEAD row should succeed");
    write_default_branch_symref(&ctx, store_b.as_ref(), "main")
        .await
        .expect("seeding repo/b's HEAD row should succeed");
    let stores = TestSymrefStoreProvider {
        stores: HashMap::from([
            (RepositoryId::new(1), store_a.clone()),
            (RepositoryId::new(2), store_b.clone()),
        ]),
    };

    cleanup_repos(ctx.clone(), &sot, &stores, 4242)
        .await
        .expect("poll-path cleanup should succeed");

    assert_eq!(
        head_branch(&ctx, &store_a).await?,
        None,
        "cleanup must delete repo/a's HEAD row",
    );
    assert_eq!(
        head_branch(&ctx, &store_b).await?,
        None,
        "cleanup must delete repo/b's HEAD row",
    );
    for name in ["repo/a", "repo/b"] {
        assert!(
            sot.get_by_repo_name(
                &ctx,
                &RepositoryName(name.to_string()),
                Staleness::MostRecent,
            )
            .await?
            .is_none(),
            "cleanup must delete the reserved SoT row for {name}",
        );
    }
    Ok(())
}

#[mononoke::fbinit_test]
async fn poll_cleanup_with_unseeded_symref_store_succeeds(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let sot = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
    // An unseeded sequence refuses to allocate.
    sot.seed_repo_id_sequence(&ctx, 100_902).await?;
    sot.insert_repos(
        &ctx,
        &[(
            RepositoryId::new(1),
            RepositoryName("repo/a".to_string()),
            GitSourceOfTruth::Reserved,
        )],
    )
    .await?;
    sot.update_mutation_id_by_repo_names_for_reserved_repos(
        &ctx,
        &[RepositoryName("repo/a".to_string())],
        4242,
    )
    .await?;

    let store = sqlite_store(RepositoryId::new(1))?;
    let stores = TestSymrefStoreProvider {
        stores: HashMap::from([(RepositoryId::new(1), store.clone())]),
    };

    cleanup_repos(ctx.clone(), &sot, &stores, 4242)
        .await
        .expect("cleanup against a store with no HEAD row must succeed");

    assert_eq!(
        head_branch(&ctx, &store).await?,
        None,
        "there is still no HEAD row after cleanup",
    );
    assert!(
        sot.get_by_repo_name(
            &ctx,
            &RepositoryName("repo/a".to_string()),
            Staleness::MostRecent,
        )
        .await?
        .is_none(),
        "cleanup must still delete the reserved SoT row",
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn prepare_failure_cleanup_deletes_symrefs_and_sot_rows(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let sot = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
    // An unseeded sequence refuses to allocate.
    sot.seed_repo_id_sequence(&ctx, 100_902).await?;
    sot.insert_repos(
        &ctx,
        &[(
            RepositoryId::new(1),
            RepositoryName("repo/a".to_string()),
            GitSourceOfTruth::Reserved,
        )],
    )
    .await?;

    let store = sqlite_store(RepositoryId::new(1))?;
    write_default_branch_symref(&ctx, store.as_ref(), "main")
        .await
        .expect("seeding the HEAD row should succeed");
    let stores = TestSymrefStoreProvider {
        stores: HashMap::from([(RepositoryId::new(1), store.clone())]),
    };
    let params = thrift::CreateReposParams {
        repos: vec![request_with_branch("repo/a", Some("main"))],
        ..Default::default()
    };

    cleanup_reserved_repos_after_failure(&ctx, &sot, &stores, &[RepositoryId::new(1)], &params)
        .await
        .expect("prepare-failure cleanup should succeed");

    assert_eq!(
        head_branch(&ctx, &store).await?,
        None,
        "prepare-failure cleanup must delete the HEAD row",
    );
    assert!(
        sot.get_by_repo_name(
            &ctx,
            &RepositoryName("repo/a".to_string()),
            Staleness::MostRecent,
        )
        .await?
        .is_none(),
        "prepare-failure cleanup must delete the reserved SoT row",
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn poll_cleanup_failed_symref_delete_keeps_sot_rows(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let sot = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
    // An unseeded sequence refuses to allocate.
    sot.seed_repo_id_sequence(&ctx, 100_902).await?;
    sot.insert_repos(
        &ctx,
        &[(
            RepositoryId::new(1),
            RepositoryName("repo/a".to_string()),
            GitSourceOfTruth::Reserved,
        )],
    )
    .await?;
    sot.update_mutation_id_by_repo_names_for_reserved_repos(
        &ctx,
        &[RepositoryName("repo/a".to_string())],
        7,
    )
    .await?;

    let stores = TestSymrefStoreProvider {
        stores: HashMap::from([(
            RepositoryId::new(1),
            Arc::new(FailingSymrefStore {
                repo_id: RepositoryId::new(1),
            }) as Arc<dyn GitSymbolicRefs>,
        )]),
    };

    cleanup_repos(ctx.clone(), &sot, &stores, 7)
        .await
        .expect_err("cleanup must fail when the symref delete fails");

    let entry = sot
        .get_by_repo_name(
            &ctx,
            &RepositoryName("repo/a".to_string()),
            Staleness::MostRecent,
        )
        .await?
        .expect("the SoT row must survive a failed symref delete");
    assert_eq!(
        entry.source_of_truth,
        GitSourceOfTruth::Reserved,
        "the surviving row must still be Reserved (retryable)",
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn prepare_failure_cleanup_failed_symref_delete_keeps_sot_rows(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let sot = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
    // An unseeded sequence refuses to allocate.
    sot.seed_repo_id_sequence(&ctx, 100_902).await?;
    sot.insert_repos(
        &ctx,
        &[(
            RepositoryId::new(1),
            RepositoryName("repo/a".to_string()),
            GitSourceOfTruth::Reserved,
        )],
    )
    .await?;

    let stores = TestSymrefStoreProvider {
        stores: HashMap::from([(
            RepositoryId::new(1),
            Arc::new(FailingSymrefStore {
                repo_id: RepositoryId::new(1),
            }) as Arc<dyn GitSymbolicRefs>,
        )]),
    };
    let params = thrift::CreateReposParams {
        repos: vec![request_with_branch("repo/a", Some("main"))],
        ..Default::default()
    };

    cleanup_reserved_repos_after_failure(&ctx, &sot, &stores, &[RepositoryId::new(1)], &params)
        .await
        .expect_err("prepare-failure cleanup must fail when the symref delete fails");

    let entry = sot
        .get_by_repo_name(
            &ctx,
            &RepositoryName("repo/a".to_string()),
            Staleness::MostRecent,
        )
        .await?
        .expect("the SoT row must survive a failed symref delete");
    assert_eq!(
        entry.source_of_truth,
        GitSourceOfTruth::Reserved,
        "the surviving row must still be Reserved (retryable)",
    );
    Ok(())
}
