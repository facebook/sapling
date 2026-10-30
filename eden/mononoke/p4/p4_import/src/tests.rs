/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::task::Poll;

use anyhow::anyhow;
use async_trait::async_trait;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use futures::StreamExt;
use futures::stream;
use futures::stream::BoxStream;
use metaconfig_types::FilestoreParams;
use mononoke_macros::mononoke;
use p4_types::Action;
use p4_types::FileRevision;
use p4_types::Integration;
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

/// Prints only the revisions in `contents`: printing any other fails the test.
struct FakeSource {
    contents: HashMap<String, &'static [u8]>,
}

#[async_trait]
impl ChangeSource for FakeSource {
    async fn describe(&self, change: u64) -> Result<Changelist> {
        Err(anyhow!("unexpected describe of {change}"))
    }

    async fn print(&self, file: &FileRevision) -> Result<BoxStream<'static, Result<Bytes>>> {
        let content = self
            .contents
            .get(&file.depot_path)
            .ok_or_else(|| anyhow!("unexpected print of {}", file.depot_path))?;
        Ok(chunks(&[content]).boxed())
    }

    async fn integrations(
        &self,
        files: &[&FileRevision],
    ) -> Result<HashMap<String, Vec<Integration>>> {
        Err(anyhow!("unexpected filelog of {files:?}"))
    }
}

fn revision(depot_path: &str, action: Action, file_type: &str, size: Option<u64>) -> FileRevision {
    FileRevision {
        depot_path: depot_path.to_owned(),
        action,
        file_type: file_type.to_owned(),
        revision: 1,
        size,
        digest: None,
    }
}

#[mononoke::fbinit_test]
async fn test_changelist_contents_are_stored_by_depot_path(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;
    let source = FakeSource {
        contents: HashMap::from([
            ("//test1/add.txt".to_owned(), &b"hello\n"[..]),
            ("//test1/run.sh".to_owned(), &b"#!/bin/sh\n"[..]),
            ("//test1/link".to_owned(), &b"add.txt\n"[..]),
        ]),
    };
    let changelist = Changelist {
        number: 42,
        user: "alice".to_owned(),
        time: 0,
        description: "mixed".to_owned(),
        files: vec![
            revision("//test1/add.txt", Action::Add, "text", Some(6)),
            revision("//test1/run.sh", Action::Edit, "text+x", Some(10)),
            revision("//test1/gone.txt", Action::Delete, "text", None),
            revision("//test1/link", Action::Add, "symlink", Some(8)),
            revision("//other/add.txt", Action::Add, "text", Some(6)),
        ],
    };

    let contents =
        upload_changelist_contents(&ctx, &repo, &source, "//test1/", &changelist).await?;

    let mut stored = Vec::new();
    for (path, metadata) in &contents {
        let content =
            filestore::fetch_concat(repo.repo_blobstore(), &ctx, metadata.content_id).await?;
        stored.push((path.as_str(), content));
    }
    stored.sort();
    assert_eq!(
        vec![
            ("//test1/add.txt", Bytes::from_static(b"hello\n")),
            ("//test1/link", Bytes::from_static(b"add.txt")),
            ("//test1/run.sh", Bytes::from_static(b"#!/bin/sh\n")),
        ],
        stored
    );
    Ok(())
}
