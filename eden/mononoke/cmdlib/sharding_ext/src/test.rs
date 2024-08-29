/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_macros::mononoke;

use crate::decode_repo_name;
use crate::encode_repo_name;
use crate::RepoShard;

#[mononoke::test]
fn basic_create_repo_shard_test() {
    let repo_shard = RepoShard::with_repo_name("repo");
    assert_eq!(repo_shard.repo_name, "repo".to_string());

    let repo_shard = RepoShard::with_source_and_target("source_repo", "target_repo");
    assert_eq!(repo_shard.repo_name, "source_repo".to_string());
    assert_eq!(repo_shard.target_repo_name, Some("target_repo".to_string()));
}

#[mononoke::test]
fn create_repo_shard_with_sizeless_chunks_test() {
    let repo_shard = RepoShard::with_chunks("source_repo", "4_OF_16", Some("target_repo"))
        .expect("Failed in creating RepoShard");
    assert_eq!(repo_shard.repo_name, "source_repo".to_string());
    assert_eq!(repo_shard.target_repo_name, Some("target_repo".to_string()));
    assert_eq!(repo_shard.total_chunks, Some(16));
    assert_eq!(repo_shard.chunk_id, Some(4));
    assert_eq!(repo_shard.chunk_size, None);
}

#[mononoke::test]
fn create_invalid_repo_shard_with_sizeless_chunks_test() {
    RepoShard::with_chunks("source_repo", "4_OF_-16", Some("target_repo"))
        .expect_err("Should have failed in creating RepoShard");
    RepoShard::with_chunks("source_repo", "_4_OF_16", Some("target_repo"))
        .expect_err("Should have failed in creating RepoShard");
    RepoShard::with_chunks("source_repo", "4_OF_16_", Some("target_repo"))
        .expect_err("Should have failed in creating RepoShard");
    RepoShard::with_chunks("source_repo", "4__OF__16", Some("target_repo"))
        .expect_err("Should have failed in creating RepoShard");
    RepoShard::with_chunks("source_repo", "4-OF-16", Some("target_repo"))
        .expect_err("Should have failed in creating RepoShard");
    RepoShard::with_chunks("source_repo", "16", Some("target_repo"))
        .expect_err("Should have failed in creating RepoShard");
    RepoShard::with_chunks("source_repo", "4.0_OF_16", Some("target_repo"))
        .expect_err("Should have failed in creating RepoShard");
    RepoShard::with_chunks("source_repo", "4_of_16", Some("target_repo"))
        .expect_err("Should have failed in creating RepoShard");
    RepoShard::with_chunks("source_repo", "4_OF_OF_16", Some("target_repo"))
        .expect_err("Should have failed in creating RepoShard");
    RepoShard::with_chunks("source_repo", "4_OF_16_OF_", Some("target_repo"))
        .expect_err("Should have failed in creating RepoShard");
    RepoShard::with_chunks("source_repo", "4 16", Some("target_repo"))
        .expect_err("Should have failed in creating RepoShard");
    RepoShard::with_chunks("source_repo", "4_CHUNK_16", Some("target_repo"))
        .expect_err("Should have failed in creating RepoShard");
}

#[mononoke::test]
fn create_repo_shard_with_sized_chunks_test() {
    let repo_shard =
        RepoShard::with_chunks("source_repo", "4_OF_16_SIZE_1000", Some("target_repo"))
            .expect("Failed in creating RepoShard");
    assert_eq!(repo_shard.repo_name, "source_repo".to_string());
    assert_eq!(repo_shard.target_repo_name, Some("target_repo".to_string()));
    assert_eq!(repo_shard.total_chunks, Some(16));
    assert_eq!(repo_shard.chunk_id, Some(4));
    assert_eq!(repo_shard.chunk_size, Some(1000));
}

