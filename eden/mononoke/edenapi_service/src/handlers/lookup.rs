/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Result;
use async_trait::async_trait;
use futures::stream;
use futures::StreamExt;

use edenapi_types::AnyFileContentId;
use edenapi_types::AnyId;
use edenapi_types::Batch;
use edenapi_types::FileContentTokenMetadata;
use edenapi_types::IndexableId;
use edenapi_types::LookupRequest;
use edenapi_types::LookupResponse;
use edenapi_types::LookupResult;
use edenapi_types::UploadToken;
use edenapi_types::UploadTokenMetadata;
use ephemeral_blobstore::BubbleId;
use ephemeral_blobstore::StorageLocation;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::HgNodeHash;
use mononoke_api_hg::HgDataId;
use mononoke_api_hg::HgRepoContext;

use super::EdenApiHandler;
use super::EdenApiMethod;
use super::HandlerResult;

const MAX_CONCURRENT_LOOKUPS_PER_REQUEST: usize = 10000;

enum Lookup {
    NotPresent,
    Present(Option<UploadTokenMetadata>),
}

impl From<bool> for Lookup {
    fn from(b: bool) -> Self {
        if b {
            Self::Present(None)
        } else {
            Self::NotPresent
        }
    }
}

async fn maybe_copy_file(
    repo: HgRepoContext,
    id: AnyFileContentId,
    bubble_id: Option<BubbleId>,
    copy_from_bubble_id: BubbleId,
) -> Result<Lookup> {
    Ok(if let Some(bubble_id) = bubble_id {
        let blob_repo = repo.repo().blob_repo();
        match repo
            .open_bubble(copy_from_bubble_id)
            .await?
            .copy_file_to_bubble(
                repo.ctx(),
                blob_repo.get_repoid(),
                blob_repo.blobstore().clone(),
                bubble_id,
                blob_repo.filestore_config(),
                id.into(),
            )
            .await?
        {
            None => Lookup::NotPresent,
            Some(data) => Lookup::Present(Some(
                FileContentTokenMetadata {
                    content_size: data.total_size,
                }
                .into(),
            )),
        }
    } else {
        Lookup::NotPresent
    })
}

async fn check_file(
    repo: HgRepoContext,
    id: AnyFileContentId,
    bubble_id: Option<BubbleId>,
    copy_from_bubble: Option<BubbleId>,
) -> Result<Lookup> {
    let content_id = repo.convert_file_to_content_id(id, bubble_id).await?;
    let lookup = if let Some(content_id) = content_id {
        // Reasons why check if content id is present:
        // 1. If content_id is provided, we haven't yet checked it is actually
        // in the blobstore
        // 2. Maybe alias was written to blobstore but the actual blob has not
        // 3. We want to do a comprehensive lookup here
        if repo.is_file_present(content_id, bubble_id).await? {
            Lookup::Present(Some(
                FileContentTokenMetadata {
                    content_size: repo.fetch_file_content_size(content_id, bubble_id).await?,
                }
                .into(),
            ))
        } else {
            Lookup::NotPresent
        }
    } else {
        Lookup::NotPresent
    };
    let lookup = match (lookup, copy_from_bubble) {
        (Lookup::NotPresent, Some(copy_bid)) => maybe_copy_file(repo, id, bubble_id, copy_bid)
            .await
            .unwrap_or(Lookup::NotPresent),
        (l, _) => l,
    };
    Ok(lookup)
}

/// Check if the item is present already and generate a token if it is.
/// Return None if the item has to be uploaded
/// Item can be of any type:
///     * file content id (canonical and sha1, sha256 aliases)
///     * hg filenode id
///     * hg tree id
///     * hg changeset id
async fn check_request_item(repo: HgRepoContext, item: LookupRequest) -> Result<LookupResponse> {
    let old_bubble_id = item.bubble_id;
    let bubble_id = old_bubble_id.map(BubbleId::new);
    if item.copy_from_bubble_id.is_some() && !matches!(item.id, AnyId::AnyFileContentId(_)) {
        bail!("copy_from_bubble_id is only supported with files")
    }
    if item.bubble_id.is_some()
        && matches!(
            item.id,
            AnyId::HgFilenodeId(_) | AnyId::HgTreeId(_) | AnyId::HgChangesetId(_)
        )
    {
        bail!("Hg derived data cannot be stored in bubbles")
    }
    let lookup = match item.id {
        AnyId::AnyFileContentId(id) => {
            check_file(
                repo,
                id,
                bubble_id,
                item.copy_from_bubble_id.map(BubbleId::new),
            )
            .await?
        }
        AnyId::BonsaiChangesetId(id) => repo
            .changeset_exists(
                id.into(),
                match bubble_id {
                    Some(id) => StorageLocation::Bubble(id),
                    None => StorageLocation::Persistent,
                },
            )
            .await?
            .into(),
        // Hg derived data does not exist on bubbles, let's fail fast
        AnyId::HgFilenodeId(id) => repo
            .filenode_exists(HgFileNodeId::from_node_hash(HgNodeHash::from(id)))
            .await?
            .into(),
        AnyId::HgTreeId(id) => repo
            .tree_exists(HgManifestId::new(HgNodeHash::from(id)))
            .await?
            .into(),
        AnyId::HgChangesetId(id) => repo
            .hg_changeset_exists(HgChangesetId::new(HgNodeHash::from(id)))
            .await?
            .into(),
    };
    let result = match lookup {
        Lookup::NotPresent => LookupResult::NotPresent(IndexableId {
            id: item.id,
            bubble_id: old_bubble_id,
        }),
        Lookup::Present(None) => {
            let token = UploadToken::new_fake_token(item.id.clone(), item.bubble_id);
            LookupResult::Present(token)
        }
        Lookup::Present(Some(metadata)) => {
            let token = UploadToken::new_fake_token_with_metadata(
                item.id.clone(),
                item.bubble_id,
                metadata,
            );
            LookupResult::Present(token)
        }
    };

    Ok(LookupResponse { result })
}

/// Process lookup (batched) request.
pub struct LookupHandler;

#[async_trait]
impl EdenApiHandler for LookupHandler {
    type Request = Batch<LookupRequest>;
    type Response = LookupResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: EdenApiMethod = EdenApiMethod::Lookup;
    const ENDPOINT: &'static str = "/lookup";

    async fn handler(
        repo: HgRepoContext,
        _path: Self::PathExtractor,
        _query: Self::QueryStringExtractor,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let tokens = request
            .batch
            .into_iter()
            .map(move |item| check_request_item(repo.clone(), item));

        Ok(stream::iter(tokens)
            .buffer_unordered(MAX_CONCURRENT_LOOKUPS_PER_REQUEST)
            .boxed())
    }
}
