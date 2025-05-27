/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use anyhow::bail;
use async_trait::async_trait;
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
use filestore::FilestoreConfigRef;
use futures::StreamExt;
use futures::stream;
use gotham_ext::handler::SlapiCommitIdentityScheme;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::HgNodeHash;
use mononoke_api::ChangesetSpecifier;
use mononoke_api::MononokeRepo;
use mononoke_api::Repo;
use mononoke_api_hg::HgDataId;
use mononoke_api_hg::HgRepoContext;
use repo_blobstore::RepoBlobstoreRef;
use repo_identity::RepoIdentityRef;

use super::HandlerResult;
use super::SaplingRemoteApiHandler;
use super::SaplingRemoteApiMethod;
use super::handler::SaplingRemoteApiContext;

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

async fn maybe_copy_file<R: MononokeRepo>(
    hg_repo_ctx: HgRepoContext<R>,
    id: AnyFileContentId,
    bubble_id: Option<BubbleId>,
    copy_from_bubble_id: BubbleId,
) -> Result<Lookup> {
    Ok(if let Some(bubble_id) = bubble_id {
        let repo = hg_repo_ctx.repo_ctx().repo();
        match hg_repo_ctx
            .open_bubble(copy_from_bubble_id)
            .await?
            .copy_file_to_bubble(
                hg_repo_ctx.ctx(),
                repo.repo_identity().id(),
                repo.repo_blobstore().clone(),
                bubble_id,
                *repo.filestore_config(),
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

async fn check_file<R: MononokeRepo>(
    repo: HgRepoContext<R>,
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
async fn check_request_item<R: MononokeRepo>(
    repo: HgRepoContext<R>,
    item: LookupRequest,
) -> Result<LookupResponse> {
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
            .filenode_exists(<HgFileNodeId as HgDataId<R>>::from_node_hash(
                HgNodeHash::from(id),
            ))
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
        AnyId::GitChangesetId(id) => {
            let specifier = ChangesetSpecifier::GitSha1(id.into());
            match repo.repo_ctx().resolve_specifier(specifier).await? {
                Some(_) => Lookup::Present(None),
                None => Lookup::NotPresent,
            }
        }
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
impl SaplingRemoteApiHandler for LookupHandler {
    type Request = Batch<LookupRequest>;
    type Response = LookupResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::Lookup;
    const ENDPOINT: &'static str = "/lookup";
    const SUPPORTED_FLAVOURS: &'static [SlapiCommitIdentityScheme] = &[
        SlapiCommitIdentityScheme::Hg,
        SlapiCommitIdentityScheme::Git,
    ];

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();
        let tokens = request
            .batch
            .into_iter()
            .map(move |item| check_request_item(repo.clone(), item));

        Ok(stream::iter(tokens)
            .buffer_unordered(MAX_CONCURRENT_LOOKUPS_PER_REQUEST)
            .boxed())
    }
}
