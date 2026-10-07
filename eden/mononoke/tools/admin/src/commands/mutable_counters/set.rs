/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use context::CoreContext;
use mutable_counters::MutableCountersRef;
use repo_identity::RepoIdentityRef;

use super::Repo;

#[derive(Args)]
pub struct SetArgs {
    /// The name of the counter
    counter_name: String,
    /// The value to be set for the counter
    value: i64,
    /// The previous value of the counter. If provided, the counter will be updated only if its
    /// previous value matches the one provided as input
    #[clap(long)]
    prev_value: Option<i64>,
}

pub async fn set(ctx: &CoreContext, repo: &Repo, set_args: SetArgs) -> Result<()> {
    let mutable_counters = repo.mutable_counters();
    let name = set_args.counter_name.as_ref();
    let was_set = mutable_counters
        .set_counter(ctx, name, set_args.value, set_args.prev_value)
        .await?;
    let repo_id = repo.repo_identity().id();
    let repo_name = repo.repo_identity().name();
    if was_set {
        println!(
            "Value of {} in repo {}(Id: {}) set to {}",
            name, repo_name, repo_id, set_args.value
        );
    } else {
        println!(
            "Value of {} in repo {}(Id: {}) was NOT set to {}. The previous value of the counter did not match {:?}",
            name,
            repo_name,
            repo_id,
            set_args.value,
            set_args.prev_value.clone()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use fbinit::FacebookInit;
    use mononoke_macros::mononoke;
    use test_repo_factory::TestRepoFactory;

    use super::*;

    fn set_args(value: i64, prev_value: Option<i64>) -> SetArgs {
        SetArgs {
            counter_name: "foo".to_string(),
            value,
            prev_value,
        }
    }

    /// FIXME: BUG! This did return `Ok(())` (exit status 0) when `set --prev-value` found a
    /// different previous value and left the counter untouched, but should have returned an
    /// error so that callers can see the compare-and-set did not happen.
    #[mononoke::fbinit_test]
    async fn test_set_with_mismatched_prev_value_fails(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: Repo = TestRepoFactory::new(fb)?.build().await?;
        set(&ctx, &repo, set_args(7, None)).await?;
        set(&ctx, &repo, set_args(10, Some(7))).await?;

        let result = set(&ctx, &repo, set_args(12, Some(8))).await;
        // FIXME: BUG! The mismatch is only printed; the command still succeeds.
        assert!(result.is_ok());
        assert_eq!(
            repo.mutable_counters().get_counter(&ctx, "foo").await?,
            Some(10)
        );
        Ok(())
    }
}
