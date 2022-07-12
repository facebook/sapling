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
use maplit::hashmap;
use maplit::hashset;
use metaconfig_types::RepoConfig;
use metaconfig_types::ServiceWriteRestrictions;
use mononoke_types::PrefixTrie;
use permission_checker::MononokeIdentitySet;
use repo_bookmark_attrs::RepoBookmarkAttrs;
use repo_permission_checker::RepoPermissionChecker;

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
    service_writes: HashMap<String, bool>,
}

#[async_trait]
impl RepoPermissionChecker for TestPermissionChecker {
    async fn check_if_read_access_allowed(
        &self,
        _identities: &MononokeIdentitySet,
    ) -> Result<bool> {
        Ok(self.read)
    }

    async fn check_if_draft_access_allowed(
        &self,
        _identities: &MononokeIdentitySet,
    ) -> Result<bool> {
        Ok(self.draft)
    }

    async fn check_if_write_access_allowed(
        &self,
        _identities: &MononokeIdentitySet,
    ) -> Result<bool> {
        Ok(self.write)
    }

    async fn check_if_read_only_bypass_allowed(
        &self,
        _identities: &MononokeIdentitySet,
    ) -> Result<bool> {
        Ok(self.read_only_bypass)
    }

    async fn check_if_service_writes_allowed(
        &self,
        _identities: &MononokeIdentitySet,
        service_name: &str,
    ) -> Result<bool> {
        Ok(self
            .service_writes
            .get(service_name)
            .copied()
            .unwrap_or(false))
    }
}

#[fbinit::test]
async fn test_user_access(fb: FacebookInit) -> Result<()> {
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
    let authz = AuthorizationContext::new();

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
