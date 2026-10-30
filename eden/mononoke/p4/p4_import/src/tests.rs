/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::task::Poll;

use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use futures::stream;
use metaconfig_types::FilestoreParams;
use mononoke_macros::mononoke;
use repo_blobstore::RepoBlobstore;
use test_repo_factory::TestRepoFactory;

use super::*;

#[facet::container]
struct TestRepo {
    #[facet]
    repo_blobstore: RepoBlobstore,
    #[facet]
    filestore_config: FilestoreConfig,
}

fn chunks(chunks: &[&[u8]]) -> impl Stream<Item = Result<Bytes>> + Send + use<> {
    let chunks: Vec<_> = chunks
        .iter()
        .map(|c| Ok(Bytes::copy_from_slice(c)))
        .collect();
    stream::iter(chunks)
}

async fn upload_and_fetch(
    ctx: &CoreContext,
    repo: &TestRepo,
    content: &[&[u8]],
) -> Result<(ContentMetadataV2, Bytes)> {
    let size = content.iter().map(|c| c.len() as u64).sum();
    let metadata = upload_content(ctx, repo, size, chunks(content)).await?;
    let fetched = filestore::fetch_concat(repo.repo_blobstore(), ctx, metadata.content_id).await?;
    Ok((metadata, fetched))
}

#[mononoke::fbinit_test]
async fn test_content_reads_back_byte_identical(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

    let (metadata, fetched) = upload_and_fetch(&ctx, &repo, &[b"hello\nworld\n"]).await?;

    assert_eq!(&b"hello\nworld\n"[..], fetched);
    assert_eq!(12, metadata.total_size);
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_content_stored_in_filestore_chunks_reads_back_byte_identical(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = TestRepoFactory::new(fb)?
        .with_config_override(|config| {
            config.filestore = Some(FilestoreParams {
                chunk_size: 1024 * 1024,
                concurrency: 2,
            })
        })
        .build()
        .await?;
    let content: Vec<Vec<u8>> = (0..5u8).map(|i| vec![i; 1024 * 1024 + 7]).collect();
    let content: Vec<&[u8]> = content.iter().map(Vec::as_slice).collect();

    let (metadata, fetched) = upload_and_fetch(&ctx, &repo, &content).await?;

    assert_eq!(content.concat(), fetched);
    assert_eq!(5 * (1024 * 1024 + 7), metadata.total_size);
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_empty_content(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

    let (metadata, fetched) = upload_and_fetch(&ctx, &repo, &[]).await?;

    assert!(fetched.is_empty());
    assert_eq!(0, metadata.total_size);
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_content_longer_than_size_is_rejected(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

    let error = upload_content(&ctx, &repo, 3, chunks(&[b"abcd"]))
        .await
        .unwrap_err();

    assert!(format!("{error:#}").contains("Invalid size"), "{error:?}");
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_content_shorter_than_size_is_rejected(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

    let error = upload_content(&ctx, &repo, 5, chunks(&[b"abcd"]))
        .await
        .unwrap_err();

    assert!(format!("{error:#}").contains("Invalid size"), "{error:?}");
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_same_content_gets_the_same_content_id(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

    let (first, _) = upload_and_fetch(&ctx, &repo, &[b"same bytes"]).await?;
    let (second, _) = upload_and_fetch(&ctx, &repo, &[b"same ", b"bytes"]).await?;

    assert_eq!(first.content_id, second.content_id);
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_symlink_is_stored_without_the_trailing_newline(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

    let metadata = upload_file_content(
        &ctx,
        &repo,
        FileType::Symlink,
        19,
        chunks(&[b"UPPERCASE-", b"TEST.TXT\n"]),
    )
    .await?;
    let fetched = filestore::fetch_concat(repo.repo_blobstore(), &ctx, metadata.content_id).await?;

    assert_eq!(&b"UPPERCASE-TEST.TXT"[..], fetched);
    assert_eq!(18, metadata.total_size);
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_symlink_without_trailing_newline_is_rejected(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

    let error = upload_file_content(&ctx, &repo, FileType::Symlink, 6, chunks(&[b"target"]))
        .await
        .unwrap_err();

    assert!(
        format!("{error:#}").contains("does not end with a newline"),
        "{error:?}"
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_symlink_with_two_trailing_newlines_is_rejected(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

    let error = upload_file_content(&ctx, &repo, FileType::Symlink, 8, chunks(&[b"target\n\n"]))
        .await
        .unwrap_err();

    assert!(
        format!("{error:#}").contains("more than one newline"),
        "{error:?}"
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_symlink_shorter_than_size_is_rejected(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

    let error = upload_file_content(&ctx, &repo, FileType::Symlink, 8, chunks(&[b"target\n"]))
        .await
        .unwrap_err();

    assert!(
        format!("{error:#}").contains("symlink content is 7 bytes but Perforce reports 8"),
        "{error:?}"
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_symlink_longer_than_size_is_rejected(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

    let error = upload_file_content(
        &ctx,
        &repo,
        FileType::Symlink,
        6,
        chunks(&[b"targ", b"et\n", b"never read"]),
    )
    .await
    .unwrap_err();

    assert!(
        format!("{error:#}").contains("longer than Perforce reports (6 bytes)"),
        "{error:?}"
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_symlink_over_64_kib_is_rejected_before_reading(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;
    let unreadable =
        stream::poll_fn(|_| -> Poll<Option<Result<Bytes>>> { panic!("content was read") });

    let error = upload_file_content(&ctx, &repo, FileType::Symlink, 64 * 1024 + 1, unreadable)
        .await
        .unwrap_err();

    assert!(
        format!("{error:#}").contains("symlink of 65537 bytes is larger than we support"),
        "{error:?}"
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_regular_file_keeps_its_trailing_newline(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

    let metadata =
        upload_file_content(&ctx, &repo, FileType::Regular, 7, chunks(&[b"target\n"])).await?;
    let fetched = filestore::fetch_concat(repo.repo_blobstore(), &ctx, metadata.content_id).await?;

    assert_eq!(&b"target\n"[..], fetched);
    assert_eq!(7, metadata.total_size);
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_regular_file_of_the_wrong_size_is_rejected(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

    let error = upload_file_content(&ctx, &repo, FileType::Regular, 6, chunks(&[b"target\n"]))
        .await
        .unwrap_err();

    assert!(format!("{error:#}").contains("Invalid size"), "{error:?}");
    Ok(())
}
