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
use mononoke_macros::mononoke;
use mononoke_types::NonRootMPath;
use permission_checker::MononokeIdentity;

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
async fn test_source_denial_permission_request_group_is_deterministic() -> Result<()> {
    let handle = SharedFetchHandle::from_result(Ok(vec![
        path_restriction_check_with("restricted/z", "REPO_REGION:z_acl", false)?,
        path_restriction_check_with("restricted/a", "REPO_REGION:a_acl", false)?,
    ]));

    let denial_permission_request_group = super::source_denial_permission_request_group(
        &handle,
        &[],
        &super::PreFilterVariant::Definite,
    )
    .await?;

    assert_eq!(
        denial_permission_request_group,
        Some(MononokeIdentity::from_str("REPO_REGION:a_acl")?)
    );
    Ok(())
}

// What it tests: authoritative source aggregation keeps deny-over-error
// semantics after carrying the permission request group through the denial.
// Expected: any denial wins over sibling source errors, while a no-deny error
// is propagated.
#[tokio::test]
async fn test_authoritative_source_denial_permission_request_group_preserves_error_semantics()
-> Result<()> {
    let permission_request_group = MononokeIdentity::from_str("REPO_REGION:deny_acl")?;
    let denied = super::authoritative_sources_denial_permission_request_group(vec![
        Err(anyhow::anyhow!("source failed")),
        Ok(Some(permission_request_group.clone())),
    ])?;
    assert_eq!(denied, Some(permission_request_group));

    let no_denial = super::authoritative_sources_denial_permission_request_group(vec![
        Ok(None),
        Err(anyhow::anyhow!("source failed")),
    ]);
    assert!(no_denial.is_err());

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
        AuthorizationCheckResult::new(has_acl_access, false, false),
        acl,
    ))
}
