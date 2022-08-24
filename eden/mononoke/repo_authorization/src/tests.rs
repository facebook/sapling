/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkName;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::FutureExt;
use maplit::hashmap;
use maplit::hashset;
use metaconfig_types::RepoConfig;
use metaconfig_types::ServiceWriteRestrictions;
use mononoke_types::PrefixTrie;
use permission_checker::MononokeIdentitySet;
use repo_bookmark_attrs::RepoBookmarkAttrs;
use repo_permission_checker::RepoPermissionChecker;
use tunables::with_tunables_async;
use tunables::MononokeTunables;

use crate::AuthorizationContext;
use crate::RepoWriteOperation;

#[facet::container]
struct Repo {
    #[facet]
    repo_config: RepoConfig,

    #[facet]
    repo_permission_checker: dyn RepoPermissionChecker,

    #[facet]
    repo_bookmark_attrs: RepoBookmarkAttrs,
}

#[fbinit::test]
async fn test_full_access(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb)?;
    let authz = AuthorizationContext::new_bypass_access_control();

    authz.require_full_repo_read(&ctx, &repo).await?;
    authz
        .require_repo_write(&ctx, &repo, RepoWriteOperation::CreateChangeset)
        .await?;
    authz
        .require_repo_write(
            &ctx,
            &repo,
            RepoWriteOperation::CreateBookmark(BookmarkKind::Scratch),
        )
        .await?;
    authz
        .require_repo_write(
            &ctx,
            &repo,
            RepoWriteOperation::LandStack(BookmarkKind::Publishing),
        )
        .await?;
    authz
        .require_bookmark_modify(&ctx, &repo, &BookmarkName::new("main")?)
        .await?;

    Ok(())
}

#[derive(Clone, Default, Debug)]
struct TestPermissionChecker {
    read: bool,
    draft: bool,
    write: bool,
    read_only_bypass: bool,
    any_region_read: bool,
    service_writes: HashMap<String, bool>,
}

#[async_trait]
impl RepoPermissionChecker for TestPermissionChecker {
    async fn check_if_read_access_allowed(&self, _identities: &MononokeIdentitySet) -> bool {
        self.read
    }

    async fn check_if_any_region_read_access_allowed(
        &self,
        _identities: &MononokeIdentitySet,
    ) -> bool {
        self.any_region_read
    }

    async fn check_if_region_read_access_allowed<'a>(
        &'a self,
        _acls: &'a [&'a str],
        _identities: &'a MononokeIdentitySet,
    ) -> bool {
        false
    }

    async fn check_if_draft_access_allowed(&self, _identities: &MononokeIdentitySet) -> bool {
        self.draft
    }

    async fn check_if_write_access_allowed(&self, _identities: &MononokeIdentitySet) -> bool {
        self.write
    }

    async fn check_if_read_only_bypass_allowed(&self, _identities: &MononokeIdentitySet) -> bool {
        self.read_only_bypass
    }

    async fn check_if_service_writes_allowed(
        &self,
        _identities: &MononokeIdentitySet,
        service_name: &str,
    ) -> bool {
        self.service_writes
            .get(service_name)
            .copied()
            .unwrap_or(false)
    }
}

#[fbinit::test]
async fn test_user_no_write_access(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let checker = Arc::new(TestPermissionChecker {
        read: true,
        draft: true,
        write: false,
        ..Default::default()
    });
    let repo: Repo = test_repo_factory::TestRepoFactory::new(fb)?
        .with_permission_checker(checker)
        .build()?;
    let authz = AuthorizationContext::new(&ctx);

    authz.require_full_repo_read(&ctx, &repo).await?;
    authz.require_full_repo_draft(&ctx, &repo).await?;
    authz
        .require_repo_write(&ctx, &repo, RepoWriteOperation::CreateChangeset)
        .await?;
    authz
        .require_repo_write(
            &ctx,
            &repo,
            RepoWriteOperation::CreateBookmark(BookmarkKind::Scratch),
        )
        .await?;
    assert!(
        authz
            .require_repo_write(
                &ctx,
                &repo,
                RepoWriteOperation::LandStack(BookmarkKind::Publishing),
            )
            .await
            .is_err()
    );
    authz
        .require_bookmark_modify(&ctx, &repo, &BookmarkName::new("main")?)
        .await?;

    Ok(())
}

#[fbinit::test]
async fn test_user_no_draft_enforceent_off(fb: FacebookInit) -> Result<()> {
    let tunables = MononokeTunables::default();
    tunables.update_bools(&hashmap! {
        "log_draft_acl_failures".to_string() => true,
        "enforce_draft_acl".to_string() => false,
    });
    with_tunables_async(
        tunables,
        async {
            let ctx = CoreContext::test_mock(fb);
            let checker = Arc::new(TestPermissionChecker {
                read: true,
                draft: false,
                write: false,
                ..Default::default()
            });
            let repo: Repo = test_repo_factory::TestRepoFactory::new(fb)?
                .with_permission_checker(checker)
                .build()?;
            let authz = AuthorizationContext::new(&ctx);

            authz.require_full_repo_read(&ctx, &repo).await?;
            authz.require_full_repo_draft(&ctx, &repo).await?;
            authz
                .require_repo_write(&ctx, &repo, RepoWriteOperation::CreateChangeset)
                .await?;
            authz
                .require_repo_write(
                    &ctx,
                    &repo,
                    RepoWriteOperation::CreateBookmark(BookmarkKind::Scratch),
                )
                .await?;
            assert!(
                authz
                    .require_repo_write(
                        &ctx,
                        &repo,
                        RepoWriteOperation::LandStack(BookmarkKind::Publishing),
                    )
                    .await
                    .is_err()
            );

            Ok(())
        }
        .boxed(),
    )
    .await
}

