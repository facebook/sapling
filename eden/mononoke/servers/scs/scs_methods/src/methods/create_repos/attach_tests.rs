/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use fbinit::FacebookInit;
use futures::FutureExt;
use git_source_of_truth::GitSourceOfTruth;
use git_source_of_truth::RepositoryName;
use git_source_of_truth::SqlGitSourceOfTruthConfigBuilder;
use justknobs::test_helpers::JustKnobsInMemory;
use justknobs::test_helpers::KnobVal;
use justknobs::test_helpers::with_just_knobs_async;
use mononoke_macros::mononoke;
use sql_construct::SqlConstruct;

use super::*;

fn params_for(names: &[&str]) -> thrift::CreateReposParams {
    thrift::CreateReposParams {
        repos: names
            .iter()
            .map(|n| thrift::RepoCreationRequest {
                repo_name: (*n).to_string(),
                scm_type: thrift::RepoScmType::GIT,
                size_bucket: thrift::RepoSizeBucket::SMALL,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

#[mononoke::fbinit_test]
async fn stamp_count_mismatch_fails_creation(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let config = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
    // An unseeded sequence refuses to allocate.
    config.seed_repo_id_sequence(&ctx, 100_902).await?;
    config
        .insert_repos(
            &ctx,
            &[(
                RepositoryId::new(1),
                RepositoryName("repo/a".to_string()),
                GitSourceOfTruth::Reserved,
            )],
        )
        .await?;

    // Only one of the two expected reserved rows exists: the stamp must
    // fail the creation rather than proceed with a lost row.
    let params = params_for(&["repo/a", "repo/b"]);
    let result =
        update_mutation_id_by_repo_names_for_reserved_repos(ctx.clone(), &config, &params, 4242)
            .await;
    assert!(result.is_err(), "missing reserved row must fail the stamp");

    // With every expected row reserved, the stamp succeeds.
    config
        .insert_repos(
            &ctx,
            &[(
                RepositoryId::new(2),
                RepositoryName("repo/b".to_string()),
                GitSourceOfTruth::Reserved,
            )],
        )
        .await?;
    update_mutation_id_by_repo_names_for_reserved_repos(ctx.clone(), &config, &params, 4243)
        .await
        .map_err(|e| anyhow::anyhow!("stamp should succeed: {e:?}"))?;

    Ok(())
}

#[mononoke::fbinit_test]
async fn lost_ack_restamp_confirm_read(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let config = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
    // An unseeded sequence refuses to allocate.
    config.seed_repo_id_sequence(&ctx, 100_902).await?;
    config
        .insert_repos(
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
    // Attempt 1 committed the stamp but its ack was lost; the retry's
    // UPDATE reports 0 changed rows on MySQL. Sqlite counts matched rows,
    // so exercise the confirm-read directly against the stamped state.
    config
        .update_mutation_id_by_repo_names_for_reserved_repos(
            &ctx,
            &[
                RepositoryName("repo/a".to_string()),
                RepositoryName("repo/b".to_string()),
            ],
            4242,
        )
        .await?;

    let params = params_for(&["repo/a", "repo/b"]);
    assert!(
        all_requested_repos_stamped(&ctx, &config, &params, 4242)
            .await
            .map_err(|e| anyhow::anyhow!("confirm-read should succeed: {e:?}"))?,
        "every requested repo carries the stamp: the re-stamp must be accepted",
    );

    // Genuine loss: a requested repo with no stamped row must still fail.
    let params_missing = params_for(&["repo/a", "repo/b", "repo/c"]);
    assert!(
        !all_requested_repos_stamped(&ctx, &config, &params_missing, 4242)
            .await
            .map_err(|e| anyhow::anyhow!("confirm-read should succeed: {e:?}"))?,
        "a repo missing from the stamped set must not be accepted",
    );

    // Rows stamped under a different mutation are not ours.
    assert!(
        !all_requested_repos_stamped(&ctx, &config, &params, 9999)
            .await
            .map_err(|e| anyhow::anyhow!("confirm-read should succeed: {e:?}"))?,
        "rows stamped with another mutation_id must not be accepted",
    );

    // A stamped row that a concurrent attacher's poll already flipped to
    // Mononoke still proves the stamp committed: the re-stamp must be
    // accepted, not reported as row loss.
    config
        .update_source_of_truth_by_repo_names(
            &ctx,
            GitSourceOfTruth::Mononoke,
            &[RepositoryName("repo/b".to_string())],
        )
        .await?;
    assert!(
        all_requested_repos_stamped(&ctx, &config, &params, 4242)
            .await
            .map_err(|e| anyhow::anyhow!("confirm-read should succeed: {e:?}"))?,
        "a row flipped to Mononoke under our mutation_id still counts as stamped",
    );
    assert!(
        !all_requested_repos_stamped(&ctx, &config, &params, 9999)
            .await
            .map_err(|e| anyhow::anyhow!("confirm-read should succeed: {e:?}"))?,
        "flipped rows under another mutation_id must not be accepted",
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn attach_happy_path(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let config = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
    // An unseeded sequence refuses to allocate.
    config.seed_repo_id_sequence(&ctx, 100_902).await?;
    config
        .insert_repos(
            &ctx,
            &[(
                RepositoryId::new(1),
                RepositoryName("repo/a".to_string()),
                GitSourceOfTruth::Reserved,
            )],
        )
        .await?;
    config
        .update_mutation_id_by_repo_names_for_reserved_repos(
            &ctx,
            &[RepositoryName("repo/a".to_string())],
            4242,
        )
        .await?;

    let params = params_for(&["repo/a"]);
    with_just_knobs_async(
        JustKnobsInMemory::new(HashMap::from([
            (ATTACH_JK.to_string(), KnobVal::Bool(true)),
            (ALLOCATE_FROM_SEQUENCE_JK.to_string(), KnobVal::Bool(true)),
        ])),
        async {
            let outcome = reserve_repos_ids(ctx.clone(), &config, &params)
                .await
                .expect("reserve_repos_ids should succeed and attach");
            match outcome {
                ReserveOutcome::AttachedToInflight { mutation_id } => {
                    assert_eq!(mutation_id, 4242);
                }
                ReserveOutcome::Reserved(_) => {
                    panic!("expected AttachedToInflight, got Reserved")
                }
            }
            anyhow::Ok(())
        }
        .boxed(),
    )
    .await?;
    Ok(())
}

#[mononoke::fbinit_test]
async fn null_mutation_window(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let config = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
    // An unseeded sequence refuses to allocate.
    config.seed_repo_id_sequence(&ctx, 100_902).await?;
    config
        .insert_repos(
            &ctx,
            &[(
                RepositoryId::new(1),
                RepositoryName("repo/a".to_string()),
                GitSourceOfTruth::Reserved,
            )],
        )
        .await?;

    let params = params_for(&["repo/a"]);
    with_just_knobs_async(
        JustKnobsInMemory::new(HashMap::from([
            (ATTACH_JK.to_string(), KnobVal::Bool(true)),
            (ALLOCATE_FROM_SEQUENCE_JK.to_string(), KnobVal::Bool(true)),
        ])),
        async {
            let err = reserve_repos_ids(ctx.clone(), &config, &params)
                .await
                .expect_err("expected an error for a null-mutation reserved repo");
            match &err {
                scs_errors::ServiceError::Request(req) => {
                    assert!(
                        format!("{req:?}").contains("in progress"),
                        "message should mention 'in progress', got: {req:?}"
                    );
                }
                other => panic!("expected Request error, got: {other:?}"),
            }
            anyhow::Ok(())
        }
        .boxed(),
    )
    .await?;
    Ok(())
}

#[mononoke::fbinit_test]
async fn split_brain_guard(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let config = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
    // An unseeded sequence refuses to allocate.
    config.seed_repo_id_sequence(&ctx, 100_902).await?;
    // Insert reserved, stamp a mutation id, then flip it to Mononoke.
    config
        .insert_repos(
            &ctx,
            &[(
                RepositoryId::new(1),
                RepositoryName("repo/a".to_string()),
                GitSourceOfTruth::Reserved,
            )],
        )
        .await?;
    config
        .update_mutation_id_by_repo_names_for_reserved_repos(
            &ctx,
            &[RepositoryName("repo/a".to_string())],
            7,
        )
        .await?;
    config
        .update_source_of_truth_by_mutation_id(&ctx, GitSourceOfTruth::Mononoke, 7)
        .await?;

    let params = params_for(&["repo/a"]);
    with_just_knobs_async(
        JustKnobsInMemory::new(HashMap::from([
            (ATTACH_JK.to_string(), KnobVal::Bool(true)),
            (ALLOCATE_FROM_SEQUENCE_JK.to_string(), KnobVal::Bool(true)),
        ])),
        async {
            let err = reserve_repos_ids(ctx.clone(), &config, &params)
                .await
                .expect_err("expected an error for a non-reserved (mononoke) repo");
            match &err {
                scs_errors::ServiceError::Request(req) => {
                    assert!(
                        format!("{req:?}").contains("DANGER"),
                        "message should mention 'DANGER', got: {req:?}"
                    );
                }
                other => panic!("expected Request error, got: {other:?}"),
            }
            anyhow::Ok(())
        }
        .boxed(),
    )
    .await?;
    Ok(())
}

#[mononoke::fbinit_test]
async fn mixed_batch(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let config = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
    // An unseeded sequence refuses to allocate.
    config.seed_repo_id_sequence(&ctx, 100_902).await?;
    config
        .insert_repos(
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
    // Stamp the two reserved repos with DIFFERENT mutation ids.
    config
        .update_mutation_id_by_repo_names_for_reserved_repos(
            &ctx,
            &[RepositoryName("repo/a".to_string())],
            100,
        )
        .await?;
    config
        .update_mutation_id_by_repo_names_for_reserved_repos(
            &ctx,
            &[RepositoryName("repo/b".to_string())],
            200,
        )
        .await?;

    let params = params_for(&["repo/a", "repo/b"]);
    with_just_knobs_async(
        JustKnobsInMemory::new(HashMap::from([
            (ATTACH_JK.to_string(), KnobVal::Bool(true)),
            (ALLOCATE_FROM_SEQUENCE_JK.to_string(), KnobVal::Bool(true)),
        ])),
        async {
            let err = reserve_repos_ids(ctx.clone(), &config, &params)
                .await
                .expect_err("expected an error for a batch spanning multiple mutations");
            match &err {
                scs_errors::ServiceError::Request(req) => {
                    assert!(
                        format!("{req:?}").contains("multiple in-flight mutations"),
                        "message should mention 'multiple in-flight mutations', got: {req:?}"
                    );
                }
                other => panic!("expected Request error, got: {other:?}"),
            }
            anyhow::Ok(())
        }
        .boxed(),
    )
    .await?;
    Ok(())
}

#[mononoke::fbinit_test]
async fn jk_off(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let config = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
    // An unseeded sequence refuses to allocate.
    config.seed_repo_id_sequence(&ctx, 100_902).await?;
    config
        .insert_repos(
            &ctx,
            &[(
                RepositoryId::new(1),
                RepositoryName("repo/a".to_string()),
                GitSourceOfTruth::Reserved,
            )],
        )
        .await?;
    config
        .update_mutation_id_by_repo_names_for_reserved_repos(
            &ctx,
            &[RepositoryName("repo/a".to_string())],
            4242,
        )
        .await?;

    let params = params_for(&["repo/a"]);
    with_just_knobs_async(
        JustKnobsInMemory::new(HashMap::from([
            (ATTACH_JK.to_string(), KnobVal::Bool(false)),
            (ALLOCATE_FROM_SEQUENCE_JK.to_string(), KnobVal::Bool(true)),
        ])),
        async {
            let err = reserve_repos_ids(ctx.clone(), &config, &params)
                .await
                .expect_err("expected today's behavior (error) when the knob is off");
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
async fn attach_happy_path_multi_repo(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let config = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
    // An unseeded sequence refuses to allocate.
    config.seed_repo_id_sequence(&ctx, 100_902).await?;
    // Two reserved repos both stamped with the SAME mutation id: a
    // duplicate multi-repo request should dedup to one mutation and attach.
    config
        .insert_repos(
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
    config
        .update_mutation_id_by_repo_names_for_reserved_repos(
            &ctx,
            &[
                RepositoryName("repo/a".to_string()),
                RepositoryName("repo/b".to_string()),
            ],
            4242,
        )
        .await?;

    let params = params_for(&["repo/a", "repo/b"]);
    with_just_knobs_async(
        JustKnobsInMemory::new(HashMap::from([
            (ATTACH_JK.to_string(), KnobVal::Bool(true)),
            (ALLOCATE_FROM_SEQUENCE_JK.to_string(), KnobVal::Bool(true)),
        ])),
        async {
            let outcome = reserve_repos_ids(ctx.clone(), &config, &params)
                .await
                .expect("reserve_repos_ids should succeed and attach for multi-repo");
            match outcome {
                ReserveOutcome::AttachedToInflight { mutation_id } => {
                    assert_eq!(mutation_id, 4242);
                }
                ReserveOutcome::Reserved(_) => {
                    panic!("expected AttachedToInflight, got Reserved")
                }
            }
            anyhow::Ok(())
        }
        .boxed(),
    )
    .await?;
    Ok(())
}

#[mononoke::fbinit_test]
async fn attach_lookup_none(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let config = SqlGitSourceOfTruthConfigBuilder::with_sqlite_in_memory()?.build();
    // An unseeded sequence refuses to allocate.
    config.seed_repo_id_sequence(&ctx, 100_902).await?;
    // Only one of the two requested repos has a seeded (reserved+stamped)
    // row; the other has NO row at all. The absent repo makes the batch
    // non-attachable (lookup returns None => `any_non_reserved`), so the
    // whole request must fail closed rather than attaching to the single
    // reserved mutation.
    config
        .insert_repos(
            &ctx,
            &[(
                RepositoryId::new(1),
                RepositoryName("repo/a".to_string()),
                GitSourceOfTruth::Reserved,
            )],
        )
        .await?;
    config
        .update_mutation_id_by_repo_names_for_reserved_repos(
            &ctx,
            &[RepositoryName("repo/a".to_string())],
            4242,
        )
        .await?;

    let params = params_for(&["repo/a", "repo/absent"]);
    with_just_knobs_async(
        JustKnobsInMemory::new(HashMap::from([
            (ATTACH_JK.to_string(), KnobVal::Bool(true)),
            (ALLOCATE_FROM_SEQUENCE_JK.to_string(), KnobVal::Bool(true)),
        ])),
        async {
            let err = reserve_repos_ids(ctx.clone(), &config, &params)
                .await
                .expect_err("expected an error when a requested repo has no row");
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

#[mononoke::test]
fn batch_at_cap_is_allowed() {
    check_batch_size(2, 2).expect("a batch exactly at the cap must be allowed");
}

#[mononoke::test]
fn batch_over_cap_is_rejected() {
    let err = check_batch_size(3, 2).expect_err("a batch over the cap must be rejected");
    // An Internal error would be retried by `create_repos_in_mononoke`
    // rather than reported to the caller.
    let scs_errors::ServiceError::Request(request_error) = &err else {
        panic!("expected Request error, got: {err:?}");
    };
    let message = format!("{request_error:?}");
    assert!(
        message.contains('3'),
        "message should state the requested count, got: {message}"
    );
    assert!(
        message.contains('2'),
        "message should state the cap, got: {message}"
    );
}

#[mononoke::test]
fn empty_batch_is_rejected() {
    let err = check_batch_size(0, BatchSizeTier::Default.max_batch_size())
        .expect_err("an empty batch must be rejected");
    let scs_errors::ServiceError::Request(request_error) = &err else {
        panic!("expected Request error, got: {err:?}");
    };
    let message = format!("{request_error:?}");
    assert!(
        message.contains("empty"),
        "message should say the batch was empty, got: {message}"
    );
}

#[mononoke::test]
fn tiers_admit_the_batches_they_are_sized_for() {
    let elevated = BatchSizeTier::Elevated.max_batch_size();
    let default = BatchSizeTier::Default.max_batch_size();

    // Real runs through the bulk-import pipeline: ~40, 95, 134.
    for count in [40, 95, 134] {
        check_batch_size(count, elevated).unwrap_or_else(|err| {
            panic!("a pipeline batch of {count} must be allowed, got: {err:?}")
        });
    }
    // p50 of all observed traffic is 1: the common case is one repo.
    check_batch_size(1, default).expect("a single-repo batch must always be allowed");
}

#[mononoke::test]
fn only_source_control_admits_the_largest_batches_on_record() {
    // 588 (par-msl, the all-time high) and the 700+ MTK Wearables say they
    // expect. Source Control has issued batches this size and can still do
    // so; a delegated grant deliberately cannot, so a group member has to
    // split, ask for the cap to be raised, or hand the batch back to Source
    // Control. Sizing the delegated tier around its two largest outliers
    // would leave it bounding nothing. Encoded so the decision is
    // discoverable rather than a surprise at the flip.
    let source_control = BatchSizeTier::SourceControl.max_batch_size();
    let elevated = BatchSizeTier::Elevated.max_batch_size();
    for count in [588, 700] {
        check_batch_size(count, source_control).unwrap_or_else(|err| {
            panic!("Source Control must still be able to issue {count}, got: {err:?}")
        });
        check_batch_size(count, elevated)
            .expect_err(&format!("{count} is expected to exceed the elevated tier"));
    }
}

#[mononoke::test]
fn the_default_tier_does_not_admit_the_observed_p95() {
    // Also deliberate, and the sharper edge of the two. Observed over 32
    // days: p95 66, max 139 -- both from ad-hoc Thrift Fiddle onboarding
    // runs. Under a default of 50 those callers must split their batches,
    // join the group, or be recognised as Source Control, which the
    // identity check should already do for a human on the team. Anyone else
    // doing bulk creation from an unrecognised identity is exactly what the
    // cap is for, so this is the intended bite rather than collateral.
    let default = BatchSizeTier::Default.max_batch_size();
    check_batch_size(66, default).expect_err("the observed p95 is expected to exceed default");
    check_batch_size(139, default).expect_err("the observed max is expected to exceed default");
}
