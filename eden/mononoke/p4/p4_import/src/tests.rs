/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

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
