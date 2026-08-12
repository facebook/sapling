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
use super::SourceRestrictionSummary;
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

/// What it tests: rollout allowlisting is aggregated with `all`, not `any`.
/// Expected: the caller counts as rollout-allowlisted only when every
/// restriction in the batch allowlists them. Being allowlisted for one tent must
/// not authorize a different tent caught by the same request.
#[mononoke::test]
fn test_summary_rollout_allowlist_requires_every_check() -> Result<()> {
    let allowlisted = AuthorizationCheckResult::new(false, false, true, false);
    let not_allowlisted = AuthorizationCheckResult::new(false, false, false, false);

    let all_allowlisted = [
        check_with_authorization("tent_a", allowlisted)?,
        check_with_authorization("tent_b", allowlisted)?,
    ];
    assert!(
        SourceRestrictionSummary::from_checks(&all_allowlisted).is_rollout_allowlisted(),
        "every restriction in the batch allowlists the caller",
    );

    let mixed = [
        check_with_authorization("tent_a", allowlisted)?,
        check_with_authorization("tent_b", not_allowlisted)?,
    ];
    let summary = SourceRestrictionSummary::from_checks(&mixed);
    assert!(
        !summary.is_rollout_allowlisted(),
        "allowlisted on tent_a but not tent_b must not count as rollout-allowlisted",
    );
    assert!(
        !summary.has_authorization(),
        "with no ACL access, a partially allowlisted batch must be denied",
    );
    Ok(())
}

/// What it tests: `from_check_union` applies the same unanimity rule as
/// `from_checks` when merging checks reported by several sources.
/// Expected: one non-allowlisted check in the union denies the whole request.
#[mononoke::test]
fn test_summary_union_rollout_allowlist_requires_every_check() -> Result<()> {
    let allowlisted = AuthorizationCheckResult::new(false, false, true, false);
    let not_allowlisted = AuthorizationCheckResult::new(false, false, false, false);

    let allowlisted_check = check_with_authorization("tent_a", allowlisted)?;
    let other_allowlisted_check = check_with_authorization("tent_b", allowlisted)?;
    assert!(
        SourceRestrictionSummary::from_check_union([&allowlisted_check, &other_allowlisted_check])
            .is_rollout_allowlisted(),
        "every check in the union allowlists the caller",
    );

    let denied_check = check_with_authorization("tent_b", not_allowlisted)?;
    assert!(
        !SourceRestrictionSummary::from_check_union([&allowlisted_check, &denied_check])
            .is_rollout_allowlisted(),
        "one non-allowlisted check in the union denies the whole request",
    );
    Ok(())
}

/// What it tests: an empty check batch is not reported as rollout-allowlisted.
/// Expected: `is_rollout_allowlisted` is false even though `all` is vacuously
/// true over an empty batch, while `has_authorization` stays true. Nothing was
/// restricted, so the access is allowed — but it was not allowlisted, and
/// logging it as such would pollute the `is_rollout_allowlisted` column.
#[mononoke::test]
fn test_summary_empty_batch_is_not_rollout_allowlisted() -> Result<()> {
    let empty: [PathRestrictionCheckResult; 0] = [];
    let summary = SourceRestrictionSummary::from_checks(&empty);

    assert!(
        !summary.is_rollout_allowlisted(),
        "an unrestricted access must not be reported as rollout-allowlisted",
    );
    assert!(
        summary.has_authorization(),
        "no restriction matched, so the access is authorized",
    );
    Ok(())
}

/// What it tests: the tooling and admin-bypass flags keep `any` aggregation.
/// Expected: both are repo-wide grants, so a single matching check is enough.
/// Only the per-tent rollout allowlist requires unanimity.
#[mononoke::test]
fn test_summary_repo_wide_flags_use_any() -> Result<()> {
    let tooling_only = AuthorizationCheckResult::new(false, true, false, false);
    let admin_only = AuthorizationCheckResult::new(false, false, false, true);
    let neither = AuthorizationCheckResult::new(false, false, false, false);

    let checks = [
        check_with_authorization("tent_a", tooling_only)?,
        check_with_authorization("tent_b", admin_only)?,
        check_with_authorization("tent_c", neither)?,
    ];
    let summary = SourceRestrictionSummary::from_checks(&checks);

    assert!(
        summary.is_allowlisted_tooling(),
        "the tooling allowlist is repo-wide, so one matching check is enough",
    );
    assert!(
        summary.is_admin_bypass(),
        "the admin bypass is repo-wide, so one matching check is enough",
    );
    assert!(
        !summary.is_rollout_allowlisted(),
        "no check in the batch is rollout-allowlisted",
    );
    Ok(())
}

fn path_restriction_check() -> Result<PathRestrictionCheckResult> {
    path_restriction_check_with("restricted", "REPO_REGION:test_acl", true)
}

/// Build a check carrying an explicit authorization result, for the summary
/// aggregation tests. `restriction_root` only needs to be unique per check;
/// these tests assert on the aggregated flags, not on paths or ACLs.
fn check_with_authorization(
    restriction_root: &str,
    authorization: AuthorizationCheckResult,
) -> Result<PathRestrictionCheckResult> {
    let acl = MononokeIdentity::from_str("REPO_REGION:test_acl")?;
    Ok(PathRestrictionCheckResult::new(
        PathRestrictionInfo {
            restriction_root: NonRootMPath::new(restriction_root)?,
            repo_region_acl: acl.to_string(),
            permission_request_group: acl.clone(),
        },
        authorization,
        acl,
    ))
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
