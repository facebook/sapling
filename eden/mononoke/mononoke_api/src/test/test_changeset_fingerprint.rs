/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::Result;
use content_manifest_derivation::RootContentManifestId;
use derivation_queue_thrift::DerivationPriority;
use fbinit::FacebookInit;
use fixtures::Linear;
use fixtures::TestRepoFixture;
use fsnodes::RootFsnodeId;
use mononoke_macros::mononoke;
use mononoke_types::DateTime;
use repo_derived_data::RepoDerivedDataRef;
use tests_utils::CreateCommitContext;

use crate::CoreContext;
use crate::FingerprintVersion;
use crate::Mononoke;

/// Decode a hex string into bytes for expected hash comparisons.
fn from_hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[mononoke::fbinit_test]
async fn test_content_fingerprint_same_tree_different_metadata(
    fb: FacebookInit,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, _commits, _dag) = Linear::get_repo_and_dag(fb).await;

    let root_id = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("a", "hello")
        .commit()
        .await?;

    let cs_id_1 = CreateCommitContext::new(&ctx, &repo, vec![root_id])
        .set_author("alice")
        .set_author_date(DateTime::from_timestamp(1000000, 0).unwrap())
        .set_message("first")
        .add_file("b", "world")
        .commit()
        .await?;

    let cs_id_2 = CreateCommitContext::new(&ctx, &repo, vec![root_id])
        .set_author("bob")
        .set_author_date(DateTime::from_timestamp(2000000, 0).unwrap())
        .set_message("second")
        .add_file("b", "world")
        .commit()
        .await?;

    let mononoke = Mononoke::new_test(vec![("test".to_string(), repo)]).await?;
    let repo_ctx = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;

    let cs_1 = repo_ctx
        .changeset(cs_id_1)
        .await?
        .expect("changeset exists");
    let cs_2 = repo_ctx
        .changeset(cs_id_2)
        .await?
        .expect("changeset exists");

    let fp_v1_1 = cs_1.content_fingerprint(FingerprintVersion::V1).await?;
    let fp_v1_2 = cs_2.content_fingerprint(FingerprintVersion::V1).await?;

    // Computed by deriving RootFsnodeId; recompute via test failure if fsnode
    // serialization changes.
    let expected_v1 = from_hex("3e9c7c5940beec0c4130d556bc06b5c5895db4e7f31fe79b517b150de56670fa");
    assert_eq!(
        fp_v1_1, expected_v1,
        "V1 fingerprint should match the expected RootFsnodeId blake2 hash"
    );
    assert_eq!(
        fp_v1_1, fp_v1_2,
        "V1: same file tree should produce the same fingerprint regardless of metadata"
    );

    let fp_v2_1 = cs_1.content_fingerprint(FingerprintVersion::V2).await?;
    let fp_v2_2 = cs_2.content_fingerprint(FingerprintVersion::V2).await?;

    let expected_v2 = from_hex("e7d50f33bfabb947b126c74cf56951014be9de433cc6bb880de7baf96cc1e958");
    assert_eq!(
        fp_v2_1, expected_v2,
        "V2 fingerprint should match the expected RootContentManifestId blake2 hash"
    );
    assert_eq!(
        fp_v2_1, fp_v2_2,
        "V2: same file tree should produce the same fingerprint regardless of metadata"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_content_fingerprint_different_trees(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, _commits, _dag) = Linear::get_repo_and_dag(fb).await;

    let root_id = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("a", "hello")
        .commit()
        .await?;

    let cs_id_1 = CreateCommitContext::new(&ctx, &repo, vec![root_id])
        .add_file("b", "x")
        .commit()
        .await?;

    let cs_id_2 = CreateCommitContext::new(&ctx, &repo, vec![root_id])
        .add_file("b", "y")
        .commit()
        .await?;

    let mononoke = Mononoke::new_test(vec![("test".to_string(), repo)]).await?;
    let repo_ctx = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;

    let cs_1 = repo_ctx
        .changeset(cs_id_1)
        .await?
        .expect("changeset exists");
    let cs_2 = repo_ctx
        .changeset(cs_id_2)
        .await?
        .expect("changeset exists");

    let fp_v1_1 = cs_1.content_fingerprint(FingerprintVersion::V1).await?;
    let fp_v1_2 = cs_2.content_fingerprint(FingerprintVersion::V1).await?;

    // Computed by deriving RootFsnodeId; recompute via test failure if fsnode
    // serialization changes.
    assert_eq!(
        fp_v1_1,
        from_hex("7ff1a986081b6eda05e3a74e901bce62e6c8b85d7a4ef13127fd2810c6793fb6"),
        "V1 fingerprint for tree with file b='x' (RootFsnodeId blake2)"
    );
    assert_eq!(
        fp_v1_2,
        from_hex("43594d20f45c61bba7f9740525a87568cfe6745b065c512dcf3a9f8afdf9541a"),
        "V1 fingerprint for tree with file b='y' (RootFsnodeId blake2)"
    );
    assert_ne!(
        fp_v1_1, fp_v1_2,
        "V1: different trees must produce different fingerprints"
    );

    let fp_v2_1 = cs_1.content_fingerprint(FingerprintVersion::V2).await?;
    let fp_v2_2 = cs_2.content_fingerprint(FingerprintVersion::V2).await?;

    assert_eq!(
        fp_v2_1,
        from_hex("ac3d7f7d6776b657d8d46160212a48915014f4357eb8917ccab0a4f3f3479ca9"),
        "V2 fingerprint for tree with file b='x' (RootContentManifestId blake2)"
    );
    assert_eq!(
        fp_v2_2,
        from_hex("57d4f0e450c7e20e43fe6bda5261f8f769b7aa183e108009198f6b6d91595bc4"),
        "V2 fingerprint for tree with file b='y' (RootContentManifestId blake2)"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_content_fingerprint_empty_tree(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, _commits, _dag) = Linear::get_repo_and_dag(fb).await;

    let root_id = CreateCommitContext::new_root(&ctx, &repo).commit().await?;

    let mononoke = Mononoke::new_test(vec![("test".to_string(), repo)]).await?;
    let repo_ctx = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;

    let cs = repo_ctx
        .changeset(root_id)
        .await?
        .expect("changeset exists");

    let fp_v1 = cs.content_fingerprint(FingerprintVersion::V1).await?;
    // Computed by deriving RootFsnodeId; recompute via test failure if fsnode
    // serialization changes.
    assert_eq!(
        fp_v1,
        from_hex("dcb470d973f4d3ba7e34a3113934a2c555e76f683c9934ac7e40c6a07e7c00d1"),
        "Empty tree V1 fingerprint (RootFsnodeId blake2) should be stable"
    );

    let fp_v2 = cs.content_fingerprint(FingerprintVersion::V2).await?;
    assert_eq!(
        fp_v2,
        from_hex("d91fa09362fcac4ce6633c92a4e0b4ea3b2823ac45de48a68da32a4d65939b04"),
        "Empty tree V2 fingerprint (RootContentManifestId blake2) should be stable"
    );

    Ok(())
}

/// V1 and V2 must derive different manifests, so their fingerprints must
/// differ for any non-degenerate tree. Catches the failure mode where both
/// match arms accidentally call the same `derive::<>`.
#[mononoke::fbinit_test]
async fn test_content_fingerprint_v1_v2_diverge(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, _commits, _dag) = Linear::get_repo_and_dag(fb).await;

    let cs_id = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("a", "hello")
        .commit()
        .await?;

    let mononoke = Mononoke::new_test(vec![("test".to_string(), repo)]).await?;
    let repo_ctx = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;

    let cs = repo_ctx.changeset(cs_id).await?.expect("changeset exists");

    let fp_v1 = cs.content_fingerprint(FingerprintVersion::V1).await?;
    let fp_v2 = cs.content_fingerprint(FingerprintVersion::V2).await?;

    assert_ne!(
        fp_v1, fp_v2,
        "V1 (RootFsnodeId) and V2 (RootContentManifestId) must produce different fingerprints"
    );

    Ok(())
}

