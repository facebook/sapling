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

fn path_restriction_check() -> Result<PathRestrictionCheckResult> {
    let acl = permission_checker::MononokeIdentity::from_str("REPO_REGION:test_acl")?;
    Ok(PathRestrictionCheckResult::new(
        PathRestrictionInfo {
            restriction_root: NonRootMPath::new("restricted")?,
            repo_region_acl: acl.to_string(),
            permission_request_group: acl.clone(),
        },
        AuthorizationCheckResult::new(true, false, false),
        acl,
    ))
}
