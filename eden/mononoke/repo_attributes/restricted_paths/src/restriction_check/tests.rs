/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use anyhow::Context;
use anyhow::Result;
use context::CoreContext;
use context::SessionContainer;
use fbinit::FacebookInit;
use metadata::Metadata;
use mononoke_macros::mononoke;
use mononoke_types::NonRootMPath;
use permission_checker::AclProvider;
use permission_checker::Acls;
use permission_checker::InternalAclProvider;
use permission_checker::MononokeIdentity;
use permission_checker::MononokeIdentitySet;

use super::AuthorizationCheckResult;
use super::PathRestrictionCheckResult;
use super::SharedFetchHandle;
use crate::restriction_info::PathRestrictionInfo;

// What it tests: cloned source fetch handles share one spawned fetch result.
// Expected: the underlying task runs once and all awaiters receive the shared
// cached result.
#[tokio::test]
async fn test_shared_fetch_handle_awaits_one_spawned_fetch() -> Result<()> {
    let run_count = Arc::new(AtomicUsize::new(0));
    let (release_sender, release_receiver) = tokio::sync::oneshot::channel();
    let run_count_for_task = run_count.clone();

    let join_handle = mononoke::spawn_task(async move {
        run_count_for_task.fetch_add(1, Ordering::SeqCst);
        release_receiver
            .await
            .context("release shared fetch test task")?;
        Ok(vec![path_restriction_check()?])
    });

    let handle = SharedFetchHandle::from_join_handle(join_handle);
    let first_waiter = handle.await_result();
    let cloned_handle = handle.clone();
    let second_waiter = cloned_handle.await_result();

    release_sender
        .send(())
        .map_err(|()| anyhow::anyhow!("shared fetch test task dropped release receiver"))?;
    let (first_result, second_result) = futures::try_join!(first_waiter, second_waiter)?;

    assert_eq!(run_count.load(Ordering::SeqCst), 1);
    assert_eq!(first_result.as_ref(), second_result.as_ref());
    assert!(std::ptr::eq(
        first_result.as_ref().as_ptr(),
        second_result.as_ref().as_ptr(),
    ));

    Ok(())
}

// What it tests: denied checks select a stable permission request group independent of the
// source's original result order.
// Expected: the permission request group for the lexicographically first known
// restriction root is returned.
#[tokio::test]
async fn test_source_enforcement_outcome_denial_permission_request_group_is_deterministic()
-> Result<()> {
    let handle = SharedFetchHandle::from_result(Ok(vec![
        path_restriction_check_with("restricted/z", "REPO_REGION:z_acl", false)?,
        path_restriction_check_with("restricted/a", "REPO_REGION:a_acl", false)?,
    ]));

    let outcome =
        super::source_enforcement_outcome(&handle, &[], &super::PreFilterVariant::Definite).await?;

    assert_eq!(
        outcome.denial_permission_request_group,
        Some(MononokeIdentity::from_str("REPO_REGION:a_acl")?)
    );
    Ok(())
}

// What it tests: authoritative source aggregation keeps deny-over-error
// semantics after carrying the permission request group through the denial.
// Expected: any denial wins over sibling source errors, while a no-deny error
// is propagated.
#[tokio::test]
async fn test_authoritative_source_enforcement_outcome_preserves_error_semantics() -> Result<()> {
    let permission_request_group = MononokeIdentity::from_str("REPO_REGION:deny_acl")?;
    let denied = super::authoritative_sources_enforcement_outcome(vec![
        Err(anyhow::anyhow!("source failed")),
        Ok(super::AccessEnforcementOutcome {
            access_enforcement_enabled: true,
            denial_permission_request_group: Some(permission_request_group.clone()),
        }),
    ])?;
    assert_eq!(
        denied.denial_permission_request_group,
        Some(permission_request_group)
    );

    let no_denial = super::authoritative_sources_enforcement_outcome(vec![
        Ok(super::AccessEnforcementOutcome {
            access_enforcement_enabled: false,
            denial_permission_request_group: None,
        }),
        Err(anyhow::anyhow!("source failed")),
    ]);
    assert!(no_denial.is_err());

    Ok(())
}

// What it tests: a member of the admin bypass group is authorized, and the grant
// is attributed to the bypass — not mislabeled as direct ACL read access.
// Expected: is_admin_bypass is true, has_acl_access is false, and the caller is
// authorized overall.
#[mononoke::fbinit_test]
async fn test_admin_bypass_group_member_is_authorized_and_flagged(fb: FacebookInit) -> Result<()> {
    let acl_provider = admin_bypass_acl_provider()?;
    // carol is only in the bypass group, with no direct read access.
    let ctx = ctx_with_identities(fb, &["USER:carol"])?;
    let acl = MononokeIdentity::from_str("REPO_REGION:repos/hg/fbsource/=project1")?;
    let bypass_group = MononokeIdentity::from_str("GROUP:path_acls_admin_bypass")?;

    let authorization = super::check_authorization(
        &ctx,
        &acl_provider,
        &[&acl],
        None,
        None,
        Some(&bypass_group),
    )
    .await?;

    assert!(
        authorization.is_admin_bypass(),
        "bypass-group member should be flagged as an admin bypass",
    );
    assert!(
        !authorization.has_acl_access(),
        "bypass grant must not be mislabeled as direct ACL read access",
    );
    assert!(
        authorization.has_authorization(),
        "bypass-group member should be authorized overall",
    );
    Ok(())
}

