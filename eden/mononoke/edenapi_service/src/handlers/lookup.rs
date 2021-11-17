/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{bail, Error};
use async_trait::async_trait;
use futures::{stream, StreamExt};

use edenapi_types::{
    AnyId, Batch, FileContentTokenMetadata, IndexableId, LookupRequest, LookupResponse,
    LookupResult, UploadToken, UploadTokenMetadata,
};
use ephemeral_blobstore::{BubbleId, StorageLocation};
use mercurial_types::{HgChangesetId, HgFileNodeId, HgManifestId, HgNodeHash};
use mononoke_api_hg::{HgDataId, HgRepoContext};

use super::{EdenApiHandler, EdenApiMethod, HandlerResult};

const MAX_CONCURRENT_LOOKUPS_PER_REQUEST: usize = 10000;

/// Check if the item is present already and generate a token if it is.
/// Return None if the item has to be uploaded
/// Item can be of any type:
///     * file content id (canonical and sha1, sha256 aliases)
///     * hg filenode id
///     * hg tree id
///     * hg changeset id
async fn check_request_item(
    repo: HgRepoContext,
    item: LookupRequest,
) -> Result<LookupResponse, Error> {
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
    let old_bubble_id = item.bubble_id;
    let bubble_id = old_bubble_id.map(BubbleId::new);
    let hg_on_bubble_error = "Hg derived data cannot be stored in bubbles";
    let lookup = match item.id {
        AnyId::AnyFileContentId(id) => {
            let content_id = repo.convert_file_to_content_id(id, bubble_id).await?;
            if let Some(content_id) = content_id {
                // Reasons why check if content id is present:
                // 1. If content_id is provided, we haven't yet checked it is actually
                // in the blobstore
                // 2. Maybe alias was written to blobstore but the actual blob has not
                // 3. We want to do a comprehensive lookup here
                if repo.is_file_present_by_contentid(content_id).await? {
                    Lookup::Present(Some(
                        FileContentTokenMetadata {
                            content_size: repo
                                .fetch_file_content_size(content_id, bubble_id)
                                .await?,
                        }
                        .into(),
                    ))
                } else {
                    Lookup::NotPresent
                }
            } else {
                Lookup::NotPresent
            }
        }
        AnyId::BonsaiChangesetId(id) => repo
            .changeset_exists_by_bonsai(
                id.into(),
                match bubble_id {
                    Some(id) => StorageLocation::Bubble(id),
                    None => StorageLocation::Persistent,
                },
            )
            .await?
            .into(),
        // Hg derived data does not exist on bubbles, let's fail fast
        AnyId::HgFilenodeId(id) => (if bubble_id.is_none() {
            repo.filenode_exists(HgFileNodeId::from_node_hash(HgNodeHash::from(id)))
                .await?
        } else {
            bail!(hg_on_bubble_error)
        })
        .into(),
        AnyId::HgTreeId(id) => (if bubble_id.is_none() {
            repo.tree_exists(HgManifestId::new(HgNodeHash::from(id)))
                .await?
        } else {
            bail!(hg_on_bubble_error)
        })
        .into(),
        AnyId::HgChangesetId(id) => (if bubble_id.is_none() {
            repo.changeset_exists(HgChangesetId::new(HgNodeHash::from(id)))
                .await?
        } else {
            bail!(hg_on_bubble_error)
        })
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
