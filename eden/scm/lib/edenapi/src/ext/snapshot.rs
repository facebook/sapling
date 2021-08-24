/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::api::EdenApi;
use crate::ext::util::calc_contentid;

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, format_err, Context, Result};
use edenapi_types::{
    AnyFileContentId, AnyId, BonsaiChangesetContent, BonsaiFileChange, SnapshotRawData,
    SnapshotRawFiles, UploadSnapshotResponse,
};
use futures::{StreamExt, TryStreamExt};
use minibytes::Bytes;

pub async fn upload_snapshot(
    api: &(impl EdenApi + ?Sized),
    repo: String,
    data: SnapshotRawData,
) -> Result<UploadSnapshotResponse> {
    let SnapshotRawData {
        files,
        author,
        hg_parents,
        time,
        tz,
    } = data;
    let SnapshotRawFiles {
        modified,
        added,
        removed,
        untracked,
        missing,
    } = files;
    #[derive(PartialEq, Eq)]
    enum Type {
        Tracked,
        Untracked,
    }
    use Type::*;
    let (need_upload, mut upload_data): (Vec<_>, Vec<_>) = modified
        .into_iter()
        .chain(added.into_iter())
        .map(|(p, t)| (p, t, Tracked))
        .chain(
            // TODO(yancouto): Don't upload untracked files if they're too big.
            untracked.into_iter().map(|(p, t)| (p, t, Untracked)),
        )
        .map(|(path, file_type, tracked)| {
            let bytes = std::fs::read(path.as_repo_path().as_str())?;
            let content_id = calc_contentid(&bytes);
            Ok((
                (path, file_type, content_id, tracked),
                (content_id, Bytes::from_owner(bytes)),
            ))
        })
        .collect::<Result<Vec<_>, std::io::Error>>()?
        .into_iter()
        .unzip();

    // Deduplicate upload data
    let mut uniques = BTreeSet::new();
    upload_data.retain(|(content_id, _)| uniques.insert(*content_id));
    let upload_data = upload_data
        .into_iter()
        .map(|(content_id, data)| (AnyFileContentId::ContentId(content_id), data))
        .collect();

    let prepare_response = {
        api.ephemeral_prepare(repo.clone())
            .await?
            .entries
            .next()
            .await
            .context("Failed to create ephemeral bubble")??
    };
    let bubble_id = Some(prepare_response.bubble_id);
    let file_content_tokens = {
        let downcast_error = "incorrect upload token, failed to downcast 'token.data.id' to 'AnyId::AnyFileContentId::ContentId' type";
        // upload file contents first, receiving upload tokens
        api.process_files_upload(repo.clone(), upload_data, bubble_id)
            .await?
            .entries
            .try_collect::<Vec<_>>()
            .await?
            .into_iter()
            .map(|token| {
                let content_id = match token.data.id {
                    AnyId::AnyFileContentId(AnyFileContentId::ContentId(id)) => id,
                    _ => bail!(downcast_error),
                };
                Ok((content_id, token))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?
    };
    let mut response = api
        .upload_bonsai_changeset(
            repo,
            BonsaiChangesetContent {
                hg_parents,
                author,
                time,
                tz,
                extra: vec![],
                file_changes: need_upload
                    .into_iter()
                    .map(|(path, file_type, cid, tracked)| {
                        let upload_token = file_content_tokens
                            .get(&cid)
                            .with_context(|| {
                                format_err!(
                                    "unexpected error: upload token is missing for ContentId({})",
                                    cid
                                )
                            })?
                            .clone();
                        let change = if tracked == Tracked {
                            BonsaiFileChange::Change {
                                file_type,
                                upload_token,
                            }
                        } else {
                            BonsaiFileChange::UntrackedChange {
                                file_type,
                                upload_token,
                            }
                        };
                        Ok((path, change))
                    })
                    .chain(
                        removed
                            .into_iter()
                            .map(|path| Ok((path, BonsaiFileChange::Deletion))),
                    )
                    .chain(
                        missing
                            .into_iter()
                            .map(|path| Ok((path, BonsaiFileChange::UntrackedDeletion))),
                    )
                    .collect::<anyhow::Result<_>>()?,
                message: "".to_string(),
                is_snapshot: true,
            },
            bubble_id,
        )
        .await?;
    let changeset_response = response
        .entries
        .next()
        .await
        .context("Failed to create changeset")??;
    Ok(UploadSnapshotResponse {
        changeset_token: changeset_response.token,
    })
}
