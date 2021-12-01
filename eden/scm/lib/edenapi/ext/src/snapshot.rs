/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::num::NonZeroU64;
use std::time::Duration;

use anyhow::bail;
use anyhow::format_err;
use anyhow::Context;
use anyhow::Result;
use edenapi::api::EdenApi;
use edenapi_types::AnyFileContentId;
use edenapi_types::AnyId;
use edenapi_types::BonsaiChangesetContent;
use edenapi_types::BonsaiFileChange;
use edenapi_types::SnapshotRawData;
use edenapi_types::SnapshotRawFiles;
use edenapi_types::UploadSnapshotResponse;
use futures::StreamExt;
use futures::TryStreamExt;
use minibytes::Bytes;

use crate::util::calc_contentid;

pub async fn upload_snapshot(
    api: &(impl EdenApi + ?Sized),
    repo: String,
    data: SnapshotRawData,
    custom_duration_secs: Option<u64>,
    copy_from_bubble_id: Option<NonZeroU64>,
) -> Result<UploadSnapshotResponse> {
    let SnapshotRawData {
        files,
        author,
        hg_parents,
        time,
        tz,
    } = data;
    let SnapshotRawFiles {
        root,
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
        // rel_path is relative to the repo root
        .map(|(rel_path, file_type, tracked)| {
            let mut abs_path = root.clone();
            abs_path.push(&rel_path);
            let bytes = std::fs::read(abs_path.as_repo_path().as_str())?;
            let content_id = calc_contentid(&bytes);
            Ok((
                (rel_path, file_type, content_id, tracked),
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
        api.ephemeral_prepare(repo.clone(), custom_duration_secs.map(Duration::from_secs))
            .await?
            .entries
            .next()
            .await
            .context("Failed to create ephemeral bubble")??
    };
    let bubble_id = prepare_response.bubble_id;
    let file_content_tokens = {
        let downcast_error = "incorrect upload token, failed to downcast 'token.data.id' to 'AnyId::AnyFileContentId::ContentId' type";
        // upload file contents first, receiving upload tokens
        api.process_files_upload(
            repo.clone(),
            upload_data,
            Some(bubble_id),
            copy_from_bubble_id,
        )
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
            Some(bubble_id),
        )
        .await?;
    let changeset_response = response
        .entries
        .next()
        .await
        .context("Failed to create changeset")??;
    Ok(UploadSnapshotResponse {
        changeset_token: changeset_response.token,
        bubble_id,
    })
}
