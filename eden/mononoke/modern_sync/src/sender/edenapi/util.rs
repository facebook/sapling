/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use blobstore::Loadable;
use context::CoreContext;
use edenapi_types::AnyFileContentId;
use edenapi_types::AnyId;
use edenapi_types::Extra;
use edenapi_types::HgChangesetContent;
use edenapi_types::HgFilenodeData;
use edenapi_types::Parents;
use edenapi_types::RepoPathBuf;
use edenapi_types::UploadHgChangeset;
use edenapi_types::UploadToken;
use edenapi_types::UploadTreeEntry;
use mercurial_types::blobs::HgBlobChangeset;
use mercurial_types::fetch_manifest_envelope;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use repo_blobstore::RepoBlobstore;

pub async fn from_tree_to_entry(
    id: HgManifestId,
    ctx: &CoreContext,
    repo_blobstore: &RepoBlobstore,
) -> Result<UploadTreeEntry> {
    let envelope = fetch_manifest_envelope(ctx, repo_blobstore, id).await?;
    let content = envelope.contents();

    let parents = match envelope.parents() {
        (None, None) => Parents::None,
        (Some(p1), None) => Parents::One(p1.into()),
        (None, Some(p2)) => Parents::One(p2.into()),
        (Some(p1), Some(p2)) => Parents::Two(p1.into(), p2.into()),
    };

    Ok(UploadTreeEntry {
        node_id: envelope.node_id().into(),
        data: content.to_vec(),
        parents,
    })
}

pub async fn from_id_to_filenode(
    file_id: HgFileNodeId,
    ctx: &CoreContext,
    repo_blobstore: &RepoBlobstore,
) -> Result<HgFilenodeData> {
    let file_node = file_id.load(ctx, repo_blobstore).await?;

    // These tokens are mostly implemented to make sure client sends content before uplaoding filenodes
    // but they're not really verified, given we're indeed sending the content, let's use a placeholder
    let content_id = file_node.content_id();
    let token = UploadToken::new_fake_token(
        AnyId::AnyFileContentId(AnyFileContentId::ContentId(content_id.into())),
        None,
    );

    Ok(HgFilenodeData {
        node_id: file_id.into_nodehash().into(),
        parents: file_node.hg_parents().into(),
        metadata: file_node.metadata().clone().to_vec(),
        file_content_upload_token: token,
    })
}

pub fn to_upload_hg_changeset(hg_cs: HgBlobChangeset) -> Result<UploadHgChangeset> {
    let extra = hg_cs
        .extra()
        .iter()
        .map(|(k, v)| Extra {
            key: k.to_vec(),
            value: v.to_vec(),
        })
        .collect();

    let hg_files: Result<Vec<RepoPathBuf>> = hg_cs
        .files()
        .iter()
        .map(edenapi_service::utils::to_hg_path)
        .collect();

    let hg_content = HgChangesetContent {
        parents: hg_cs.parents().into(),
        manifestid: hg_cs.manifestid().into_nodehash().into(),
        user: hg_cs.user().to_vec(),
        time: hg_cs.time().timestamp_secs(),
        tz: hg_cs.time().tz_offset_secs(),
        extras: extra,
        files: hg_files?,
        message: hg_cs.message().to_vec(),
    };

    Ok(UploadHgChangeset {
        node_id: hg_cs.get_changeset_id().into_nodehash().into(),
        changeset_content: hg_content,
    })
}