// What it tests: a caller with neither read access nor bypass-group membership
// is denied even when a bypass group is configured.
// Expected: no authorization, and neither the ACL nor bypass flag is set.
#[mononoke::fbinit_test]
async fn test_non_member_without_acl_is_denied(fb: FacebookInit) -> Result<()> {
    let acl_provider = admin_bypass_acl_provider()?;
    // bob has neither read access nor bypass-group membership.
    let ctx = ctx_with_identities(fb, &["USER:bob"])?;
    let acl = MononokeIdentity::from_str("REPO_REGION:repos/hg/fbsource/=project1")?;
    let bypass_group = MononokeIdentity::from_str("GROUP:path_acls_admin_bypass")?;

    let authorization = super::check_authorization(
        &ctx,
        &acl_provider,
        &[&acl],
        None,
        None,
        Some(&bypass_group),
    )
    .await?;

    assert!(
        !authorization.has_authorization(),
        "caller without read access or bypass membership should be denied",
    );
    assert!(
        !authorization.has_acl_access(),
        "caller has no ACL read access"
    );
    assert!(
        !authorization.is_admin_bypass(),
        "caller is not in the bypass group"
    );
    Ok(())
}

// What it tests: a caller with direct ACL read access is authorized via the ACL,
// not the bypass.
// Expected: has_acl_access is true and is_admin_bypass is false.
#[mononoke::fbinit_test]
async fn test_direct_acl_read_is_not_flagged_as_bypass(fb: FacebookInit) -> Result<()> {
    let acl_provider = admin_bypass_acl_provider()?;
    // alice has direct read access on project1 but is not in the bypass group.
    let ctx = ctx_with_identities(fb, &["USER:alice"])?;
    let acl = MononokeIdentity::from_str("REPO_REGION:repos/hg/fbsource/=project1")?;
    let bypass_group = MononokeIdentity::from_str("GROUP:path_acls_admin_bypass")?;

    let authorization = super::check_authorization(
        &ctx,
        &acl_provider,
        &[&acl],
        None,
        None,
        Some(&bypass_group),
    )
    .await?;

    assert!(
        authorization.has_acl_access(),
        "user with direct ACL read access should be granted via the ACL",
    );
    assert!(
        !authorization.is_admin_bypass(),
        "a direct ACL reader must not be flagged as an admin bypass",
    );
    Ok(())
}

fn path_restriction_check() -> Result<PathRestrictionCheckResult> {
    path_restriction_check_with("restricted", "REPO_REGION:test_acl", true)
}

fn path_restriction_check_with(
    restriction_root: &str,
    acl: &str,
    has_acl_access: bool,
) -> Result<PathRestrictionCheckResult> {
    let acl = MononokeIdentity::from_str(acl)?;
    Ok(PathRestrictionCheckResult::new(
        PathRestrictionInfo {
            restriction_root: NonRootMPath::new(restriction_root)?,
            repo_region_acl: acl.to_string(),
            permission_request_group: acl.clone(),
        },
        AuthorizationCheckResult::new(has_acl_access, false, false, false),
        acl,
    ))
}

/// Build an `InternalAclProvider` for the bypass-group access tests:
/// `alice` has direct `read` access on `project1`, while `carol` is only a
/// member of the `path_acls_admin_bypass` group. `bob` has neither.
fn admin_bypass_acl_provider() -> Result<Arc<dyn AclProvider>> {
    let acls: Acls = serde_json::from_str(
        r#"
        {
            "repo_regions": {
                "repos/hg/fbsource/=project1": {
                    "actions": {
                        "read": ["USER:alice"]
                    }
                }
            },
            "groups": {
                "path_acls_admin_bypass": ["USER:carol"]
            }
        }
        "#,
    )?;
    Ok(InternalAclProvider::new(acls))
}

/// Build a test `CoreContext` whose caller presents the given identities.
fn ctx_with_identities(fb: FacebookInit, ids: &[&str]) -> Result<CoreContext> {
    let identities = ids
        .iter()
        .map(|id| id.parse())
        .collect::<Result<MononokeIdentitySet>>()?;
    let metadata = Metadata::default().set_identities(identities);
    let session = SessionContainer::builder(fb)
        .metadata(Arc::new(metadata))
        .build();
    Ok(CoreContext::test_mock_session(session))
}
