/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::ensure;
use anyhow::Error;
use anyhow::Result;
use blobstore::Loadable;
use bytes::Bytes;
use bytes::BytesMut;
use cloned::cloned;
use context::CoreContext;
use edenapi_service::utils::to_hg_path;
use edenapi_types::commit::BonsaiExtra;
use edenapi_types::commit::BonsaiParents;
use edenapi_types::commit::HgInfo;
use edenapi_types::AnyFileContentId;
use edenapi_types::AnyId;
use edenapi_types::BonsaiFileChange;
use edenapi_types::Extra;
use edenapi_types::HgChangesetContent;
use edenapi_types::HgFilenodeData;
use edenapi_types::IdenticalChangesetContent;
use edenapi_types::Parents;
use edenapi_types::RepoPathBuf;
use edenapi_types::UploadHgChangeset;
use edenapi_types::UploadToken;
use edenapi_types::UploadTreeEntry;
use mercurial_types::blobs::HgBlobChangeset;
use mercurial_types::fetch_manifest_envelope;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstore;
use sorted_vector_map::SortedVectorMap;

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

pub fn concatenate_bytes(vec_of_bytes: Vec<Bytes>) -> Bytes {
    let mut bytes_mut = BytesMut::new();
    for b in vec_of_bytes {
        bytes_mut.extend_from_slice(&b);
    }
    bytes_mut.freeze()
}

pub fn to_identical_changeset(
    css: (HgBlobChangeset, BonsaiChangeset),
) -> Result<IdenticalChangesetContent> {
    let (hg_cs, bcs) = css;
    let BonsaiChangesetMut {
        parents,
        author,
        author_date,
        committer: _,
        committer_date: _,
        message,
        hg_extra: _,
        git_extra_headers,
        file_changes,
        is_snapshot,
        git_tree_hash,
        git_annotated_tag,
    } = bcs.clone().into_mut();

    let hg_info = HgInfo {
        node_id: hg_cs.get_changeset_id().into_nodehash().into(),
        manifestid: hg_cs.manifestid().into_nodehash().into(),
        extras: hg_cs
            .extra()
            .iter()
            .map(|(key, value)| Extra {
                key: key.to_vec(),
                value: value.to_vec(),
            })
            .collect(),
    };

    let bonsai_parents = BonsaiParents::from_iter(parents.clone().iter().map(|p| (*p).into()));

    // Ensure items are indeed equivalent between bonsai and hg changeset
    ensure!(author.as_bytes() == hg_cs.user(), "Author mismatch");
    ensure!(author_date == *hg_cs.time(), "Time mismatch");
    ensure!(message.as_bytes() == hg_cs.message(), "Message mismatch");
    ensure!(git_tree_hash.is_none(), "Unexpected git tree hash found");
    ensure!(
        git_annotated_tag.is_none(),
        "Unexpected git annotated tag found"
    );
    ensure!(
        git_extra_headers.is_none(),
        "Unexpected git extra headers found"
    );

    Ok(IdenticalChangesetContent {
        bcs_id: bcs.get_changeset_id().into(),
        hg_parents: hg_cs.parents().into(),
        bonsai_parents,
        author: author.to_string(),
        time: author_date.timestamp_secs(),
        tz: author_date.tz_offset_secs(),
        extras: bcs
            .hg_extra()
            .map(|(key, value)| BonsaiExtra {
                key: key.to_string(),
                value: value.to_vec(),
            })
            .collect(),
        file_changes: to_file_change(&file_changes, parents.iter().copied())?,
        message: message.to_string(),
        is_snapshot,
        hg_info,
    })
}

fn to_file_change(
    map: &SortedVectorMap<NonRootMPath, FileChange>,
    parents: impl Iterator<Item = ChangesetId> + Clone,
) -> Result<Vec<(RepoPathBuf, BonsaiFileChange)>> {
    let res = map
        .into_iter()
        .map(|(path, fc)| {
            let path = RepoPathBuf::from_string(path.to_string())?;
            let fc = match fc {
                FileChange::Deletion => BonsaiFileChange::Deletion,
                FileChange::UntrackedDeletion => BonsaiFileChange::UntrackedDeletion,
                FileChange::Change(tc) => BonsaiFileChange::Change {
                    upload_token: UploadToken::new_fake_token(
                        AnyId::AnyFileContentId(AnyFileContentId::ContentId(
                            tc.content_id().into(),
                        )),
                        None,
                    ),
                    file_type: tc.file_type().try_into()?,
                    copy_info: match tc.copy_from() {
                        Some((path, cs_id)) => {
                            cloned!(mut parents);
                            let index = parents
                                .position(|parent| parent == *cs_id)
                                .ok_or(anyhow::anyhow!("Copy from info doesn't match parents"))?;
                            Some((to_hg_path(path)?, index))
                        }
                        None => None,
                    },
                },
                FileChange::UntrackedChange(uc) => BonsaiFileChange::UntrackedChange {
                    upload_token: UploadToken::new_fake_token(
                        AnyId::AnyFileContentId(AnyFileContentId::ContentId(
                            uc.content_id().into(),
                        )),
                        None,
                    ),
                    file_type: uc.file_type().try_into()?,
                },
            };
            Ok((path, fc))
        })
        .collect::<Result<Vec<(RepoPathBuf, BonsaiFileChange)>, Error>>()?;
    Ok(res)
}
