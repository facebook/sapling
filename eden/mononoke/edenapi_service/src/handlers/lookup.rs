/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{bail, Error};
use async_trait::async_trait;
use futures::{
    stream::{self, BoxStream},
    StreamExt,
};

use edenapi_types::{
    AnyId, Batch, FileContentTokenMetadata, LookupRequest, LookupResponse, UploadToken,
    UploadTokenMetadata,
};
use ephemeral_blobstore::BubbleId;
use mercurial_types::{HgChangesetId, HgFileNodeId, HgManifestId, HgNodeHash};
use mononoke_api_hg::{HgDataId, HgRepoContext};

use super::{EdenApiHandler, EdenApiMethod};

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
    index: usize,
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
    let bubble_id = item.bubble_id.map(BubbleId::new);
    let hg_on_bubble_error = "Hg derived data cannot be stored in bubbles";
    let lookup = match item.id {
        AnyId::AnyFileContentId(id) => {
            let content_id = repo.convert_file_to_content_id(id, bubble_id).await?;
            if let Some(content_id) = content_id {
                Lookup::Present(Some(
                    FileContentTokenMetadata {
                        content_size: repo.fetch_file_content_size(content_id, bubble_id).await?,
                    }
                    .into(),
                ))
            } else {
                Lookup::NotPresent
            }
        }
        AnyId::BonsaiChangesetId(id) => repo
            .changeset_exists_by_bonsai(id.into(), bubble_id)
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

    Ok(LookupResponse {
        index,
        token: match lookup {
            Lookup::NotPresent => None,
            Lookup::Present(None) => Some(UploadToken::new_fake_token(item.id, item.bubble_id)),
            Lookup::Present(Some(metadata)) => Some(UploadToken::new_fake_token_with_metadata(
                item.id,
                item.bubble_id,
                metadata,
            )),
        },
    })
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
    ) -> anyhow::Result<BoxStream<'async_trait, anyhow::Result<Self::Response>>> {
        let tokens = request
            .batch
            .into_iter()
            .enumerate()
            .map(move |(i, item)| check_request_item(repo.clone(), item, i));

        Ok(stream::iter(tokens)
            .buffer_unordered(MAX_CONCURRENT_LOOKUPS_PER_REQUEST)
            .boxed())
    }
}