#[fbinit::test]
async fn test_user_no_draft_no_write_access(fb: FacebookInit) -> Result<()> {
    let tunables = MononokeTunables::default();
    tunables.update_bools(&hashmap! {
        "log_draft_acl_failures".to_string() => true,
        "enforce_draft_acl".to_string() => true,
    });
    with_tunables_async(
        tunables,
        async {
            let ctx = CoreContext::test_mock(fb);
            let checker = Arc::new(TestPermissionChecker {
                read: true,
                draft: false,
                write: false,
                ..Default::default()
            });
            let repo: Repo = test_repo_factory::TestRepoFactory::new(fb)?
                .with_permission_checker(checker)
                .build()?;
            let authz = AuthorizationContext::new(&ctx);

            authz.require_full_repo_read(&ctx, &repo).await?;
            assert!(authz.require_full_repo_draft(&ctx, &repo).await.is_err());
            assert!(
                authz
                    .require_repo_write(&ctx, &repo, RepoWriteOperation::CreateChangeset)
                    .await
                    .is_err(),
            );
            assert!(
                authz
                    .require_repo_write(
                        &ctx,
                        &repo,
                        RepoWriteOperation::CreateBookmark(BookmarkKind::Scratch),
                    )
                    .await
                    .is_err(),
            );
            assert!(
                authz
                    .require_repo_write(
                        &ctx,
                        &repo,
                        RepoWriteOperation::LandStack(BookmarkKind::Publishing),
                    )
                    .await
                    .is_err()
            );

            // TODO(mitrandir): This seems fishy: why would bookmark_modify succeed when
            // the user has no write acceess!? Even if that's intended this API might be
            // easily misused. We need to audit this.
            authz
                .require_bookmark_modify(&ctx, &repo, &BookmarkName::new("main")?)
                .await?;

            Ok(())
        }
        .boxed(),
    )
    .await
}

// Write access should give implied draft access. This will help with migration
// to draft access enforcement and allow us to avoid unncecessary duplication of
// ACLs.
#[fbinit::test]
async fn test_user_write_no_draft_access(fb: FacebookInit) -> Result<()> {
    let tunables = MononokeTunables::default();
    tunables.update_bools(&hashmap! {
        "log_draft_acl_failures".to_string() => true,
        "enforce_draft_acl".to_string() => true,
    });
    with_tunables_async(
        tunables,
        async {
            let ctx = CoreContext::test_mock(fb);
            let checker = Arc::new(TestPermissionChecker {
                read: true,
                draft: false,
                write: true,
                ..Default::default()
            });
            let repo: Repo = test_repo_factory::TestRepoFactory::new(fb)?
                .with_permission_checker(checker)
                .build()?;
            let authz = AuthorizationContext::new(&ctx);

            authz.require_full_repo_read(&ctx, &repo).await?;
            authz.require_full_repo_draft(&ctx, &repo).await?;
            authz
                .require_repo_write(&ctx, &repo, RepoWriteOperation::CreateChangeset)
                .await?;
            authz
                .require_repo_write(
                    &ctx,
                    &repo,
                    RepoWriteOperation::CreateBookmark(BookmarkKind::Scratch),
                )
                .await?;
            authz
                .require_repo_write(
                    &ctx,
                    &repo,
                    RepoWriteOperation::LandStack(BookmarkKind::Publishing),
                )
                .await?;
            authz
                .require_bookmark_modify(&ctx, &repo, &BookmarkName::new("main")?)
                .await?;

            Ok(())
        }
        .boxed(),
    )
    .await
}

#[fbinit::test]
async fn test_service_access(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let checker = Arc::new(TestPermissionChecker {
        service_writes: hashmap! { String::from("test") => true },
        ..Default::default()
    });
    let repo: Repo = test_repo_factory::TestRepoFactory::new(fb)?
        .with_permission_checker(checker)
        .with_config_override(|config| {
            config.source_control_service.service_write_restrictions = hashmap! {
                String::from("test") =>
                ServiceWriteRestrictions {
                    permitted_methods: hashset! { String::from("create_changeset") },
                    permitted_bookmarks: hashset! { String::from("main") },
                    permitted_path_prefixes: PrefixTrie::Included,
                    ..Default::default()
                }
            };
        })
        .build()?;
    let authz_test_service = AuthorizationContext::new_for_service_writes("test");
    let authz_other_service = AuthorizationContext::new_for_service_writes("other");

    authz_test_service
        .require_repo_write(&ctx, &repo, RepoWriteOperation::CreateChangeset)
        .await?;
    assert!(
        authz_other_service
            .require_repo_write(&ctx, &repo, RepoWriteOperation::CreateChangeset)
            .await
            .is_err()
    );

    // Test service is permitted to modify main.
    authz_test_service
        .require_bookmark_modify(&ctx, &repo, &BookmarkName::new("main")?)
        .await?;

    // Another service is not permitted to modify main.
    assert!(
        authz_other_service
            .require_bookmark_modify(&ctx, &repo, &BookmarkName::new("main")?)
            .await
            .is_err()
    );

    Ok(())
}