#[mononoke::test]
fn create_invalid_repo_shard_with_sized_chunks_test() {
    RepoShard::with_chunks("source_repo", "4_OF_16_SIZED_1000", Some("target_repo"))
        .expect_err("Should have failed in creating RepoShard");
    RepoShard::with_chunks("source_repo", "4_OF_16_1000", Some("target_repo"))
        .expect_err("Should have failed in creating RepoShard");
    RepoShard::with_chunks("source_repo", "4_OF_16-1000", Some("target_repo"))
        .expect_err("Should have failed in creating RepoShard");
    RepoShard::with_chunks("source_repo", "4_SIZE_16", Some("target_repo"))
        .expect_err("Should have failed in creating RepoShard");
    RepoShard::with_chunks("source_repo", "4_OF_16_SIZE_", Some("target_repo"))
        .expect_err("Should have failed in creating RepoShard");
    RepoShard::with_chunks("source_repo", "4_OF_16_SIZE_200.5", Some("target_repo"))
        .expect_err("Should have failed in creating RepoShard");
    RepoShard::with_chunks("source_repo", "4_OF_16_SIZE_2-6", Some("target_repo"))
        .expect_err("Should have failed in creating RepoShard");
    RepoShard::with_chunks("source_repo", "4_OF_16_SIZE_SIZE_200", Some("target_repo"))
        .expect_err("Should have failed in creating RepoShard");
}

#[mononoke::test]
fn create_basic_repo_shard_with_shard_id_test() {
    let repo_shard = RepoShard::from_shard_id("repo").expect("Failed while creating RepoShard");
    assert_eq!(repo_shard.repo_name, "repo".to_string());
}

#[mononoke::test]
fn create_x_repo_repo_shard_with_shard_id_test() {
    let repo_shard = RepoShard::from_shard_id("source_repo_TO_target_repo")
        .expect("Failed while creating RepoShard");
    assert_eq!(repo_shard.repo_name, "source_repo".to_string());
    assert_eq!(repo_shard.target_repo_name, Some("target_repo".to_string()));
}

#[mononoke::test]
fn create_invalid_x_repo_repo_shard_with_shard_id_test() {
    let repo_shard = RepoShard::from_shard_id("source_repo_TO__TO_target_repo")
        .expect("Failed while creating RepoShard");
    assert_eq!(repo_shard.repo_name, "source_repo".to_string());
    assert_eq!(
        repo_shard.target_repo_name,
        Some("_TO_target_repo".to_string())
    );

    let repo_shard = RepoShard::from_shard_id("source_repo_TO_target_repo_TO_another_repo")
        .expect("Failed while creating RepoShard");
    assert_eq!(repo_shard.repo_name, "source_repo".to_string());
    assert_eq!(
        repo_shard.target_repo_name,
        Some("target_repo_TO_another_repo".to_string())
    );

    let repo_shard = RepoShard::from_shard_id("JustARepoWithTOInItsName")
        .expect("Failed while creating RepoShard");
    assert_eq!(repo_shard.repo_name, "JustARepoWithTOInItsName".to_string());
    assert_eq!(repo_shard.target_repo_name, None);

    let repo_shard =
        RepoShard::from_shard_id("JustARepoWithTOInItsName_TO_AnotherRepoWithTOInItsName")
            .expect("Failed while creating RepoShard");
    assert_eq!(repo_shard.repo_name, "JustARepoWithTOInItsName".to_string());
    assert_eq!(
        repo_shard.target_repo_name,
        Some("AnotherRepoWithTOInItsName".to_string())
    );
}

#[mononoke::test]
fn create_chunked_repo_shard_with_shard_id_test() {
    let repo_shard =
        RepoShard::from_shard_id("repo_CHUNK_2_OF_15").expect("Failed while creating RepoShard");
    assert_eq!(repo_shard.repo_name, "repo".to_string());
    assert_eq!(repo_shard.chunk_id, Some(2));
    assert_eq!(repo_shard.total_chunks, Some(15));
    assert_eq!(repo_shard.chunk_size, None);
}

#[mononoke::test]
fn create_invalid_chunked_repo_shard_with_shard_id_test() {
    RepoShard::from_shard_id("repo_CHUNK__2_OF_15")
        .expect_err("Should have failed in creating RepoShard");
    RepoShard::from_shard_id("repo_CHUNK_2_OFF_15")
        .expect_err("Should have failed in creating RepoShard");
    RepoShard::from_shard_id("repo_CHUNK_2_OF_15_CHUNK")
        .expect_err("Should have failed in creating RepoShard");
}

#[mononoke::test]
fn create_sized_chunked_repo_shard_with_shard_id_test() {
    let repo_shard = RepoShard::from_shard_id("repo_CHUNK_2_OF_15_SIZE_1000")
        .expect("Failed while creating RepoShard");
    assert_eq!(repo_shard.repo_name, "repo".to_string());
    assert_eq!(repo_shard.chunk_id, Some(2));
    assert_eq!(repo_shard.total_chunks, Some(15));
    assert_eq!(repo_shard.chunk_size, Some(1000));
}