/// Pin the structural mapping: V1 must equal blake2(RootFsnodeId), V2 must
/// equal blake2(RootContentManifestId). Catches a swapped match arm where
/// the divergence test alone cannot tell which derivation backs which version.
#[mononoke::fbinit_test]
async fn test_content_fingerprint_dispatch_structural(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, _commits, _dag) = Linear::get_repo_and_dag(fb).await;

    let cs_id = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("a", "hello")
        .commit()
        .await?;

    let mononoke = Mononoke::new_test(vec![("test".to_string(), repo)]).await?;
    let repo_ctx = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;

    let cs = repo_ctx.changeset(cs_id).await?.expect("changeset exists");

    let rdd = repo_ctx.repo().repo_derived_data();
    let fsnode_root: RootFsnodeId = rdd.derive(&ctx, cs_id, DerivationPriority::LOW).await?;
    let cm_root: RootContentManifestId = rdd.derive(&ctx, cs_id, DerivationPriority::LOW).await?;

    let expected_v1 = fsnode_root.into_fsnode_id().blake2().as_ref().to_vec();
    let expected_v2 = cm_root
        .into_content_manifest_id()
        .blake2()
        .as_ref()
        .to_vec();

    let fp_v1 = cs.content_fingerprint(FingerprintVersion::V1).await?;
    let fp_v2 = cs.content_fingerprint(FingerprintVersion::V2).await?;

    assert_eq!(
        fp_v1, expected_v1,
        "V1 must equal blake2(RootFsnodeId), not blake2(RootContentManifestId)"
    );
    assert_eq!(
        fp_v2, expected_v2,
        "V2 must equal blake2(RootContentManifestId), not blake2(RootFsnodeId)"
    );

    Ok(())
}
