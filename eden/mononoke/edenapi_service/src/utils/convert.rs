/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! convert.rs - Conversions between Mercurial and Mononoke types.
//!
//! Mercurial and Mononoke use different types to represent similar
//! concepts, such as paths, identifiers, etc. While these types
//! fundamentally represent the same things, they often differ in
//! implementation details, adding some friction when converting.
//!
//! In theory, the conversions should never fail since these types
//! are used to represent the same data on the client and server
//! respectively, so any conversion failure should be considered
//! a bug. Nonetheless, since these types often differ substantially
//! in implementation, it is possible that conversion failures may occur
//! in practice.

use std::collections::BTreeMap;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use edenapi_types::commit::BonsaiFileChange;
use edenapi_types::token::UploadToken;
use edenapi_types::token::UploadTokenData;
use edenapi_types::token::UploadTokenMetadata;
use edenapi_types::AnyFileContentId;
use edenapi_types::AnyId;
use edenapi_types::HgChangesetContent;
use ephemeral_blobstore::BubbleId;
use mercurial_types::blobs::Extra;
use mercurial_types::blobs::RevlogChangeset;
use mercurial_types::HgManifestId;
use mercurial_types::HgNodeHash;
use mononoke_api::CreateChange;
use mononoke_api::CreateChangeFile;
use mononoke_api::CreateChangeFileContents;
use mononoke_api::CreateCopyInfo;
use mononoke_types::path::MPath;
use mononoke_types::DateTime;
use mononoke_types::NonRootMPath;
use types::RepoPath;
use types::RepoPathBuf;

use crate::errors::ErrorKind;

/// Convert a Mercurial `RepoPath` or `RepoPathBuf` into an `MPath`.
/// The input will be copied due to differences in data representation.
pub fn to_mpath(path: impl AsRef<RepoPath>) -> Result<MPath> {
    let path_bytes = path.as_ref().as_byte_slice();
    MPath::new(path_bytes).with_context(|| ErrorKind::InvalidPath(path_bytes.to_vec()))
}

/// Convert an `NonRootMPath` into a Mercurial `RepoPathBuf`.
/// The input will be copied due to differences in data representation.
pub fn to_hg_path(path: &NonRootMPath) -> Result<RepoPathBuf> {
    RepoPathBuf::from_utf8(path.to_vec()).with_context(|| ErrorKind::InvalidPath(path.to_vec()))
}

pub fn to_revlog_changeset(cs: HgChangesetContent) -> Result<RevlogChangeset> {
    Ok(RevlogChangeset {
        p1: cs.parents.p1().cloned().map(HgNodeHash::from),
        p2: cs.parents.p2().cloned().map(HgNodeHash::from),
        manifestid: HgManifestId::new(HgNodeHash::from(cs.manifestid)),
        extra: Extra::new(
            cs.extras
                .into_iter()
                .map(|extra| (extra.key.into(), extra.value.into()))
                .collect::<BTreeMap<_, _>>(),
        ),
        files: cs
            .files
            .into_iter()
            .map(|file| {
                to_mpath(file)?
                    .into_optional_non_root_path()
                    .context(ErrorKind::UnexpectedEmptyPath)
            })
            .collect::<Result<_, _>>()?,
        message: cs.message.into(),
        time: DateTime::from_timestamp(cs.time, cs.tz)?,
        user: cs.user.into(),
    })
}

pub fn to_create_change(fc: BonsaiFileChange, bubble_id: Option<BubbleId>) -> Result<CreateChange> {
    fn extract_size(metadata: Option<UploadTokenMetadata>) -> Option<u64> {
        metadata
            .map(|UploadTokenMetadata::FileContentTokenMetadata(metadata)| metadata.content_size)
    }
    let verify = move |token: &UploadToken| -> Result<()> {
        // TODO: Verify signature on upload token
        if token.data.bubble_id != bubble_id.map(Into::into) {
            bail!("Wrong bubble id on upload token")
        }
        Ok(())
    };
    match fc {
        BonsaiFileChange::Change {
            file_type,
            upload_token,
            copy_info,
        } => {
            verify(&upload_token)?;
            if let UploadTokenData {
                id: AnyId::AnyFileContentId(AnyFileContentId::ContentId(content_id)),
                bubble_id: _,
                metadata,
            } = upload_token.data
            {
                Ok(CreateChange::Tracked(
                    CreateChangeFile {
                        contents: CreateChangeFileContents::Existing {
                            file_id: content_id.into(),
                            maybe_size: extract_size(metadata),
                        },
                        file_type: file_type.into(),
                        git_lfs: None,
                    },
                    copy_info
                        .map(|(path, index)| {
                            anyhow::Ok(CreateCopyInfo::new(to_mpath(path)?, index))
                        })
                        .transpose()?,
                ))
            } else {
                bail!("Invalid upload token format, missing content id")
            }
        }
        BonsaiFileChange::UntrackedChange {
            file_type,
            upload_token,
        } => {
            verify(&upload_token)?;
            // TODO: Verify signature on upload token
            if let UploadTokenData {
                id: AnyId::AnyFileContentId(AnyFileContentId::ContentId(content_id)),
                bubble_id: _,
                metadata,
            } = upload_token.data
            {
                Ok(CreateChange::Untracked(CreateChangeFile {
                    contents: CreateChangeFileContents::Existing {
                        file_id: content_id.into(),
                        maybe_size: extract_size(metadata),
                    },
                    file_type: file_type.into(),
                    git_lfs: None,
                }))
            } else {
                bail!("Invalid upload token format, missing content id")
            }
        }
        BonsaiFileChange::UntrackedDeletion => Ok(CreateChange::UntrackedDeletion),
        BonsaiFileChange::Deletion => Ok(CreateChange::Deletion),
    }
}