#[mononoke::test]
fn create_invalid_sized_chunked_repo_shard_with_shard_id_test() {
    RepoShard::from_shard_id("repo_CHUNK__2_OF_15_SIZE_1000")
        .expect_err("Should have failed in creating RepoShard");
    RepoShard::from_shard_id("repo_CHUNK_2_OF_15_SIZE_1000_")
        .expect_err("Should have failed in creating RepoShard");
    let repo_shard = RepoShard::from_shard_id("repo_SIZE_1000_CHUNK_2_OF_15")
        .expect("Failed while creating RepoShare");
    assert_eq!(repo_shard.repo_name, "repo_SIZE_1000".to_string());
    assert_eq!(repo_shard.target_repo_name, None);
    assert_eq!(repo_shard.chunk_id, Some(2));
    assert_eq!(repo_shard.total_chunks, Some(15));
    assert_eq!(repo_shard.chunk_size, None);
    let repo_shard =
        RepoShard::from_shard_id("repo_SIZE_1000").expect("Failed while creating RepoShare");
    assert_eq!(repo_shard.repo_name, "repo_SIZE_1000".to_string());
}

#[mononoke::test]
fn create_x_repo_chunked_repo_shard_with_shard_id_test() {
    let repo_shard = RepoShard::from_shard_id("repo_TO_another_repo_CHUNK_2_OF_15")
        .expect("Failed while creating RepoShard");
    assert_eq!(repo_shard.repo_name, "repo".to_string());
    assert_eq!(
        repo_shard.target_repo_name,
        Some("another_repo".to_string())
    );
    assert_eq!(repo_shard.chunk_id, Some(2));
    assert_eq!(repo_shard.total_chunks, Some(15));
    assert_eq!(repo_shard.chunk_size, None);
}

#[mononoke::test]
fn create_x_repo_sized_chunked_repo_shard_with_shard_id_test() {
    let repo_shard = RepoShard::from_shard_id("repo_TO_another_repo_CHUNK_2_OF_15_SIZE_2000")
        .expect("Failed while creating RepoShard");
    assert_eq!(repo_shard.repo_name, "repo".to_string());
    assert_eq!(
        repo_shard.target_repo_name,
        Some("another_repo".to_string())
    );
    assert_eq!(repo_shard.chunk_id, Some(2));
    assert_eq!(repo_shard.total_chunks, Some(15));
    assert_eq!(repo_shard.chunk_size, Some(2000));
}

#[mononoke::test]
fn encode_decode_repo_name_test() {
    assert_eq!(
        decode_repo_name(&encode_repo_name("whatsapp/server")),
        "whatsapp/server".to_string()
    );
    assert_eq!(
        decode_repo_name(&encode_repo_name("fbsource")),
        "fbsource".to_string()
    );
    assert_eq!(
        decode_repo_name(&encode_repo_name("repo/with/lots/of/backslashes")),
        "repo/with/lots/of/backslashes".to_string()
    );
    assert_eq!(
        decode_repo_name(&encode_repo_name("repo+with+lots+of+pluses")),
        "repo+with+lots+of+pluses".to_string()
    );
    assert_eq!(
        decode_repo_name(&encode_repo_name("repo//with//double_slashes")),
        "repo//with//double_slashes".to_string()
    );
    assert_eq!(
        decode_repo_name(&encode_repo_name("/+/repo/+/")),
        "/+/repo/+/".to_string()
    );
}

#[mononoke::test]
fn create_full_blown_repo_shard_with_shard_id_test() {
    let repo_shard =
        RepoShard::from_shard_id("whatsapp/server_TO_another+repo_CHUNK_2_OF_15_SIZE_2000")
            .expect("Failed while creating RepoShard");
    assert_eq!(repo_shard.repo_name, "whatsapp/server".to_string());
    assert_eq!(
        repo_shard.target_repo_name,
        Some("another+repo".to_string())
    );
    assert_eq!(repo_shard.chunk_id, Some(2));
    assert_eq!(repo_shard.total_chunks, Some(15));
    assert_eq!(repo_shard.chunk_size, Some(2000));
}
